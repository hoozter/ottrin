//! Terminal paired evidence authorizes only its exact interrupted release.
use super::*;

fn replace_bytes(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
fn refused(parent: &File, central: &File, name: &OsStr, claim: &Path, before: &[u8]) {
    assert!(Snapshot::load(parent, central, name, SnapshotAccess::Read).is_err());
    assert_eq!(std::fs::read(claim).unwrap(), before);
    assert!(undo_completed_copy(parent, central, name, |_| Ok(())).is_err());
    assert_eq!(std::fs::read(claim).unwrap(), before);
}

#[test]
fn windows_native_copy_claim_release_recovers_every_exact_prefix_without_rewriting_journals() {
    let root = tempfile::tempdir().unwrap();
    let (name, _, _cleanup) = completed_undo_fixture(root.path(), true, false);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let header = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
        .unwrap()
        .state
        .header
        .unwrap();
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    remove_undo_fixture_source(root.path());
    let local = root.path().join("destination").join(&name).join(LOCAL);
    let mirror = root.path().join("central").join(header.central_name());
    let journal = std::fs::read(&local).unwrap();
    assert_eq!(std::fs::read(&mirror).unwrap(), journal);
    let claim = root
        .path()
        .join("destination")
        .join(header.claim.test_relative_path());
    let full = std::fs::read(&claim).unwrap();
    let prefix = FRAME_HEADER + u32::from_le_bytes(full[8..12].try_into().unwrap()) as usize;
    assert!(full.len() > prefix + FRAME_HEADER);
    for cut in prefix..=full.len() {
        replace_bytes(&claim, &full[..cut]);
        let snapshot = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).unwrap();
        assert!(snapshot.state.undo_complete());
        assert_eq!(
            std::fs::read(&claim).unwrap(),
            &full[..cut],
            "read-only discovery changed prefix {cut}"
        );
        drop(snapshot);
        undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
        assert_eq!(std::fs::read(&claim).unwrap(), full, "release prefix {cut}");
        assert_eq!(std::fs::read(&local).unwrap(), journal);
        assert_eq!(std::fs::read(&mirror).unwrap(), journal);
        assert!(!root.path().join("destination/copy").try_exists().unwrap());
        assert_eq!(
            std::fs::read(
                root.path()
                    .join("destination")
                    .join(header.candidate_name())
                    .join("nested/file")
            )
            .unwrap(),
            b"original data"
        );
    }
}

#[test]
fn windows_native_copy_claim_release_preserves_unknown_tails_and_requires_both_terminal_journals() {
    let root = tempfile::tempdir().unwrap();
    let (name, _, _cleanup) = completed_undo_fixture(root.path(), false, false);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let header = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
        .unwrap()
        .state
        .header
        .unwrap();
    let local = root.path().join("destination").join(&name).join(LOCAL);
    let mirror = root.path().join("central").join(header.central_name());
    let nonterminal = std::fs::read(&local).unwrap();
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    let terminal = std::fs::read(&local).unwrap();
    let claim = root
        .path()
        .join("destination")
        .join(header.claim.test_relative_path());
    let full = std::fs::read(&claim).unwrap();
    let prefix = FRAME_HEADER + u32::from_le_bytes(full[8..12].try_into().unwrap()) as usize;
    let partial = &full[..prefix + 12];
    replace_bytes(&claim, partial);
    // A local terminal state alone cannot authorize repair.
    replace_bytes(&mirror, &nonterminal);
    refused(&parent, &central, &name, &claim, partial);
    assert_eq!(std::fs::read(&local).unwrap(), terminal);
    assert_eq!(std::fs::read(&mirror).unwrap(), nonterminal);
    // Even matching nonterminal journals cannot adopt an incomplete release.
    replace_bytes(&local, &nonterminal);
    refused(&parent, &central, &name, &claim, partial);
    assert_eq!(std::fs::read(&local).unwrap(), nonterminal);
    assert_eq!(std::fs::read(&mirror).unwrap(), nonterminal);
    replace_bytes(&local, &terminal);
    replace_bytes(&mirror, &terminal);
    let mut wrong_header = partial.to_vec();
    wrong_header[prefix] ^= 1;
    let mut wrong_digest = full.clone();
    wrong_digest[prefix + 52] ^= 1;
    let mut extra = full.clone();
    extra.extend_from_slice(b"unrecognized bytes");
    for unknown in [
        full[..prefix - 1].to_vec(),
        wrong_header,
        wrong_digest,
        extra,
    ] {
        replace_bytes(&claim, &unknown);
        refused(&parent, &central, &name, &claim, &unknown);
        assert_eq!(std::fs::read(&local).unwrap(), terminal);
        assert_eq!(std::fs::read(&mirror).unwrap(), terminal);
    }
    // Recognizing the ordinary stream never grants deletion of an unknown ADS.
    replace_bytes(&claim, partial);
    let mut ads = claim.as_os_str().to_os_string();
    ads.push(":unrecognized");
    let ads = std::path::PathBuf::from(ads);
    std::fs::write(&ads, b"retain unknown stream").unwrap();
    refused(&parent, &central, &name, &claim, partial);
    assert_eq!(std::fs::read(&ads).unwrap(), b"retain unknown stream");
    std::fs::remove_file(ads).unwrap();
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    assert_eq!(std::fs::read(&claim).unwrap(), full);
    assert_eq!(std::fs::read(&local).unwrap(), terminal);
    assert_eq!(std::fs::read(&mirror).unwrap(), terminal);
    // Old terminal journals cannot repair a later generation's partial release.
    let transaction = super::super::super::super::random_transaction_id().unwrap();
    let later =
        claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &transaction).unwrap();
    assert!(matches!(
        later.abort_unstarted(io("owned future generation cancellation")),
        PlatformError::Io(_)
    ));
    let newer = std::fs::read(&claim).unwrap();
    assert!(!newer.starts_with(&full));
    let release = FRAME_HEADER + u32::from_le_bytes(newer[8..12].try_into().unwrap()) as usize;
    replace_bytes(&claim, &newer[..release + 12]);
    let snapshot = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read).unwrap();
    assert!(snapshot.claim.is_none());
    drop(snapshot);
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    assert_eq!(std::fs::read(&claim).unwrap(), &newer[..release + 12]);
    replace_bytes(&claim, &newer);
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    assert_eq!(std::fs::read(&claim).unwrap(), newer);
    assert_eq!(std::fs::read(&local).unwrap(), terminal);
    assert_eq!(std::fs::read(&mirror).unwrap(), terminal);
}

