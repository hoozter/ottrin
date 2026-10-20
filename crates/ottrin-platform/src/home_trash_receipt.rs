//! Resolve an operation's exact home-Trash receipt without scanning other mounts.
//! External Trash has no provable origin binding, so it cannot supply Undo here.
use super::*;
use std::io::Read;
use std::os::unix::{ffi::OsStringExt, fs::OpenOptionsExt};

pub(super) fn candidates(
    original: &Path,
    identity: &PathMoveIdentity,
) -> Result<Vec<TrashItem>, PlatformError> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Ok(Vec::new());
    };
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    candidates_in(original, identity, &home, &data_home)
}

fn candidates_in(
    original: &Path,
    identity: &PathMoveIdentity,
    home: &Path,
    data_home: &Path,
) -> Result<Vec<TrashItem>, PlatformError> {
    let io = |error: std::io::Error| PlatformError::Io(error.to_string());
    let canonical_home = std::fs::canonicalize(home).map_err(io)?;
    let parent = original
        .parent()
        .ok_or_else(|| PlatformError::InvalidSource(original.display().to_string()))?;
    if !home.is_absolute()
        || !std::fs::canonicalize(parent)
            .map_err(io)?
            .starts_with(canonical_home)
    {
        return Ok(Vec::new());
    }
    let trash = data_home.join("Trash");
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(trash.join("files")).map_err(io)? {
        let entry = entry.map_err(io)?;
        // Inspect metadata only for the exact object that was just moved.
        // Unrelated entries may be huge, malformed, or backed by slow storage.
        if rename_identity(&entry.path()).ok().as_ref() != Some(identity) {
            continue;
        }
        let mut info_name = entry.file_name();
        info_name.push(".trashinfo");
        let info = trash.join("info").join(info_name);
        let mut file = match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&info)
        {
            Ok(file) if file.metadata().map_err(io)?.is_file() => file,
            _ => continue,
        };
        let mut text = String::new();
        if file.by_ref().take(65537).read_to_string(&mut text).is_err()
            || text.len() > 65536
            || text.lines().next() != Some("[Trash Info]")
        {
            continue;
        }
        let Some(encoded) = parse_trashinfo_field(&text, "Path") else {
            continue;
        };
        let decoded = percent_encoding::percent_decode_str(&encoded).collect();
        let stored = PathBuf::from(OsString::from_vec(decoded));
        if stored != original {
            continue;
        }
        let Some(name) = original.file_name() else {
            continue;
        };
        // Reuse the ordinary native item conversion and origin validation.
        let mut item = system_trash_item_to_ottrin(trash::TrashItem {
            id: info.into_os_string(),
            name: name.to_os_string(),
            original_parent: parent.to_path_buf(),
            time_deleted: -1,
        });
        item.origin_storage =
            trash_origin_storage_binding_at(&entry.path(), original, home, Some(data_home));
        matches.push(item);
    }
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn receipt_lookup_matches_native_identity_and_encoded_path_without_reading_unrelated_info() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let data = root.path().join("data");
        let trash = data.join("Trash");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::create_dir(trash.join("info")).unwrap();
        let original = home.join(OsString::from_vec(b"report %=\xff.txt".to_vec()));
        std::fs::write(&original, b"exact payload").unwrap();
        let identity = rename_identity(&original).unwrap();
        std::fs::rename(&original, trash.join("files/chosen")).unwrap();
        let encoded: String = original
            .as_os_str()
            .as_bytes()
            .iter()
            .map(|byte| format!("%{byte:02X}"))
            .collect();
        let info = format!("[Trash Info]\nPath={encoded}\nDeletionDate=2026-10-21T01:00:00\n");
        std::fs::write(trash.join("info/chosen.trashinfo"), &info).unwrap();
        std::fs::write(trash.join("files/unrelated"), b"other payload").unwrap();
        // Unrelated metadata must not participate in receipt parsing.
        std::fs::write(
            trash.join("info/unrelated.trashinfo"),
            "[Trash Info]\ninvalid\n",
        )
        .unwrap();
        let items = candidates_in(&original, &identity, &home, &data).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].original_path, original);
        assert_eq!(
            std::fs::read(&items[0].trash_path).unwrap(),
            b"exact payload"
        );
        assert!(items[0].origin_storage.is_some());
        assert_eq!(
            std::fs::read(trash.join("files/unrelated")).unwrap(),
            b"other payload"
        );

        // Ambiguous hard-linked entries must still fail closed.
        std::fs::hard_link(trash.join("files/chosen"), trash.join("files/alias")).unwrap();
        std::fs::write(trash.join("info/alias.trashinfo"), &info).unwrap();
        assert!(
            exact_trash_undo_receipt(
                &original,
                &identity,
                candidates_in(&original, &identity, &home, &data)
            )
            .is_none()
        );
        std::fs::write(
            trash.join("info/chosen.trashinfo"),
            "[Trash Info]\nPath=%XX\n",
        )
        .unwrap();
        std::fs::write(
            trash.join("info/alias.trashinfo"),
            "[Trash Info]\nPath=/wrong\n",
        )
        .unwrap();
        assert!(
            candidates_in(&original, &identity, &home, &data)
                .unwrap()
                .is_empty()
        );
    }
}