/// Test the authority at the release writer boundary, rather than only checking
/// the final bytes after a sequential retry.
#[test]
fn windows_native_copy_claim_release_keeps_terminal_journals_leased_during_undo_retry() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_GENERIC_WRITE, SYNCHRONIZE};

    let root = tempfile::tempdir().unwrap();
    let (name, _, _cleanup) = completed_undo_fixture(root.path(), false, false);
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let header = Snapshot::load(&parent, &central, &name, SnapshotAccess::Read)
        .unwrap()
        .state
        .header
        .unwrap();
    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
    remove_undo_fixture_source(root.path());
    let stage = root.path().join("destination").join(&name);
    let local = stage.join(LOCAL);
    let mirror = root.path().join("central").join(header.central_name());
    let journal = std::fs::read(&local).unwrap();
    let ledger = root
        .path()
        .join("destination")
        .join(header.claim.test_relative_path());
    let released = std::fs::read(&ledger).unwrap();
    let prefix = FRAME_HEADER + u32::from_le_bytes(released[8..12].try_into().unwrap()) as usize;
    replace_bytes(&ledger, &released[..prefix + 12]);

    let observations = Rc::new(RefCell::new(Vec::new()));
    let observed = observations.clone();
    claims::RELEASE_HANDOFF.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            for path in [&local, &mirror] {
                for access in [FILE_GENERIC_WRITE, DELETE | SYNCHRONIZE] {
                    let directory =
                        windows_fs::open_directory(path.parent().unwrap(), FILE_GENERIC_READ)
                            .unwrap();
                    let attempt = unsafe {
                        windows_fs::open_relative(
                            &directory,
                            path.file_name().unwrap(),
                            RelativeOptions {
                                directory: Some(false),
                                create_new: false,
                                access,
                                sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                                security: ptr::null(),
                            },
                        )
                    };
                    observed
                        .borrow_mut()
                        .push(attempt.err().and_then(|error| error.raw_os_error()));
                }
            }
        }))
    });
    let result = undo_completed_copy(&parent, &central, &name, |_| Ok(()));
    // Clear the one-shot hook even if the operation failed before the boundary.
    claims::RELEASE_HANDOFF.with(|hook| hook.borrow_mut().take());
    result.unwrap();
    assert_eq!(
        *observations.borrow(),
        vec![Some(32); 4],
        "both terminal journals must exclude write and delete handles until release is durable"
    );
    assert_eq!(std::fs::read(&ledger).unwrap(), released);
    assert_eq!(std::fs::read(stage.join(LOCAL)).unwrap(), journal);
    assert_eq!(
        std::fs::read(root.path().join("central").join(header.central_name())).unwrap(),
        journal
    );
    assert!(!root.path().join("destination/copy").exists());
}
