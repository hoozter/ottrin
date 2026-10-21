#![cfg_attr(not(test), allow(dead_code))]

// The Linux journal implementation below remains compiled for recovery-test coverage while
// user-facing Move is deliberately disabled. Remove this allowance only when the runtime gate
// is reopened with the documented platform and crash evidence.
#[cfg(target_os = "linux")]
use super::move_recovery;
#[cfg(target_os = "windows")]
use super::move_recovery;
#[cfg(target_os = "linux")]
use super::next_available_name;
use super::{ConflictAction, MoveDisposition, PlatformError, reject_directory_copy_into_itself};
#[cfg(target_os = "linux")]
use super::{copy_entry, rename_without_replacement};
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
#[cfg(target_os = "windows")]
use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::{
    fs::OpenOptions,
    io::{Read, Seek, Write},
    mem, ptr,
};
#[cfg(target_os = "windows")]
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_HANDLE_EOF, ERROR_INSUFFICIENT_BUFFER, ERROR_MORE_DATA, HANDLE, LocalFree,
    RtlNtStatusToDosError, STATUS_OBJECT_NAME_COLLISION,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GRANT_ACCESS, SE_FILE_OBJECT, SetEntriesInAclW, SetNamedSecurityInfoW,
    TRUSTEE_IS_SID, TRUSTEE_IS_USER,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetTokenInformation, PROTECTED_DACL_SECURITY_INFORMATION,
    SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ALL_ACCESS, FILE_BASIC_INFO, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    FILE_DISPOSITION_INFO_EX, FILE_STREAM_INFO, FileBasicInfo, FileDispositionInfoEx,
    FileStreamInfo, GetFileInformationByHandleEx, ReplaceFileW, SetFileInformationByHandle,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

#[cfg(target_os = "windows")]
pub(super) mod windows_undo;

pub(super) struct MovePublication {
    #[cfg(target_os = "windows")]
    pub windows_undo: Option<crate::PathMoveUndoReceipt>,
    pub disposition: MoveDisposition,
    pub published_path: Option<PathBuf>,
    pub retained_displaced_path: Option<PathBuf>,
    #[cfg(target_os = "linux")]
    pub published_content: Option<move_recovery::SourceTreeSnapshot>,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsMoveRoute {
    SameVolume,
    CrossVolume,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsSameVolumeSourceKind {
    File,
    Directory,
    Link,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsSameVolumeOverwritePublication {
    ReplaceFile,
    RetainedRename,
}

#[cfg(any(target_os = "windows", test))]
fn windows_same_volume_overwrite_publication(
    source: WindowsSameVolumeSourceKind,
    displaced: WindowsSameVolumeSourceKind,
) -> WindowsSameVolumeOverwritePublication {
    if source == WindowsSameVolumeSourceKind::File && displaced == WindowsSameVolumeSourceKind::File
    {
        WindowsSameVolumeOverwritePublication::ReplaceFile
    } else {
        WindowsSameVolumeOverwritePublication::RetainedRename
    }
}

#[cfg(target_os = "windows")]
fn windows_same_volume_kind_from_identity(
    identity: move_recovery::WindowsStableIdentity,
) -> Option<WindowsSameVolumeSourceKind> {
    windows_same_volume_source_kind(
        identity.is_regular_file_without_reparse(),
        identity.is_directory_without_reparse(),
        identity.is_symbolic_link(),
    )
}

#[cfg(any(target_os = "windows", test))]
fn windows_same_volume_source_kind(
    regular_file_without_reparse: bool,
    directory_without_reparse: bool,
    symbolic_link: bool,
) -> Option<WindowsSameVolumeSourceKind> {
    match (
        regular_file_without_reparse,
        directory_without_reparse,
        symbolic_link,
    ) {
        (true, false, false) => Some(WindowsSameVolumeSourceKind::File),
        (false, true, false) => Some(WindowsSameVolumeSourceKind::Directory),
        (false, false, true) => Some(WindowsSameVolumeSourceKind::Link),
        _ => None,
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(target_os = "windows", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(target_os = "windows", serde(rename_all = "kebab-case"))]
enum WindowsCrossVolumeSourceKind {
    File,
    Directory,
    SymbolicLink,
}

#[cfg(target_os = "windows")]
fn default_windows_cross_volume_source_kind() -> WindowsCrossVolumeSourceKind {
    WindowsCrossVolumeSourceKind::File
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct WindowsDirectoryManifestEntry {
    /// Relative Windows path encoded as its original UTF-16 code units.
    relative_path: Vec<u16>,
    kind: WindowsCrossVolumeSourceKind,
    size: u64,
    /// File-content SHA-256; empty only for directories.
    sha256: String,
    /// Distinguishes file and directory symbolic links without dereferencing either target.
    link_is_directory: bool,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowsDirectoryJournalEntry {
    relative_path: Vec<u16>,
    kind: WindowsCrossVolumeSourceKind,
    identity: move_recovery::WindowsStableIdentity,
    size: u64,
    sha256: String,
    #[serde(default)]
    link_target: Vec<u16>,
    #[serde(default)]
    link_is_directory: bool,
}

#[cfg(any(target_os = "windows", test))]
fn windows_manifest_relative_path_is_safe(path: &[u16]) -> bool {
    if path.is_empty() {
        return false;
    }
    path.split(|unit| *unit == u16::from(b'\\') || *unit == u16::from(b'/'))
        .all(|component| {
            !component.is_empty()
                && component != [b'.' as u16]
                && component != [b'.' as u16, b'.' as u16]
        })
}

/// Encodes a symbolic link's stored target without resolving or normalizing it.
#[cfg(any(target_os = "windows", test))]
fn windows_symbolic_link_evidence(target: &[u16]) -> (u64, String) {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    for unit in target {
        hasher.update(unit.to_le_bytes());
    }
    (
        (target.len() as u64) * 2,
        format!("{:x}", hasher.finalize()),
    )
}

#[cfg(any(target_os = "windows", test))]
fn windows_symbolic_link_snapshot_matches(
    expected_target: &[u16],
    expected_is_directory: bool,
    observed_target: &[u16],
    observed_is_directory: bool,
) -> bool {
    expected_target == observed_target && expected_is_directory == observed_is_directory
}

#[cfg(target_os = "windows")]
fn read_windows_symbolic_link(path: &Path) -> Result<(Vec<u16>, bool), PlatformError> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

    let target = std::fs::read_link(path).map_err(|error| PlatformError::Io(error.to_string()))?;
    let attributes = std::fs::symlink_metadata(path)
        .map_err(|error| PlatformError::Io(error.to_string()))?
        .file_attributes();
    Ok((
        target.as_os_str().encode_wide().collect(),
        attributes & FILE_ATTRIBUTE_DIRECTORY != 0,
    ))
}

#[cfg(target_os = "windows")]
fn create_windows_symbolic_link(
    path: &Path,
    target: &[u16],
    is_directory: bool,
) -> Result<(), PlatformError> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::fs::{symlink_dir, symlink_file};

    let target = OsString::from_wide(target);
    if is_directory {
        symlink_dir(target, path)
    } else {
        symlink_file(target, path)
    }
    .map_err(|error| PlatformError::Io(error.to_string()))
}

#[cfg(target_os = "windows")]
fn verify_windows_symbolic_link(
    path: &Path,
    expected_target: &[u16],
    expected_is_directory: bool,
) -> Result<(u64, String), PlatformError> {
    let (target, is_directory) = read_windows_symbolic_link(path)?;
    if !windows_symbolic_link_snapshot_matches(
        expected_target,
        expected_is_directory,
        &target,
        is_directory,
    ) {
        return Err(PlatformError::Io(format!(
            "Windows Move symbolic-link evidence changed: {}",
            path.display()
        )));
    }
    Ok(windows_symbolic_link_evidence(&target))
}

/// Produces the content evidence stored in the existing size/hash journal fields for a tree.
/// Paths and entry kinds are part of the digest so equal bytes at a different topology do not
/// verify. Sorting removes filesystem enumeration order from the durable evidence.
#[cfg(any(target_os = "windows", test))]
fn windows_directory_manifest_evidence(
    mut entries: Vec<WindowsDirectoryManifestEntry>,
) -> Result<(u64, String), PlatformError> {
    use sha2::{Digest, Sha256};

    entries.sort_unstable_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let entry_kinds = entries
        .iter()
        .map(|entry| (entry.relative_path.as_slice(), entry.kind))
        .collect::<std::collections::BTreeMap<_, _>>();
    if entry_kinds.len() != entries.len() {
        return Err(PlatformError::Io(
            "Windows Move directory manifest is inconsistent".into(),
        ));
    }

    let mut total_size = 0_u64;
    let mut hasher = Sha256::new();
    hasher.update((entries.len() as u64).to_le_bytes());
    for entry in &entries {
        let has_invalid_ancestor = entry
            .relative_path
            .iter()
            .enumerate()
            .filter(|(_, unit)| **unit == u16::from(b'\\') || **unit == u16::from(b'/'))
            .any(|(separator, _)| {
                entry_kinds.get(&entry.relative_path[..separator])
                    != Some(&WindowsCrossVolumeSourceKind::Directory)
            });
        if !windows_manifest_relative_path_is_safe(&entry.relative_path)
            || has_invalid_ancestor
            || (entry.kind == WindowsCrossVolumeSourceKind::Directory
                && (entry.size != 0 || !entry.sha256.is_empty()))
            || (matches!(
                entry.kind,
                WindowsCrossVolumeSourceKind::File | WindowsCrossVolumeSourceKind::SymbolicLink
            ) && entry.sha256.is_empty())
            || (entry.kind != WindowsCrossVolumeSourceKind::SymbolicLink && entry.link_is_directory)
        {
            return Err(PlatformError::Io(
                "Windows Move directory manifest is inconsistent".into(),
            ));
        }
        hasher.update((entry.relative_path.len() as u64).to_le_bytes());
        for unit in &entry.relative_path {
            hasher.update(unit.to_le_bytes());
        }
        hasher.update([match entry.kind {
            WindowsCrossVolumeSourceKind::File => 0,
            WindowsCrossVolumeSourceKind::Directory => 1,
            WindowsCrossVolumeSourceKind::SymbolicLink => 2,
        }]);
        hasher.update(entry.size.to_le_bytes());
        hasher.update((entry.sha256.len() as u64).to_le_bytes());
        hasher.update(entry.sha256.as_bytes());
        hasher.update([u8::from(entry.link_is_directory)]);
        total_size = total_size
            .checked_add(entry.size)
            .ok_or_else(|| PlatformError::Io("Windows Move directory size overflow".into()))?;
    }
    Ok((total_size, format!("{:x}", hasher.finalize())))
}

#[cfg(any(target_os = "windows", test))]
fn windows_cross_volume_source_kind(
    is_file: bool,
    is_directory: bool,
    is_symbolic_link: bool,
) -> Option<WindowsCrossVolumeSourceKind> {
    match (is_file, is_directory, is_symbolic_link) {
        (true, false, false) => Some(WindowsCrossVolumeSourceKind::File),
        (false, true, false) => Some(WindowsCrossVolumeSourceKind::Directory),
        (false, false, true) => Some(WindowsCrossVolumeSourceKind::SymbolicLink),
        _ => None,
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_move_route(source_volume: u64, destination_volume: u64) -> WindowsMoveRoute {
    if source_volume == destination_volume {
        WindowsMoveRoute::SameVolume
    } else {
        WindowsMoveRoute::CrossVolume
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_destination_parent_is_pinned<T: Eq>(opened: T, expected: T) -> bool {
    opened == expected
}

#[cfg(any(target_os = "windows", test))]
fn windows_stream_set_is_copy_safe<I, S>(streams: I, is_directory: bool) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut streams = streams.into_iter();
    // NTFS directories have no unnamed $DATA stream. Named streams on either
    // entry kind cannot be represented by our verified-copy manifest.
    if is_directory {
        return streams.next().is_none();
    }
    matches!(streams.next(), Some(name) if name.as_ref() == std::ffi::OsStr::new("::$DATA"))
        && streams.next().is_none()
}

#[cfg(any(target_os = "windows", test))]
fn publish_windows_cross_volume_candidate_with<R, F, N>(
    rename: R,
    flush_published: F,
    flush_namespace: N,
) -> Result<(), PlatformError>
where
    R: FnOnce() -> Result<(), PlatformError>,
    F: FnOnce() -> Result<(), PlatformError>,
    N: FnOnce() -> Result<(), PlatformError>,
{
    rename()?;
    flush_published()?;
    flush_namespace()
}

#[cfg(any(target_os = "windows", test))]
fn publish_windows_cross_volume_directory_overwrite_with<R, D, P, F, N>(
    retain_displaced: R,
    flush_retained_namespace: D,
    publish_candidate: P,
    flush_published: F,
    flush_published_namespace: N,
) -> Result<(), PlatformError>
where
    R: FnOnce() -> Result<(), PlatformError>,
    D: FnOnce() -> Result<(), PlatformError>,
    P: FnOnce() -> Result<(), PlatformError>,
    F: FnOnce() -> Result<(), PlatformError>,
    N: FnOnce() -> Result<(), PlatformError>,
{
    retain_displaced()?;
    flush_retained_namespace()?;
    publish_candidate()?;
    flush_published()?;
    flush_published_namespace()
}

#[cfg(any(target_os = "windows", test))]
fn flush_windows_cross_volume_recovered_publication_with<P, R, N, J>(
    flush_published: P,
    flush_retained: R,
    flush_namespace: N,
    record_publication: J,
) -> Result<(), PlatformError>
where
    P: FnOnce() -> Result<(), PlatformError>,
    R: FnOnce() -> Result<(), PlatformError>,
    N: FnOnce() -> Result<(), PlatformError>,
    J: FnOnce() -> Result<(), PlatformError>,
{
    flush_published()?;
    flush_retained()?;
    flush_namespace()?;
    record_publication()
}

#[cfg(any(target_os = "windows", test))]
fn complete_windows_cross_volume_source_removal_with<F, C>(
    flush_source_namespace: F,
    complete: C,
) -> Result<(), PlatformError>
where
    F: FnOnce() -> Result<(), PlatformError>,
    C: FnOnce() -> Result<(), PlatformError>,
{
    flush_source_namespace()?;
    complete()
}

#[cfg(target_os = "windows")]
fn require_windows_move_without_extended_attributes(
    file: &std::fs::File,
) -> Result<(), PlatformError> {
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_EA_INFORMATION, FileEaInformation, NtQueryInformationFile,
    };
    let mut ea = FILE_EA_INFORMATION::default();
    let mut io = IO_STATUS_BLOCK::default();
    let status = unsafe {
        NtQueryInformationFile(
            file.as_raw_handle(),
            &mut io,
            ptr::from_mut(&mut ea).cast(),
            mem::size_of::<FILE_EA_INFORMATION>() as u32,
            FileEaInformation,
        )
    };
    if status < 0 {
        return Err(PlatformError::Io(format!(
            "Windows Move extended-attribute query failed: {}",
            std::io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32),
        )));
    }
    if ea.EaSize != 0 {
        return Err(PlatformError::Io(
            "Windows cross-volume Move cannot preserve extended attributes; source retained".into(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn copy_windows_file_and_metadata(
    source: &mut std::fs::File,
    staged: &mut std::fs::File,
) -> Result<(), PlatformError> {
    std::io::copy(source, staged).map_err(|error| PlatformError::Io(error.to_string()))?;

    copy_windows_basic_metadata(source, staged)
}

#[cfg(target_os = "windows")]
fn copy_windows_basic_metadata(
    source: &std::fs::File,
    staged: &std::fs::File,
) -> Result<(), PlatformError> {
    require_windows_move_without_extended_attributes(source)?;
    let mut basic = FILE_BASIC_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            source.as_raw_handle() as HANDLE,
            FileBasicInfo,
            ptr::addr_of_mut!(basic).cast(),
            mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(PlatformError::Io(format!(
            "Windows Move source metadata read failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    if unsafe {
        SetFileInformationByHandle(
            staged.as_raw_handle() as HANDLE,
            FileBasicInfo,
            ptr::addr_of!(basic).cast(),
            mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(PlatformError::Io(format!(
            "Windows Move staged metadata write failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    staged
        .sync_all()
        .map_err(|error| PlatformError::Io(error.to_string()))
}

// Both Win32 queries below return linked records with counted UTF-16 names.
#[cfg(target_os = "windows")]
fn windows_file_streams(handle: &std::fs::File) -> Result<Vec<std::ffi::OsString>, PlatformError> {
    // u64 storage provides the required eight-byte record alignment. A full
    // result is required; never accept a truncated stream list as copy-safe.
    let mut buffer = vec![0_u64; 8192];
    loop {
        let succeeded = unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle() as HANDLE,
                FileStreamInfo,
                buffer.as_mut_ptr().cast(),
                u32::try_from(std::mem::size_of_val(buffer.as_slice()))
                    .map_err(|_| PlatformError::Io("Windows stream list is too large".into()))?,
            )
        };
        if succeeded != 0 {
            return crate::windows_fs::record_names(
                &buffer,
                mem::offset_of!(FILE_STREAM_INFO, StreamName),
                mem::offset_of!(FILE_STREAM_INFO, StreamNameLength),
            )
            .map_err(|error| PlatformError::Io(error.to_string()));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_HANDLE_EOF as i32) {
            return Ok(Vec::new());
        }
        if matches!(error.raw_os_error(), Some(code) if code == ERROR_MORE_DATA as i32 || code == ERROR_INSUFFICIENT_BUFFER as i32)
            && buffer.len() < 131072
        {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        return Err(PlatformError::Io(format!(
            "Windows Move could not enumerate owned entry streams: {error}"
        )));
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsCrossVolumePreflight {
    Skip,
    TransactionRequired,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsSourceRemovalDecision {
    RemoveOpenedSource,
    PreserveForRecovery,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsDirectoryCleanupTopology {
    ClaimSource,
    DrainClaimedSource,
    Complete,
    RetainAmbiguous,
}

#[cfg(any(target_os = "windows", test))]
fn classify_windows_directory_cleanup_topology(
    source_matches: bool,
    claimed_source_matches: bool,
) -> WindowsDirectoryCleanupTopology {
    match (source_matches, claimed_source_matches) {
        (true, false) => WindowsDirectoryCleanupTopology::ClaimSource,
        (false, true) => WindowsDirectoryCleanupTopology::DrainClaimedSource,
        (false, false) => WindowsDirectoryCleanupTopology::Complete,
        (true, true) => WindowsDirectoryCleanupTopology::RetainAmbiguous,
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_directory_cleanup_order(
    journal: &[WindowsDirectoryManifestEntry],
    observed: &[WindowsDirectoryManifestEntry],
) -> Option<Vec<usize>> {
    let mut journal_by_path = std::collections::BTreeMap::new();
    for entry in journal {
        if journal_by_path
            .insert(entry.relative_path.as_slice(), entry)
            .is_some()
        {
            return None;
        }
    }
    let mut observed_paths = std::collections::BTreeSet::new();
    for entry in observed {
        if !observed_paths.insert(entry.relative_path.as_slice())
            || journal_by_path.get(entry.relative_path.as_slice()) != Some(&entry)
        {
            return None;
        }
    }
    let mut order = (0..observed.len()).collect::<Vec<_>>();
    order.sort_unstable_by(|left, right| {
        let depth = |path: &[u16]| {
            path.iter()
                .filter(|unit| **unit == u16::from(b'\\') || **unit == u16::from(b'/'))
                .count()
        };
        depth(&observed[*right].relative_path)
            .cmp(&depth(&observed[*left].relative_path))
            .then_with(|| {
                observed[*right]
                    .relative_path
                    .cmp(&observed[*left].relative_path)
            })
    });
    Some(order)
}

#[cfg(any(target_os = "windows", test))]
fn classify_windows_cross_volume_preflight(
    conflict: ConflictAction,
    destination_exists: bool,
) -> WindowsCrossVolumePreflight {
    if conflict == ConflictAction::Skip && destination_exists {
        WindowsCrossVolumePreflight::Skip
    } else {
        WindowsCrossVolumePreflight::TransactionRequired
    }
}

fn reconcile_windows_rename_candidates<T: Clone + PartialEq>(
    conflict: ConflictAction,
    central: &[T],
    local: &[T],
) -> Option<Vec<T>> {
    if central == local {
        return Some(central.to_vec());
    }
    if conflict != ConflictAction::Rename {
        return None;
    }
    if central.starts_with(local) {
        Some(central.to_vec())
    } else if local.starts_with(central) {
        Some(local.to_vec())
    } else {
        None
    }
}

#[cfg(any(target_os = "windows", test))]
fn next_windows_rename_candidate(candidates: &[PathBuf]) -> Option<PathBuf> {
    let base = candidates.first()?;
    let parent = base.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = base.file_stem().unwrap_or(base.as_os_str());
    let extension = base.extension();
    for index in 1.. {
        let mut name = stem.to_os_string();
        name.push(format!(" ({index})"));
        if let Some(extension) = extension {
            name.push(".");
            name.push(extension);
        }
        let candidate = parent.join(name);
        if !candidates.contains(&candidate) && std::fs::symlink_metadata(&candidate).is_err() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(any(target_os = "windows", test))]
fn classify_windows_source_removal(
    opened_identity_matches: bool,
    named_identity_matches: bool,
    content_matches: bool,
) -> WindowsSourceRemovalDecision {
    if opened_identity_matches && named_identity_matches && content_matches {
        WindowsSourceRemovalDecision::RemoveOpenedSource
    } else {
        WindowsSourceRemovalDecision::PreserveForRecovery
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsCrossVolumeRestartAction {
    CopyAndVerify,
    DiscardIncompleteStaging,
    PublishStaged,
    FlushPublished,
    RemoveSource,
    Complete,
    RetainAmbiguous,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsIncompleteStagingEntryKind {
    File,
    Directory,
}

#[cfg(any(target_os = "windows", test))]
fn windows_incomplete_staging_entry_kind(
    regular_file_without_reparse: bool,
    directory_without_reparse: bool,
) -> Option<WindowsIncompleteStagingEntryKind> {
    match (regular_file_without_reparse, directory_without_reparse) {
        (true, false) => Some(WindowsIncompleteStagingEntryKind::File),
        (false, true) => Some(WindowsIncompleteStagingEntryKind::Directory),
        _ => None,
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_staging_candidate_matches_transaction(transaction: &Path, candidate: &Path) -> bool {
    candidate == transaction.join(WINDOWS_CROSS_VOLUME_CANDIDATE_NAME)
}

#[cfg(any(target_os = "windows", test))]
fn classify_windows_incomplete_directory_candidate(
    source_kind: WindowsCrossVolumeSourceKind,
    source_matches: bool,
    target_absent: bool,
    construction_pending: bool,
    candidate_identity_recorded: bool,
    candidate_identity_matches: bool,
    candidate_exists: bool,
) -> Option<WindowsCrossVolumeRestartAction> {
    (source_kind == WindowsCrossVolumeSourceKind::Directory
        && source_matches
        && target_absent
        && construction_pending
        && candidate_exists
        && (!candidate_identity_recorded || candidate_identity_matches))
        .then_some(WindowsCrossVolumeRestartAction::DiscardIncompleteStaging)
}

#[cfg(any(target_os = "windows", test))]
fn classify_windows_directory_slice_action(
    source_kind: WindowsCrossVolumeSourceKind,
    action: WindowsCrossVolumeRestartAction,
) -> WindowsCrossVolumeRestartAction {
    match (source_kind, action) {
        (
            WindowsCrossVolumeSourceKind::Directory,
            WindowsCrossVolumeRestartAction::CopyAndVerify
            | WindowsCrossVolumeRestartAction::DiscardIncompleteStaging
            | WindowsCrossVolumeRestartAction::PublishStaged
            | WindowsCrossVolumeRestartAction::FlushPublished
            | WindowsCrossVolumeRestartAction::RemoveSource
            | WindowsCrossVolumeRestartAction::Complete,
        ) => action,
        (WindowsCrossVolumeSourceKind::Directory, _) => {
            WindowsCrossVolumeRestartAction::RetainAmbiguous
        }
        (
            WindowsCrossVolumeSourceKind::File | WindowsCrossVolumeSourceKind::SymbolicLink,
            action,
        ) => action,
    }
}

#[cfg(any(target_os = "windows", test))]
fn require_windows_publication_durability(
    action: WindowsCrossVolumeRestartAction,
    publication_recorded: bool,
) -> WindowsCrossVolumeRestartAction {
    if action == WindowsCrossVolumeRestartAction::RemoveSource && !publication_recorded {
        WindowsCrossVolumeRestartAction::FlushPublished
    } else {
        action
    }
}

#[cfg(any(target_os = "windows", test))]
fn classify_windows_cross_volume_restart<T: Eq>(
    source: Option<T>,
    target: Option<T>,
    staged: Option<T>,
    expected_source: T,
    expected_staged: Option<T>,
) -> WindowsCrossVolumeRestartAction {
    if source.as_ref() == Some(&expected_source) && target.is_none() && staged.is_none() {
        WindowsCrossVolumeRestartAction::CopyAndVerify
    } else if source.as_ref() == Some(&expected_source)
        && target.is_none()
        && staged.as_ref() == expected_staged.as_ref()
        && expected_staged.is_some()
    {
        WindowsCrossVolumeRestartAction::PublishStaged
    } else if source.as_ref() == Some(&expected_source)
        && target.as_ref() == expected_staged.as_ref()
        && expected_staged.is_some()
        && staged.is_none()
    {
        WindowsCrossVolumeRestartAction::RemoveSource
    } else if source.is_none()
        && target.as_ref() == expected_staged.as_ref()
        && expected_staged.is_some()
        && staged.is_none()
    {
        WindowsCrossVolumeRestartAction::Complete
    } else {
        WindowsCrossVolumeRestartAction::RetainAmbiguous
    }
}

#[cfg(any(target_os = "windows", test))]
#[allow(
    clippy::too_many_arguments,
    reason = "the classifier compares four observed identities with three expected identities and the retained-publication policy; grouping them would only hide the recovery topology"
)]
fn classify_windows_cross_volume_overwrite_restart<T: Eq>(
    retained_publication_allowed: bool,
    source: Option<T>,
    target: Option<T>,
    staged: Option<T>,
    retained_displaced: Option<T>,
    expected_source: T,
    expected_staged: T,
    expected_displaced: T,
) -> WindowsCrossVolumeRestartAction {
    if source.as_ref() == Some(&expected_source)
        && target.as_ref() == Some(&expected_displaced)
        && staged.is_none()
        && retained_displaced.is_none()
    {
        WindowsCrossVolumeRestartAction::CopyAndVerify
    } else if source.as_ref() == Some(&expected_source)
        && target.as_ref() == Some(&expected_displaced)
        && staged.as_ref() == Some(&expected_staged)
        && retained_displaced.is_none()
    {
        WindowsCrossVolumeRestartAction::PublishStaged
    } else if retained_publication_allowed
        && source.as_ref() == Some(&expected_source)
        && target.is_none()
        && staged.as_ref() == Some(&expected_staged)
        && retained_displaced.as_ref() == Some(&expected_displaced)
    {
        // A directory/link retention gap, or a journaled file-publication
        // reclaim, can finish without replacing any newly occupied target.
        WindowsCrossVolumeRestartAction::PublishStaged
    } else if source.as_ref() == Some(&expected_source)
        && target.as_ref() == Some(&expected_staged)
        && staged.is_none()
        && retained_displaced.as_ref() == Some(&expected_displaced)
    {
        WindowsCrossVolumeRestartAction::RemoveSource
    } else if source.is_none()
        && target.as_ref() == Some(&expected_staged)
        && staged.is_none()
        && retained_displaced.as_ref() == Some(&expected_displaced)
    {
        WindowsCrossVolumeRestartAction::Complete
    } else {
        WindowsCrossVolumeRestartAction::RetainAmbiguous
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsReplaceTopology {
    PublishedAndRetained,
    PublishedAmbiguous,
    NotPublished,
    Ambiguous,
}

#[cfg(any(target_os = "windows", test))]
fn classify_windows_replace_topology<T: Eq>(
    target: Option<T>,
    replacement: Option<T>,
    retained_displaced: Option<T>,
    expected_source: T,
    expected_displaced: T,
) -> WindowsReplaceTopology {
    if target.as_ref() == Some(&expected_source) {
        if replacement.is_none() && retained_displaced.as_ref() == Some(&expected_displaced) {
            WindowsReplaceTopology::PublishedAndRetained
        } else {
            WindowsReplaceTopology::PublishedAmbiguous
        }
    } else if target.as_ref() == Some(&expected_displaced)
        && replacement.as_ref() == Some(&expected_source)
        && retained_displaced.is_none()
    {
        WindowsReplaceTopology::NotPublished
    } else {
        WindowsReplaceTopology::Ambiguous
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsOverwriteRestartAction {
    ClaimSource,
    PublishClaimed,
    PublishAfterRetain,
    FinishPublished,
    RetainAmbiguous,
}

#[cfg(any(target_os = "windows", test))]
fn classify_windows_overwrite_restart<T: Eq>(
    original: Option<T>,
    target: Option<T>,
    replacement: Option<T>,
    retained_displaced: Option<T>,
    expected_source: T,
    expected_displaced: T,
) -> WindowsOverwriteRestartAction {
    if original.as_ref() == Some(&expected_source)
        && target.as_ref() == Some(&expected_displaced)
        && replacement.is_none()
        && retained_displaced.is_none()
    {
        WindowsOverwriteRestartAction::ClaimSource
    } else if original.is_none()
        && target.as_ref() == Some(&expected_displaced)
        && replacement.as_ref() == Some(&expected_source)
        && retained_displaced.is_none()
    {
        WindowsOverwriteRestartAction::PublishClaimed
    } else if original.is_none()
        && target.as_ref() == Some(&expected_source)
        && replacement.is_none()
        && retained_displaced.as_ref() == Some(&expected_displaced)
    {
        WindowsOverwriteRestartAction::FinishPublished
    } else if original.is_none()
        && target.is_none()
        && replacement.as_ref() == Some(&expected_source)
        && retained_displaced.as_ref() == Some(&expected_displaced)
    {
        WindowsOverwriteRestartAction::PublishAfterRetain
    } else {
        WindowsOverwriteRestartAction::RetainAmbiguous
    }
}

#[cfg(target_os = "windows")]
const WINDOWS_REPLACEMENT_NAME: &str = "replacement";
#[cfg(target_os = "windows")]
const WINDOWS_RETAINED_NAME: &str = "retained-displaced";
#[cfg(target_os = "windows")]
const WINDOWS_LOCAL_JOURNAL_NAME: &str = "journal";
#[cfg(any(target_os = "windows", test))]
const WINDOWS_CROSS_VOLUME_CANDIDATE_NAME: &str = "candidate";
#[cfg(target_os = "windows")]
const WINDOWS_INCOMPLETE_STAGING_DISPOSITION_FLAGS: u32 = FILE_DISPOSITION_FLAG_DELETE
    | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
    | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE;
#[cfg(all(test, not(target_os = "windows")))]
const WINDOWS_INCOMPLETE_STAGING_DISPOSITION_FLAGS: u32 = 0x13;
#[cfg(any(target_os = "windows", test))]
const WINDOWS_RECOVERY_LOCK_SHARE_MODE: u32 = 0;

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WindowsOverwritePhase {
    Prepared,
    SourceClaimed,
    Published,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowsOverwriteRecord {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
    phase: WindowsOverwritePhase,
    source: move_recovery::NativePath,
    source_parent_identity: move_recovery::WindowsStableIdentity,
    source_identity: move_recovery::WindowsStableIdentity,
    destination_parent: move_recovery::NativePath,
    destination_parent_identity: move_recovery::WindowsStableIdentity,
    target: move_recovery::NativePath,
    destination_identity: move_recovery::WindowsStableIdentity,
    transaction_directory: move_recovery::NativePath,
    transaction_directory_identity: move_recovery::WindowsStableIdentity,
    replacement: move_recovery::NativePath,
    retained_displaced: move_recovery::NativePath,
    retained_identity: move_recovery::WindowsStableIdentity,
}

#[cfg(target_os = "windows")]
impl WindowsOverwriteRecord {
    fn same_transaction_evidence(&self, other: &Self) -> bool {
        let mut left = self.clone();
        let mut right = other.clone();
        left.phase = WindowsOverwritePhase::Prepared;
        right.phase = WindowsOverwritePhase::Prepared;
        left == right
    }

    fn transaction_id(&self) -> Result<[u8; 16], PlatformError> {
        let bytes = self.transaction_id.as_bytes();
        if bytes.len() != 32 {
            return Err(PlatformError::Io(
                "Windows Move journal has an invalid transaction ID".to_string(),
            ));
        }
        let mut id = [0_u8; 16];
        for (index, pair) in bytes.chunks_exact(2).enumerate() {
            let text =
                std::str::from_utf8(pair).map_err(|error| PlatformError::Io(error.to_string()))?;
            id[index] = u8::from_str_radix(text, 16)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
        }
        Ok(id)
    }

    fn source_path(&self) -> Result<PathBuf, PlatformError> {
        self.source
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }

    fn destination_parent_path(&self) -> Result<PathBuf, PlatformError> {
        self.destination_parent
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }

    fn target_path(&self) -> Result<PathBuf, PlatformError> {
        self.target
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }

    fn transaction_path(&self) -> Result<PathBuf, PlatformError> {
        self.transaction_directory
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }

    fn replacement_path(&self) -> Result<PathBuf, PlatformError> {
        self.replacement
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }

    fn retained_path(&self) -> Result<PathBuf, PlatformError> {
        self.retained_displaced
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WindowsCrossVolumePhase {
    Prepared,
    Staged,
    ReclaimingPublication,
    Published,
    SourceRemoved,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowsCrossVolumeRecord {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
    phase: WindowsCrossVolumePhase,
    source: move_recovery::NativePath,
    source_parent_identity: move_recovery::WindowsStableIdentity,
    source_identity: move_recovery::WindowsStableIdentity,
    #[serde(default = "default_windows_cross_volume_source_kind")]
    source_kind: WindowsCrossVolumeSourceKind,
    source_size: u64,
    source_sha256: String,
    #[serde(default)]
    source_link_target: Option<Vec<u16>>,
    #[serde(default)]
    source_link_is_directory: Option<bool>,
    #[serde(default)]
    source_manifest: Vec<WindowsDirectoryJournalEntry>,
    destination_parent: move_recovery::NativePath,
    destination_parent_identity: move_recovery::WindowsStableIdentity,
    #[serde(default)]
    destination_identity: Option<move_recovery::WindowsStableIdentity>,
    #[serde(default)]
    destination_manifest: Vec<WindowsDirectoryJournalEntry>,
    conflict: ConflictAction,
    target_candidates: Vec<move_recovery::NativePath>,
    transaction_directory: move_recovery::NativePath,
    transaction_directory_identity: move_recovery::WindowsStableIdentity,
    candidate: move_recovery::NativePath,
    candidate_identity: Option<move_recovery::WindowsStableIdentity>,
    #[serde(default)]
    retained_displaced: Option<move_recovery::NativePath>,
}

#[cfg(target_os = "windows")]
impl WindowsCrossVolumeRecord {
    fn transaction_id(&self) -> Result<[u8; 16], PlatformError> {
        let bytes = self.transaction_id.as_bytes();
        if bytes.len() != 32 {
            return Err(PlatformError::Io(
                "Windows Move journal has an invalid transaction ID".into(),
            ));
        }
        let mut id = [0_u8; 16];
        for (index, pair) in bytes.chunks_exact(2).enumerate() {
            let text =
                std::str::from_utf8(pair).map_err(|error| PlatformError::Io(error.to_string()))?;
            id[index] = u8::from_str_radix(text, 16)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
        }
        Ok(id)
    }

    fn source_path(&self) -> Result<PathBuf, PlatformError> {
        self.source
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }

    fn destination_parent_path(&self) -> Result<PathBuf, PlatformError> {
        self.destination_parent
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }

    fn target_path(&self) -> Result<PathBuf, PlatformError> {
        self.target_candidates
            .last()
            .ok_or_else(|| {
                PlatformError::Io("Windows cross-volume target journal is empty".into())
            })?
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }

    fn transaction_path(&self) -> Result<PathBuf, PlatformError> {
        self.transaction_directory
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }

    fn candidate_path(&self) -> Result<PathBuf, PlatformError> {
        self.candidate
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }

    fn retained_path(&self) -> Result<Option<PathBuf>, PlatformError> {
        self.retained_displaced
            .as_ref()
            .map(move_recovery::NativePath::decode)
            .transpose()
            .map_err(|error| PlatformError::Io(error.to_string()))
    }

    fn same_transaction_evidence(&self, other: &Self) -> bool {
        let candidate_identity_agrees = self.candidate_identity.is_none()
            || other.candidate_identity.is_none()
            || self.candidate_identity == other.candidate_identity;
        let target_candidates_agree = reconcile_windows_rename_candidates(
            self.conflict,
            &self.target_candidates,
            &other.target_candidates,
        )
        .is_some();
        let mut left = self.clone();
        let mut right = other.clone();
        left.phase = WindowsCrossVolumePhase::Prepared;
        right.phase = WindowsCrossVolumePhase::Prepared;
        left.candidate_identity = None;
        right.candidate_identity = None;
        left.target_candidates.clear();
        right.target_candidates.clear();
        candidate_identity_agrees && target_candidates_agree && left == right
    }
}

#[cfg(target_os = "linux")]
const CROSS_DEVICE_STAGED_NAME: &str = "candidate";
#[cfg(target_os = "linux")]
const SOURCE_CLEANUP_NAME: &str = "source-cleanup";

#[cfg(all(test, target_os = "linux"))]
static TEST_MOVE_SERIALIZER: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(all(test, target_os = "linux"))]
pub(crate) static TEST_MOVE_ENV_SERIALIZER: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runtime Move stays fail-closed except where the exact Linux-safe implementation is proven.
#[cfg(target_os = "linux")]
pub(super) fn move_entry_transactional(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
) -> Result<MovePublication, PlatformError> {
    let source_metadata = match std::fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A recoverable Move has already claimed the source under its private transaction
            // name. Let the journal/topology recovery path resolve that state before reporting
            // the original pathname as invalid.
            return move_with_hooks(source, destination_dir, conflict, |_| Ok(()));
        }
        Err(_) => return Err(PlatformError::InvalidSource(source.display().to_string())),
    };
    let destination_dir = std::fs::canonicalize(destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;
    let destination_metadata = std::fs::symlink_metadata(&destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;
    reject_directory_copy_into_itself(source, &destination_dir)?;

    if source_metadata.dev() != destination_metadata.dev() {
        return move_with_hooks(source, &destination_dir, conflict, |_| Ok(()));
    }

    let target = destination_dir.join(
        source
            .file_name()
            .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?,
    );
    match conflict {
        ConflictAction::Skip => match rename_without_replacement(source, &target) {
            Ok(()) => Ok(MovePublication {
                #[cfg(target_os = "windows")]
                windows_undo: None,
                disposition: MoveDisposition::Moved,
                published_path: Some(target),
                retained_displaced_path: None,
                published_content: None,
            }),
            Err(PlatformError::DestinationExists(_)) => Ok(MovePublication {
                #[cfg(target_os = "windows")]
                windows_undo: None,
                disposition: MoveDisposition::Skipped,
                published_path: None,
                retained_displaced_path: None,
                published_content: None,
            }),
            Err(error) => Err(error),
        },
        ConflictAction::Rename => {
            let mut candidate = target;
            loop {
                match rename_without_replacement(source, &candidate) {
                    Ok(()) => {
                        return Ok(MovePublication {
                            #[cfg(target_os = "windows")]
                            windows_undo: None,
                            disposition: MoveDisposition::Moved,
                            published_path: Some(candidate),
                            retained_displaced_path: None,
                            published_content: None,
                        });
                    }
                    Err(PlatformError::DestinationExists(_)) => {
                        candidate = next_available_name(&candidate);
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        ConflictAction::Overwrite => match std::fs::symlink_metadata(&target) {
            Ok(_) => move_with_hooks(source, &destination_dir, conflict, |_| Ok(())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                rename_without_replacement(source, &target)?;
                Ok(MovePublication {
                    #[cfg(target_os = "windows")]
                    windows_undo: None,
                    disposition: MoveDisposition::Moved,
                    published_path: Some(target),
                    retained_displaced_path: None,
                    published_content: None,
                })
            }
            Err(error) => Err(PlatformError::Io(error.to_string())),
        },
    }
}

#[cfg(target_os = "windows")]
pub(super) fn move_entry_transactional(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
) -> Result<MovePublication, PlatformError> {
    let destination_name = source
        .file_name()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    move_windows_entry_to_named_destination(source, destination_dir, destination_name, conflict)
}

/// Use the ordinary Move transaction with an exact destination basename. An
/// inverse must restore the original name even when Keep Both renamed the entry.
#[cfg(target_os = "windows")]
pub(super) fn move_windows_entry_to_named_destination(
    source: &Path,
    destination_dir: &Path,
    destination_name: &std::ffi::OsStr,
    conflict: ConflictAction,
) -> Result<MovePublication, PlatformError> {
    super::windows_fs::valid_component(destination_name)
        .map_err(|error| PlatformError::InvalidDestination(error.to_string()))?;
    // Every recovery scan and transaction mutation shares one process-lifetime-independent
    // ownership domain. Windows releases the exclusive share reservation if this process dies.
    let _recovery_lock = acquire_windows_recovery_lock()?;
    if let Some(publication) =
        recover_windows_overwrites_before_move(source, destination_dir, destination_name)?
    {
        return Ok(publication);
    }
    std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
    std::fs::symlink_metadata(destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;
    reject_directory_copy_into_itself(source, destination_dir)?;

    let source_handle = move_recovery::open_reparse_safe_entry_for_rename(source)
        .map_err(|error| PlatformError::Io(format!("source handle open failed: {error}")))?;
    let source_identity = move_recovery::stable_identity_from_handle(&source_handle)
        .map_err(|error| PlatformError::Io(format!("source identity probe failed: {error}")))?;
    let destination_handle = move_recovery::open_reparse_safe_directory_for_rename(destination_dir)
        .map_err(|error| PlatformError::Io(format!("destination handle open failed: {error}")))?;
    let destination_identity = move_recovery::stable_identity_from_handle(&destination_handle)
        .map_err(|error| {
            PlatformError::Io(format!("destination identity probe failed: {error}"))
        })?;

    let source_parent_identity = move_recovery::stable_identity_of_entry(
        source
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
    )
    .map_err(|error| PlatformError::Io(format!("source parent identity probe failed: {error}")))?;
    let route = windows_move_route(
        source_identity.volume_serial_number(),
        destination_identity.volume_serial_number(),
    );
    let mut publication = match route {
        WindowsMoveRoute::SameVolume => {
            windows_same_volume_source_kind(
                source_identity.is_regular_file_without_reparse(),
                source_identity.is_directory_without_reparse(),
                source_identity.is_symbolic_link(),
            )
            .ok_or_else(|| PlatformError::UnsupportedSafeMove(source.display().to_string()))?;
            move_windows_same_volume(
                source,
                destination_dir,
                destination_name,
                conflict,
                source_handle,
                destination_handle,
            )
        }
        WindowsMoveRoute::CrossVolume => move_windows_cross_volume_preflight(
            source,
            destination_dir,
            destination_name,
            conflict,
            source_handle,
            destination_handle,
        ),
    }?;
    if route == WindowsMoveRoute::SameVolume
        && publication.retained_displaced_path.is_none()
        && source_parent_identity.is_directory_without_reparse()
        && let Some(after_path) = publication.published_path.as_ref()
    {
        #[cfg(test)]
        windows_move_boundary(WindowsMoveBoundary::BeforeUndoReceipt, after_path);
        publication.windows_undo = Some(windows_undo::issue(
            source,
            after_path,
            source_identity,
            source_parent_identity,
            destination_identity,
        )?);
    }
    Ok(publication)
}

#[cfg(target_os = "windows")]
static WINDOWS_RECOVERY_PROCESS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(target_os = "windows")]
struct WindowsRecoveryLock {
    _process: std::sync::MutexGuard<'static, ()>,
    _system: std::fs::File,
}

#[cfg(target_os = "windows")]
fn acquire_windows_recovery_lock() -> Result<WindowsRecoveryLock, PlatformError> {
    // Serialize callers in this process before taking the cross-process share reservation.
    // Without this layer, ordinary parallel Move commands from the same Ottrin process report
    // a false external-recovery conflict instead of waiting for the current owner to finish.
    let process = WINDOWS_RECOVERY_PROCESS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let recovery_root = windows_recovery_root()?;
    let recovery_lock = windows_recovery_lock_path(&recovery_root);
    let lock_parent = recovery_lock
        .parent()
        .ok_or_else(|| PlatformError::Io("Windows Move recovery lock has no parent".to_string()))?;
    std::fs::create_dir_all(lock_parent).map_err(|error| {
        PlatformError::Io(format!(
            "Windows Move recovery lock parent creation failed: {error}"
        ))
    })?;
    let system = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .share_mode(WINDOWS_RECOVERY_LOCK_SHARE_MODE)
        .open(&recovery_lock)
        .map_err(|error| PlatformError::ClaimedRecoveryPending {
            transaction: recovery_lock.display().to_string(),
            message: format!("another Windows Move recovery owner may be active: {error}"),
        })?;
    Ok(WindowsRecoveryLock {
        _process: process,
        _system: system,
    })
}

#[cfg(any(target_os = "windows", test))]
fn windows_recovery_lock_path(recovery_root: &Path) -> PathBuf {
    recovery_root.with_extension("lock")
}

#[cfg(target_os = "windows")]
fn move_windows_cross_volume_preflight(
    source: &Path,
    destination_dir: &Path,
    destination_name: &std::ffi::OsStr,
    conflict: ConflictAction,
    source_handle: std::fs::File,
    destination_handle: std::fs::File,
) -> Result<MovePublication, PlatformError> {
    let destination_exists = match std::fs::symlink_metadata(destination_dir.join(destination_name))
    {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    };
    match classify_windows_cross_volume_preflight(conflict, destination_exists) {
        WindowsCrossVolumePreflight::Skip => Ok(MovePublication {
            #[cfg(target_os = "windows")]
            windows_undo: None,
            disposition: MoveDisposition::Skipped,
            published_path: None,
            retained_displaced_path: None,
        }),
        WindowsCrossVolumePreflight::TransactionRequired
            if !destination_exists || conflict == ConflictAction::Rename =>
        {
            let source_identity = move_recovery::stable_identity_from_handle(&source_handle)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            drop(source_handle);
            let source_handle = move_recovery::open_reparse_safe_cross_volume_source(source)
                .map_err(|error| {
                    PlatformError::Io(format!("cross-volume source lock failed: {error}"))
                })?;
            if move_recovery::stable_identity_from_handle(&source_handle)
                .map_err(|error| PlatformError::Io(error.to_string()))?
                != source_identity
            {
                return Err(PlatformError::Io(
                    "Windows cross-volume source changed while acquiring its Move lock".into(),
                ));
            }
            start_windows_cross_volume_move(
                source,
                destination_dir,
                &destination_dir.join(destination_name),
                // Replace applies only to an existing, identified destination.
                // A mixed batch also contains unoccupied names; publish those
                // without replacement and preserve any destination that races
                // into existence before publication.
                if conflict == ConflictAction::Overwrite && !destination_exists {
                    ConflictAction::Skip
                } else {
                    conflict
                },
                source_handle,
                destination_handle,
                None,
            )
        }
        WindowsCrossVolumePreflight::TransactionRequired
            if conflict == ConflictAction::Overwrite =>
        {
            let target = destination_dir.join(destination_name);
            let destination_identity = move_recovery::stable_identity_of_entry(&target)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            let source_identity = move_recovery::stable_identity_from_handle(&source_handle)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            let matching_supported_entries = (source_identity.is_regular_file_without_reparse()
                && destination_identity.is_regular_file_without_reparse())
                || (source_identity.is_directory_without_reparse()
                    && destination_identity.is_directory_without_reparse())
                || (source_identity.is_symbolic_link() && destination_identity.is_symbolic_link());
            if !matching_supported_entries {
                return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
                    source.display().to_string(),
                ));
            }
            drop(source_handle);
            let source_handle = move_recovery::open_reparse_safe_cross_volume_source(source)
                .map_err(|error| {
                    PlatformError::Io(format!("cross-volume source lock failed: {error}"))
                })?;
            if move_recovery::stable_identity_from_handle(&source_handle)
                .map_err(|error| PlatformError::Io(error.to_string()))?
                != source_identity
                || observe_windows_identity(&target)? != Some(destination_identity)
            {
                return Err(PlatformError::Io(
                    "Windows cross-volume Overwrite input changed while acquiring its Move lock"
                        .into(),
                ));
            }
            start_windows_cross_volume_move(
                source,
                destination_dir,
                &target,
                conflict,
                source_handle,
                destination_handle,
                Some(destination_identity),
            )
        }
        WindowsCrossVolumePreflight::TransactionRequired => Err(
            PlatformError::UnsupportedSafeCrossDeviceMove(source.display().to_string()),
        ),
    }
}

#[cfg(target_os = "windows")]
fn remove_windows_source_by_handle_if_still_named(
    source: &Path,
    source_handle: &std::fs::File,
    record: &WindowsCrossVolumeRecord,
    recovery_transaction: &Path,
) -> Result<(), PlatformError> {
    require_windows_move_without_extended_attributes(source_handle).map_err(|error| {
        PlatformError::CommittedCleanupPending {
            transaction: recovery_transaction.display().to_string(),
            message: error.to_string(),
        }
    })?;
    let opened_matches =
        move_recovery::stable_identity_from_handle(source_handle).map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: recovery_transaction.display().to_string(),
                message: format!("opened source identity revalidation failed: {error}"),
            }
        })? == record.source_identity;
    let named_matches = match move_recovery::stable_identity_of_entry(source) {
        Ok(identity) => identity == record.source_identity,
        Err(error) => {
            return Err(PlatformError::CommittedCleanupPending {
                transaction: recovery_transaction.display().to_string(),
                message: format!("named source identity revalidation failed: {error}"),
            });
        }
    };
    let content_matches = match record.source_kind {
        WindowsCrossVolumeSourceKind::File => {
            let mut content_handle = source_handle.try_clone().map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: recovery_transaction.display().to_string(),
                    message: format!("source content revalidation handle clone failed: {error}"),
                }
            })?;
            let streams = windows_file_streams(&content_handle).map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: recovery_transaction.display().to_string(),
                    message: format!("source stream revalidation failed: {error}"),
                }
            })?;
            windows_stream_set_is_copy_safe(streams, false)
                && hash_windows_file(&mut content_handle).map_err(|error| {
                    PlatformError::CommittedCleanupPending {
                        transaction: recovery_transaction.display().to_string(),
                        message: format!("source content revalidation failed: {error}"),
                    }
                })? == (record.source_size, record.source_sha256.clone())
        }
        WindowsCrossVolumeSourceKind::SymbolicLink => {
            let expected_target = record.source_link_target.as_deref().ok_or_else(|| {
                PlatformError::CommittedCleanupPending {
                    transaction: recovery_transaction.display().to_string(),
                    message: "symbolic-link cleanup evidence is missing its target".into(),
                }
            })?;
            let expected_is_directory = record.source_link_is_directory.ok_or_else(|| {
                PlatformError::CommittedCleanupPending {
                    transaction: recovery_transaction.display().to_string(),
                    message: "symbolic-link cleanup evidence is missing its kind".into(),
                }
            })?;
            let streams = windows_file_streams(source_handle).map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: recovery_transaction.display().to_string(),
                    message: format!("source symbolic-link stream revalidation failed: {error}"),
                }
            })?;
            windows_stream_set_is_copy_safe(streams, expected_is_directory)
                && verify_windows_symbolic_link(source, expected_target, expected_is_directory)
                    .map_err(|error| PlatformError::CommittedCleanupPending {
                        transaction: recovery_transaction.display().to_string(),
                        message: format!("source symbolic-link revalidation failed: {error}"),
                    })?
                    == (record.source_size, record.source_sha256.clone())
        }
        WindowsCrossVolumeSourceKind::Directory => false,
    };
    if classify_windows_source_removal(opened_matches, named_matches, content_matches)
        == WindowsSourceRemovalDecision::PreserveForRecovery
    {
        return Err(PlatformError::CommittedCleanupPending {
            transaction: recovery_transaction.display().to_string(),
            message: "source identity or content changed after destination publication; source was preserved"
                .into(),
        });
    }

    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    let disposition_size =
        u32::try_from(mem::size_of::<FILE_DISPOSITION_INFO_EX>()).map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: recovery_transaction.display().to_string(),
                message: error.to_string(),
            }
        })?;
    let removed = unsafe {
        SetFileInformationByHandle(
            source_handle.as_raw_handle() as HANDLE,
            FileDispositionInfoEx,
            ptr::from_ref(&disposition).cast_mut().cast(),
            disposition_size,
        )
    };
    if removed == 0 {
        return Err(PlatformError::CommittedCleanupPending {
            transaction: recovery_transaction.display().to_string(),
            message: format!(
                "handle-authoritative source removal failed: {}",
                std::io::Error::last_os_error()
            ),
        });
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_directory_source_claim_path(
    source_parent: &Path,
    record: &WindowsCrossVolumeRecord,
) -> Result<PathBuf, PlatformError> {
    record.transaction_id()?;
    Ok(source_parent.join(format!(".ottrin-move-{}-source", record.transaction_id)))
}

#[cfg(target_os = "windows")]
fn set_windows_move_cleanup_delete_disposition(
    handle: &std::fs::File,
    path: &Path,
    recovery_transaction: &Path,
) -> Result<(), PlatformError> {
    require_windows_move_without_extended_attributes(handle).map_err(|error| {
        PlatformError::CommittedCleanupPending {
            transaction: recovery_transaction.display().to_string(),
            message: error.to_string(),
        }
    })?;
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: WINDOWS_INCOMPLETE_STAGING_DISPOSITION_FLAGS,
    };
    let removed = unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle() as HANDLE,
            FileDispositionInfoEx,
            ptr::from_ref(&disposition).cast_mut().cast(),
            u32::try_from(mem::size_of::<FILE_DISPOSITION_INFO_EX>()).map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: recovery_transaction.display().to_string(),
                    message: error.to_string(),
                }
            })?,
        )
    };
    if removed == 0 {
        return Err(PlatformError::CommittedCleanupPending {
            transaction: recovery_transaction.display().to_string(),
            message: format!(
                "handle-authoritative Windows Move cleanup failed for {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            ),
        });
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn drain_windows_claimed_source_directory(
    claimed_source: &Path,
    claimed_source_handle: std::fs::File,
    record: &WindowsCrossVolumeRecord,
    recovery_transaction: &Path,
) -> Result<(), PlatformError> {
    let cleanup_error = |message: String| PlatformError::CommittedCleanupPending {
        transaction: recovery_transaction.display().to_string(),
        message,
    };
    if move_recovery::stable_identity_from_handle(&claimed_source_handle)
        .map_err(|error| cleanup_error(format!("claimed source identity probe failed: {error}")))?
        != record.source_identity
        || move_recovery::stable_identity_of_entry(claimed_source)
            .map_err(|error| cleanup_error(format!("named claimed source probe failed: {error}")))?
            != record.source_identity
        || !windows_stream_set_is_copy_safe(
            windows_file_streams(&claimed_source_handle)
                .map_err(|error| cleanup_error(error.to_string()))?,
            true,
        )
    {
        return Err(cleanup_error(
            "claimed source directory identity or streams changed; cleanup was preserved".into(),
        ));
    }

    let snapshot = capture_windows_directory_snapshot(claimed_source, &claimed_source_handle)
        .map_err(|error| cleanup_error(format!("claimed source scan failed: {error}")))?;
    let observed_journal = snapshot
        .journal_entries()
        .map_err(|error| cleanup_error(error.to_string()))?;
    let durable_by_path = record
        .source_manifest
        .iter()
        .map(|entry| (entry.relative_path.as_slice(), entry))
        .collect::<std::collections::BTreeMap<_, _>>();
    if durable_by_path.len() != record.source_manifest.len()
        || observed_journal
            .iter()
            .any(|entry| durable_by_path.get(entry.relative_path.as_slice()) != Some(&entry))
    {
        return Err(cleanup_error(
            "claimed source directory no longer matches the durable removal manifest".into(),
        ));
    }
    let durable_manifest = record
        .source_manifest
        .iter()
        .map(|entry| WindowsDirectoryManifestEntry {
            relative_path: entry.relative_path.clone(),
            kind: entry.kind,
            size: entry.size,
            sha256: entry.sha256.clone(),
            link_is_directory: entry.link_is_directory,
        })
        .collect::<Vec<_>>();
    let observed_manifest = snapshot
        .manifest_entries()
        .map_err(|error| cleanup_error(error.to_string()))?;
    let order = windows_directory_cleanup_order(&durable_manifest, &observed_manifest).ok_or_else(
        || cleanup_error("claimed source directory contains changed or unjournaled entries".into()),
    )?;
    let mut entries = snapshot.entries.into_iter().map(Some).collect::<Vec<_>>();
    for index in order {
        let entry = entries[index]
            .take()
            .expect("cleanup order contains each observed entry once");
        let path = claimed_source.join(&entry.relative_path);
        if move_recovery::stable_identity_from_handle(&entry.handle)
            .map_err(|error| cleanup_error(format!("opened cleanup entry probe failed: {error}")))?
            != entry.identity
            || move_recovery::stable_identity_of_entry(&path).map_err(|error| {
                cleanup_error(format!("named cleanup entry probe failed: {error}"))
            })? != entry.identity
        {
            return Err(cleanup_error(format!(
                "claimed source entry changed before removal: {}",
                path.display()
            )));
        }
        set_windows_move_cleanup_delete_disposition(&entry.handle, &path, recovery_transaction)?;
        drop(entry);
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(cleanup_error(error.to_string())),
            Ok(_) => {
                return Err(cleanup_error(format!(
                    "claimed source entry remained after handle removal: {}",
                    path.display()
                )));
            }
        }
    }

    if move_recovery::stable_identity_from_handle(&claimed_source_handle)
        .map_err(|error| cleanup_error(format!("claimed source root probe failed: {error}")))?
        != record.source_identity
        || move_recovery::stable_identity_of_entry(claimed_source)
            .map_err(|error| cleanup_error(format!("named source root probe failed: {error}")))?
            != record.source_identity
    {
        return Err(cleanup_error(
            "claimed source root changed before handle removal".into(),
        ));
    }
    set_windows_move_cleanup_delete_disposition(
        &claimed_source_handle,
        claimed_source,
        recovery_transaction,
    )?;
    drop(claimed_source_handle);
    match std::fs::symlink_metadata(claimed_source) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(cleanup_error(error.to_string())),
        Ok(_) => Err(cleanup_error(
            "claimed source root remained after handle removal".into(),
        )),
    }
}

#[cfg(target_os = "windows")]
fn move_windows_same_volume(
    source: &Path,
    destination_dir: &Path,
    destination_name: &std::ffi::OsStr,
    conflict: ConflictAction,
    source_handle: std::fs::File,
    destination_handle: std::fs::File,
) -> Result<MovePublication, PlatformError> {
    let mut candidate = destination_name.to_os_string();
    loop {
        let published_path = destination_dir.join(&candidate);
        match rename_windows_handle_no_replace(&source_handle, &destination_handle, &candidate) {
            Ok(()) => {
                return Ok(MovePublication {
                    #[cfg(target_os = "windows")]
                    windows_undo: None,
                    disposition: MoveDisposition::Moved,
                    published_path: Some(published_path),
                    retained_displaced_path: None,
                });
            }
            Err(PlatformError::DestinationExists(_)) => match conflict {
                ConflictAction::Skip => {
                    return Ok(MovePublication {
                        #[cfg(target_os = "windows")]
                        windows_undo: None,
                        disposition: MoveDisposition::Skipped,
                        published_path: None,
                        retained_displaced_path: None,
                    });
                }
                ConflictAction::Rename => {
                    candidate = super::next_available_name(&published_path)
                        .file_name()
                        .expect("generated candidate has a file name")
                        .to_os_string();
                }
                ConflictAction::Overwrite => {
                    return start_windows_same_volume_overwrite(
                        source,
                        destination_dir,
                        &published_path,
                        source_handle,
                    );
                }
            },
            Err(error) => return Err(error),
        }
    }
}

#[cfg(target_os = "windows")]
pub(super) fn rename_windows_handle_no_replace(
    source: &std::fs::File,
    destination_directory: &std::fs::File,
    destination_name: &std::ffi::OsStr,
) -> Result<(), PlatformError> {
    let name = super::windows_fs::valid_component(destination_name)
        .map_err(|error| PlatformError::InvalidDestination(error.to_string()))?;
    let name_bytes = name
        .len()
        .checked_mul(mem::size_of::<u16>())
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| {
            PlatformError::InvalidDestination(destination_name.to_string_lossy().into())
        })?;
    let buffer_size = mem::size_of::<FILE_RENAME_INFORMATION>()
        .checked_add(name_bytes as usize)
        .ok_or_else(|| {
            PlatformError::InvalidDestination(destination_name.to_string_lossy().into())
        })?;
    // FILE_RENAME_INFORMATION contains pointer-sized fields; retain matching alignment while
    // appending its variable-length UTF-16 name rather than casting an alignment-1 byte allocation.
    let mut buffer = vec![0_u64; buffer_size.div_ceil(mem::size_of::<u64>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    unsafe {
        ptr::addr_of_mut!((*info).Anonymous.ReplaceIfExists).write(false);
        ptr::addr_of_mut!((*info).RootDirectory)
            .write(destination_directory.as_raw_handle() as HANDLE);
        ptr::addr_of_mut!((*info).FileNameLength).write(name_bytes);
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            ptr::addr_of_mut!((*info).FileName).cast(),
            name.len(),
        );
    }
    let buffer_size = u32::try_from(buffer_size)
        .map_err(|error| PlatformError::InvalidDestination(error.to_string()))?;
    let mut io_status = IO_STATUS_BLOCK::default();
    let status = unsafe {
        NtSetInformationFile(
            source.as_raw_handle() as HANDLE,
            &mut io_status,
            buffer.as_ptr().cast(),
            buffer_size,
            FileRenameInformation,
        )
    };
    if status >= 0 {
        return Ok(());
    }
    if status == STATUS_OBJECT_NAME_COLLISION {
        Err(PlatformError::DestinationExists(
            destination_name.to_string_lossy().into_owned(),
        ))
    } else {
        let error =
            std::io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32);
        Err(PlatformError::Io(error.to_string()))
    }
}

#[cfg(target_os = "windows")]
fn windows_wide_path(path: &Path) -> Result<Vec<u16>, PlatformError> {
    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(PlatformError::InvalidDestination(
            path.display().to_string(),
        ));
    }
    wide.push(0);
    Ok(wide)
}

#[cfg(target_os = "windows")]
fn observe_windows_identity(
    path: &Path,
) -> Result<Option<move_recovery::WindowsStableIdentity>, PlatformError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => move_recovery::stable_identity_of_entry(path)
            .map(Some)
            .map_err(|error| PlatformError::Io(error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PlatformError::Io(error.to_string())),
    }
}

#[cfg(target_os = "windows")]
fn discard_incomplete_windows_directory_candidate(
    path: &Path,
    expected_identity: move_recovery::WindowsStableIdentity,
) -> Result<(), PlatformError> {
    let handle = move_recovery::open_reparse_safe_cross_volume_cleanup_entry(path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let opened_identity = move_recovery::stable_identity_from_handle(&handle)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    if opened_identity != expected_identity
        || move_recovery::stable_identity_of_entry(path)
            .map_err(|error| PlatformError::Io(error.to_string()))?
            != expected_identity
    {
        return Err(PlatformError::Io(format!(
            "incomplete Windows staging entry changed while it was opened: {}",
            path.display()
        )));
    }

    let entry_kind = windows_incomplete_staging_entry_kind(
        opened_identity.is_regular_file_without_reparse(),
        opened_identity.is_directory_without_reparse(),
    )
    .ok_or_else(|| {
        PlatformError::Io(format!(
            "incomplete Windows staging entry has an unsupported type or reparse tag: {}",
            path.display()
        ))
    })?;
    if !windows_stream_set_is_copy_safe(
        windows_file_streams(&handle)?,
        entry_kind == WindowsIncompleteStagingEntryKind::Directory,
    ) {
        return Err(PlatformError::Io(format!(
            "incomplete Windows staging entry has unsupported named streams: {}",
            path.display()
        )));
    }
    if entry_kind == WindowsIncompleteStagingEntryKind::Directory {
        for name in crate::windows_fs::directory_children(&handle)
            .map_err(|error| PlatformError::Io(error.to_string()))?
        {
            let child = path.join(name);
            let child_identity = move_recovery::stable_identity_of_entry(&child)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            discard_incomplete_windows_directory_candidate(&child, child_identity)?;
        }
    }

    if move_recovery::stable_identity_from_handle(&handle)
        .map_err(|error| PlatformError::Io(error.to_string()))?
        != opened_identity
        || move_recovery::stable_identity_of_entry(path)
            .map_err(|error| PlatformError::Io(error.to_string()))?
            != opened_identity
    {
        return Err(PlatformError::Io(format!(
            "incomplete Windows staging entry changed before handle removal: {}",
            path.display()
        )));
    }
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: WINDOWS_INCOMPLETE_STAGING_DISPOSITION_FLAGS,
    };
    let removed = unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle() as HANDLE,
            FileDispositionInfoEx,
            ptr::from_ref(&disposition).cast_mut().cast(),
            u32::try_from(mem::size_of::<FILE_DISPOSITION_INFO_EX>())
                .map_err(|error| PlatformError::Io(error.to_string()))?,
        )
    };
    if removed == 0 {
        return Err(PlatformError::Io(format!(
            "handle-authoritative incomplete Windows staging removal failed for {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    drop(handle);
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PlatformError::Io(error.to_string())),
        Ok(_) => Err(PlatformError::Io(format!(
            "incomplete Windows staging entry remained after handle removal: {}",
            path.display()
        ))),
    }
}

#[cfg(target_os = "windows")]
fn windows_recovery_root() -> Result<PathBuf, PlatformError> {
    #[cfg(test)]
    if let Some(root) = TEST_RECOVERY_ROOT.with(|root| root.borrow().clone()) {
        return Ok(root);
    }
    #[cfg(test)]
    if let Some(root) = std::env::var_os("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT") {
        return Ok(PathBuf::from(root));
    }
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| PlatformError::Io("LOCALAPPDATA is unavailable".to_string()))?;
    Ok(PathBuf::from(local_app_data)
        .join("Ottrin")
        .join("State")
        .join("moves"))
}

#[cfg(target_os = "windows")]
fn encode_transaction_id(id: [u8; 16]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum WindowsJournalKind {
    SameVolumeOverwrite,
    CrossVolume,
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_journal_name(name: &std::ffi::OsStr) -> Option<([u8; 16], WindowsJournalKind)> {
    let name = name.to_str()?;
    let (encoded, kind) = if let Some(encoded) = name.strip_suffix(".cross-journal") {
        (encoded, WindowsJournalKind::CrossVolume)
    } else if let Some(encoded) = name.strip_suffix(".journal") {
        (encoded, WindowsJournalKind::SameVolumeOverwrite)
    } else {
        return None;
    };
    if encoded.len() != 32 {
        return None;
    }
    let mut transaction_id = [0_u8; 16];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(pair).ok()?;
        transaction_id[index] = u8::from_str_radix(pair, 16).ok()?;
    }
    Some((transaction_id, kind))
}

/// Lists retained Windows Move journals without creating or mutating recovery state.
#[cfg(any(target_os = "windows", test))]
pub(super) fn discover_retained_windows_move_transactions_at(
    recovery_root: &Path,
) -> Result<Vec<move_recovery::WindowsDiscoveredMoveTransaction>, PlatformError> {
    let entries = match std::fs::read_dir(recovery_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    };
    let mut discovered = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| PlatformError::Io(error.to_string()))?;
        let Some((transaction_id, kind)) = parse_windows_journal_name(&entry.file_name()) else {
            continue;
        };
        if !entry
            .file_type()
            .map_err(|error| PlatformError::Io(error.to_string()))?
            .is_file()
        {
            return Err(PlatformError::Io(format!(
                "Windows Move journal is not a regular file: {}",
                entry.path().display()
            )));
        }
        #[cfg(target_os = "windows")]
        {
            let record_transaction_id = match kind {
                WindowsJournalKind::SameVolumeOverwrite => {
                    load_windows_overwrite_record(&entry.path())?.transaction_id()?
                }
                WindowsJournalKind::CrossVolume => {
                    load_windows_cross_volume_record(&entry.path())?.transaction_id()?
                }
            };
            if record_transaction_id != transaction_id {
                return Err(PlatformError::Io(format!(
                    "Windows Move journal filename transaction ID {} does not match its record",
                    encode_transaction_id(transaction_id)
                )));
            }
        }
        let kind = match kind {
            WindowsJournalKind::SameVolumeOverwrite => {
                move_recovery::WindowsMoveTransactionKind::SameVolumeOverwrite
            }
            WindowsJournalKind::CrossVolume => {
                move_recovery::WindowsMoveTransactionKind::CrossVolume
            }
        };
        discovered.push(move_recovery::WindowsDiscoveredMoveTransaction {
            transaction_id,
            kind,
        });
    }
    discovered.sort_by_key(|transaction| (transaction.transaction_id, transaction.kind));
    if discovered
        .windows(2)
        .any(|pair| pair[0].transaction_id == pair[1].transaction_id)
    {
        return Err(PlatformError::Io(
            "multiple durable Windows Move journals share a transaction ID".to_string(),
        ));
    }
    Ok(discovered)
}

#[cfg(target_os = "windows")]
pub(super) fn discover_retained_windows_move_transactions()
-> Result<Vec<move_recovery::WindowsDiscoveredMoveTransaction>, PlatformError> {
    discover_retained_windows_move_transactions_at(&windows_recovery_root()?)
}

#[cfg(target_os = "windows")]
fn windows_journal_path(root: &Path, transaction_id: [u8; 16]) -> PathBuf {
    root.join(format!("{}.journal", encode_transaction_id(transaction_id)))
}

#[cfg(target_os = "windows")]
fn restrict_windows_transaction_directory(path: &Path) -> Result<(), PlatformError> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(PlatformError::Io(format!(
            "current-user token open failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    struct TokenHandle(HANDLE);
    impl Drop for TokenHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let token = TokenHandle(token);
    let mut required = 0_u32;
    unsafe {
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut required);
    }
    if std::io::Error::last_os_error()
        .raw_os_error()
        .map(|code| code as u32)
        != Some(ERROR_INSUFFICIENT_BUFFER)
        || required < mem::size_of::<TOKEN_USER>() as u32
    {
        return Err(PlatformError::Io(
            "current-user token size query failed".to_string(),
        ));
    }
    let mut token_user = vec![0_u64; (required as usize).div_ceil(mem::size_of::<u64>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            token_user.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(PlatformError::Io(format!(
            "current-user token query failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    let user = unsafe { &*token_user.as_ptr().cast::<TOKEN_USER>() };
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_ALL_ACCESS,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: windows_sys::Win32::Security::Authorization::TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: user.User.Sid.cast(),
            ..Default::default()
        },
    };
    let mut acl = ptr::null_mut();
    let status = unsafe { SetEntriesInAclW(1, &access, ptr::null(), &mut acl) };
    if status != 0 {
        return Err(PlatformError::Io(format!(
            "private transaction ACL creation failed with status {status}"
        )));
    }
    struct LocalAcl(*mut windows_sys::Win32::Security::ACL);
    impl Drop for LocalAcl {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0.cast());
            }
        }
    }
    let acl = LocalAcl(acl);
    let mut wide = windows_wide_path(path)?;
    let status = unsafe {
        SetNamedSecurityInfoW(
            wide.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            acl.0,
            ptr::null(),
        )
    };
    if status != 0 {
        return Err(PlatformError::Io(format!(
            "private transaction ACL application failed with status {status}"
        )));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn write_windows_record_file<T: serde::Serialize>(
    path: &Path,
    record: &T,
    create_only: bool,
) -> Result<(), PlatformError> {
    let bytes = serde_json::to_vec(record).map_err(|error| PlatformError::Io(error.to_string()))?;
    let parent = path
        .parent()
        .ok_or_else(|| PlatformError::Io("journal has no parent".to_string()))?;
    if create_only || !path.exists() {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| PlatformError::Io(error.to_string()))?;
    } else {
        let mut temp_id = [0_u8; 16];
        getrandom::fill(&mut temp_id).map_err(|error| PlatformError::Io(error.to_string()))?;
        let temporary = parent.join(format!("journal.next-{}", encode_transaction_id(temp_id)));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        drop(file);
        let path_wide = windows_wide_path(path)?;
        let temporary_wide = windows_wide_path(&temporary)?;
        if unsafe {
            ReplaceFileW(
                path_wide.as_ptr(),
                temporary_wide.as_ptr(),
                ptr::null(),
                0,
                ptr::null(),
                ptr::null(),
            )
        } == 0
        {
            return Err(PlatformError::Io(format!(
                "durable journal replacement failed: {}",
                std::io::Error::last_os_error()
            )));
        }
    }
    #[cfg(test)]
    windows_move_boundary(WindowsMoveBoundary::JournalPublished, path);
    // ReplaceFileW's REPLACEFILE_WRITE_THROUGH flag is unsupported. Both new
    // journal names and replacement names require an explicit namespace flush.
    let directory =
        move_recovery::open_reparse_safe_directory_for_rename(parent).map_err(|error| {
            PlatformError::Io(format!("journal namespace flush open failed: {error}"))
        })?;
    directory
        .sync_all()
        .map_err(|error| PlatformError::Io(format!("journal namespace flush failed: {error}")))
}

#[cfg(target_os = "windows")]
fn persist_windows_overwrite_record(
    recovery_root: &Path,
    record: &WindowsOverwriteRecord,
) -> Result<(), PlatformError> {
    let transaction_id = record.transaction_id()?;
    let local = record.transaction_path()?.join(WINDOWS_LOCAL_JOURNAL_NAME);
    write_windows_record_file(&local, record, false)?;
    write_windows_record_file(
        &windows_journal_path(recovery_root, transaction_id),
        record,
        false,
    )
}

#[cfg(target_os = "windows")]
fn load_windows_overwrite_record(path: &Path) -> Result<WindowsOverwriteRecord, PlatformError> {
    let bytes = std::fs::read(path).map_err(|error| PlatformError::Io(error.to_string()))?;
    let record: WindowsOverwriteRecord =
        serde_json::from_slice(&bytes).map_err(|error| PlatformError::Io(error.to_string()))?;
    if record.schema_major != move_recovery::JOURNAL_SCHEMA_MAJOR
        || record.schema_minor > move_recovery::JOURNAL_SCHEMA_MINOR
    {
        return Err(PlatformError::Io(format!(
            "unsupported Windows Move journal schema {}.{}",
            record.schema_major, record.schema_minor
        )));
    }
    Ok(record)
}

#[cfg(target_os = "windows")]
fn load_windows_cross_volume_record(
    path: &Path,
) -> Result<WindowsCrossVolumeRecord, PlatformError> {
    let bytes = std::fs::read(path).map_err(|error| PlatformError::Io(error.to_string()))?;
    let record: WindowsCrossVolumeRecord =
        serde_json::from_slice(&bytes).map_err(|error| PlatformError::Io(error.to_string()))?;
    if record.schema_major != move_recovery::JOURNAL_SCHEMA_MAJOR
        || record.schema_minor > move_recovery::JOURNAL_SCHEMA_MINOR
    {
        return Err(PlatformError::Io(format!(
            "unsupported Windows cross-volume Move journal schema {}.{}",
            record.schema_major, record.schema_minor
        )));
    }
    Ok(record)
}

#[cfg(target_os = "windows")]
fn recover_windows_overwrites_before_move(
    source: &Path,
    destination_dir: &Path,
    destination_name: &std::ffi::OsStr,
) -> Result<Option<MovePublication>, PlatformError> {
    let recovery_root = windows_recovery_root()?;
    let entries = match std::fs::read_dir(&recovery_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    };
    let mut matching = None;
    for entry in entries {
        let entry = entry.map_err(|error| PlatformError::Io(error.to_string()))?;
        let path = entry.path();
        let (is_matching, publication) = if path.extension()
            == Some(std::ffi::OsStr::new("journal"))
        {
            let record = load_windows_overwrite_record(&path)?;
            // A paired, completed publication is retained history, not a
            // reservation of its former source/target names. Those names
            // may now describe unrelated entries. Do not replay this old
            // transaction against them or return its result for a new Move.
            // Unpaired/pending/private-claim evidence still takes the
            // existing fail-closed recovery path below.
            if record.phase == WindowsOverwritePhase::Published {
                let transaction = record.transaction_path()?;
                let local =
                    load_windows_overwrite_record(&transaction.join(WINDOWS_LOCAL_JOURNAL_NAME))?;
                if local == record
                    && record.retained_identity == record.destination_identity
                    && observe_windows_identity(&transaction)?
                        == Some(record.transaction_directory_identity)
                    && observe_windows_identity(&record.replacement_path()?)?.is_none()
                    && observe_windows_identity(&record.retained_path()?)?
                        == Some(record.retained_identity)
                {
                    continue;
                }
            }
            let same_endpoints = record.source_path()? == source
                && record.destination_parent_path()? == destination_dir;
            let is_matching =
                same_endpoints && record.target_path()? == destination_dir.join(destination_name);
            if same_endpoints && !is_matching {
                return Err(PlatformError::ClaimedRecoveryPending {
                    transaction: path.display().to_string(),
                    message: "Pending Windows Move has a different destination name".into(),
                });
            }
            (
                is_matching,
                resume_windows_overwrite(&recovery_root, record)?,
            )
        } else if path.extension() == Some(std::ffi::OsStr::new("cross-journal")) {
            let mut record = load_windows_cross_volume_record(&path)?;
            if completed_windows_cross_volume_overwrite(&recovery_root, &mut record)?.is_some() {
                continue;
            }
            let same_endpoints = record.source_path()? == source
                && record.destination_parent_path()? == destination_dir;
            let requested_target = record
                .target_candidates
                .first()
                .ok_or_else(|| PlatformError::Io("Windows Move target journal is empty".into()))?
                .decode()
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            let is_matching =
                same_endpoints && requested_target == destination_dir.join(destination_name);
            if same_endpoints && !is_matching {
                return Err(PlatformError::ClaimedRecoveryPending {
                    transaction: path.display().to_string(),
                    message: "Pending Windows Move has a different destination name".into(),
                });
            }
            let publication = resume_windows_cross_volume(&recovery_root, &mut record, None)?;
            (is_matching, publication)
        } else {
            continue;
        };
        if is_matching {
            if matching.is_some() {
                return Err(PlatformError::ClaimedRecoveryPending {
                    transaction: recovery_root.display().to_string(),
                    message: "multiple durable Windows Overwrite transactions match this Move"
                        .to_string(),
                });
            }
            matching = Some(publication);
        }
    }
    Ok(matching)
}

#[cfg(target_os = "windows")]
fn windows_cross_volume_journal_path(root: &Path, transaction_id: [u8; 16]) -> PathBuf {
    root.join(format!(
        "{}.cross-journal",
        encode_transaction_id(transaction_id)
    ))
}

#[cfg(target_os = "windows")]
pub(super) fn recover_retained_windows_move_transaction(
    transaction_id: [u8; 16],
) -> Result<(), PlatformError> {
    let _recovery_lock = acquire_windows_recovery_lock()?;
    let recovery_root = windows_recovery_root()?;
    let same_volume_journal = windows_journal_path(&recovery_root, transaction_id);
    let cross_volume_journal = windows_cross_volume_journal_path(&recovery_root, transaction_id);
    let same_volume_exists = same_volume_journal
        .try_exists()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let cross_volume_exists = cross_volume_journal
        .try_exists()
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    if same_volume_exists && cross_volume_exists {
        return Err(PlatformError::ClaimedRecoveryPending {
            transaction: encode_transaction_id(transaction_id),
            message: "multiple durable Windows Move journals share this transaction ID".to_string(),
        });
    }
    if cross_volume_exists {
        let mut record = load_windows_cross_volume_record(&cross_volume_journal)?;
        if record.transaction_id()? != transaction_id {
            return Err(PlatformError::Io(
                "Windows Move journal filename does not match its transaction ID".to_string(),
            ));
        }
        return resume_windows_cross_volume(&recovery_root, &mut record, None).map(|_| ());
    }
    if !same_volume_exists {
        return Err(PlatformError::Io(format!(
            "retained Windows Move transaction was not found: {}",
            encode_transaction_id(transaction_id)
        )));
    }

    let record = load_windows_overwrite_record(&same_volume_journal)?;
    if record.transaction_id()? != transaction_id {
        return Err(PlatformError::Io(
            "Windows Move journal filename does not match its transaction ID".to_string(),
        ));
    }
    resume_windows_overwrite(&recovery_root, record).map(|_| ())
}

#[cfg(target_os = "windows")]
fn persist_windows_cross_volume_record(
    recovery_root: &Path,
    record: &WindowsCrossVolumeRecord,
) -> Result<(), PlatformError> {
    let transaction_id = record.transaction_id()?;
    write_windows_record_file(
        &windows_cross_volume_journal_path(recovery_root, transaction_id),
        record,
        false,
    )?;
    write_windows_record_file(
        &record.transaction_path()?.join(WINDOWS_LOCAL_JOURNAL_NAME),
        record,
        false,
    )
}

#[cfg(target_os = "windows")]
fn finish_windows_cross_volume_teardown(
    recovery_root: &Path,
    record: &WindowsCrossVolumeRecord,
) -> Result<MovePublication, PlatformError> {
    use windows_sys::Win32::Foundation::GENERIC_READ;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };
    let transaction = record.transaction_path()?;
    let destination = record.destination_parent_path()?;
    let id = record.transaction_id()?;
    let cleanup_error = |message: String| PlatformError::CommittedCleanupPending {
        transaction: transaction.display().to_string(),
        message,
    };
    if record.phase != WindowsCrossVolumePhase::SourceRemoved
        || record.conflict == ConflictAction::Overwrite
        || record.candidate_identity.is_none()
        || record.retained_displaced.is_some()
        || transaction != destination.join(format!(".ottrin-move-{}", encode_transaction_id(id)))
        || !windows_staging_candidate_matches_transaction(&transaction, &record.candidate_path()?)
        || record.target_path()?.parent() != Some(destination.as_path())
    {
        return Err(cleanup_error(
            "Windows completed Move evidence is inconsistent".into(),
        ));
    }
    // Keep the verified parent at its recorded name through all teardown flushes.
    let parent = OpenOptions::new()
        .access_mode(move_recovery::WINDOWS_DESTINATION_PARENT_ACCESS_MODE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(&destination)
        .map_err(|error| cleanup_error(error.to_string()))?;
    if move_recovery::stable_identity_from_handle(&parent)
        .map_err(|error| cleanup_error(error.to_string()))?
        != record.destination_parent_identity
    {
        return Err(cleanup_error(
            "Windows completed Move destination parent changed".into(),
        ));
    }
    match observe_windows_identity(&transaction)? {
        Some(identity) if identity == record.transaction_directory_identity => {
            let directory = move_recovery::open_reparse_safe_cross_volume_candidate(&transaction)
                .map_err(|error| cleanup_error(error.to_string()))?;
            if move_recovery::stable_identity_from_handle(&directory)
                .map_err(|error| cleanup_error(error.to_string()))?
                != record.transaction_directory_identity
            {
                return Err(cleanup_error(
                    "Windows completed Move transaction changed while opening".into(),
                ));
            }
            let local = transaction.join(WINDOWS_LOCAL_JOURNAL_NAME);
            match OpenOptions::new()
                .access_mode(GENERIC_READ | DELETE)
                .share_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&local)
            {
                Ok(file) => {
                    if !move_recovery::stable_identity_from_handle(&file)
                        .map_err(|error| cleanup_error(error.to_string()))?
                        .is_regular_file_without_reparse()
                    {
                        return Err(cleanup_error(
                            "Windows completed Move local journal is not a regular file".into(),
                        ));
                    }
                    let local_record: WindowsCrossVolumeRecord = serde_json::from_reader(&file)
                        .map_err(|error| {
                            cleanup_error(format!(
                                "Windows completed Move local journal is invalid: {error}"
                            ))
                        })?;
                    if !record.same_transaction_evidence(&local_record)
                        || local_record.phase != WindowsCrossVolumePhase::Published
                        || local_record.candidate_identity != record.candidate_identity
                        || local_record.target_candidates != record.target_candidates
                    {
                        return Err(cleanup_error(
                            "Windows completed Move journals disagree".into(),
                        ));
                    }
                    #[cfg(test)]
                    windows_move_boundary(WindowsMoveBoundary::JournalCleanupValidated, &local);
                    set_windows_move_cleanup_delete_disposition(&file, &local, &transaction)?;
                    drop(file);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(cleanup_error(error.to_string())),
            }
            directory
                .sync_all()
                .map_err(|error| cleanup_error(error.to_string()))?;
            #[cfg(test)]
            windows_move_boundary(WindowsMoveBoundary::LocalJournalRemoved, &transaction);
            // Delete only the verified empty directory, never unknown contents.
            set_windows_move_cleanup_delete_disposition(&directory, &transaction, &transaction)?;
            drop(directory);
            parent
                .sync_all()
                .map_err(|error| cleanup_error(error.to_string()))?;
            #[cfg(test)]
            windows_move_boundary(
                WindowsMoveBoundary::TransactionDirectoryRemoved,
                &transaction,
            );
        }
        None => {
            // The durable SourceRemoved record survives an interrupted final teardown.
            parent
                .sync_all()
                .map_err(|error| cleanup_error(error.to_string()))?;
        }
        Some(_) => {
            return Err(cleanup_error(
                "Windows completed Move transaction identity changed".into(),
            ));
        }
    }
    let central = windows_cross_volume_journal_path(recovery_root, id);
    let central_file = OpenOptions::new()
        .access_mode(GENERIC_READ | DELETE)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&central)
        .map_err(|error| cleanup_error(error.to_string()))?;
    if !move_recovery::stable_identity_from_handle(&central_file)
        .map_err(|error| cleanup_error(error.to_string()))?
        .is_regular_file_without_reparse()
    {
        return Err(cleanup_error(
            "Windows completed Move central journal is not a regular file".into(),
        ));
    }
    let current: WindowsCrossVolumeRecord =
        serde_json::from_reader(&central_file).map_err(|error| cleanup_error(error.to_string()))?;
    if &current != record {
        return Err(cleanup_error(
            "Windows completed Move central journal changed".into(),
        ));
    }
    #[cfg(test)]
    windows_move_boundary(WindowsMoveBoundary::JournalCleanupValidated, &central);
    set_windows_move_cleanup_delete_disposition(&central_file, &central, &transaction)?;
    drop(central_file);
    move_recovery::open_reparse_safe_directory_for_rename(recovery_root)
        .map_err(|error| cleanup_error(error.to_string()))?
        .sync_all()
        .map_err(|error| cleanup_error(error.to_string()))?;
    Ok(MovePublication {
        #[cfg(target_os = "windows")]
        windows_undo: None,
        disposition: MoveDisposition::Moved,
        published_path: Some(record.target_path()?),
        retained_displaced_path: None,
    })
}

#[cfg(target_os = "windows")]
fn advance_windows_cross_volume_rename_target(
    recovery_root: &Path,
    record: &mut WindowsCrossVolumeRecord,
) -> Result<PathBuf, PlatformError> {
    let candidates = record
        .target_candidates
        .iter()
        .map(move_recovery::NativePath::decode)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let next = next_windows_rename_candidate(&candidates)
        .ok_or_else(|| PlatformError::Io("Windows cross-volume target journal is empty".into()))?;
    record
        .target_candidates
        .push(move_recovery::NativePath::encode(&next));
    persist_windows_cross_volume_record(recovery_root, record).map_err(|error| {
        PlatformError::ClaimedRecoveryPending {
            transaction: record
                .transaction_path()
                .map_or_else(|_| "unknown".into(), |path| path.display().to_string()),
            message: format!(
                "Windows Rename candidate was not durable before publication: {error}"
            ),
        }
    })?;
    Ok(next)
}

#[cfg(target_os = "windows")]
fn hash_windows_file(file: &mut std::fs::File) -> Result<(u64, String), PlatformError> {
    use sha2::{Digest, Sha256};

    file.seek(std::io::SeekFrom::Start(0))
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size = size
            .checked_add(read as u64)
            .ok_or_else(|| PlatformError::Io("Windows Move source size overflow".into()))?;
    }
    Ok((size, format!("{:x}", hasher.finalize())))
}

#[cfg(target_os = "windows")]
struct WindowsDirectorySnapshotEntry {
    relative_path: PathBuf,
    path: PathBuf,
    kind: WindowsCrossVolumeSourceKind,
    identity: move_recovery::WindowsStableIdentity,
    size: u64,
    sha256: String,
    link_target: Vec<u16>,
    link_is_directory: bool,
    handle: std::fs::File,
}

#[cfg(target_os = "windows")]
struct WindowsDirectorySnapshot {
    entries: Vec<WindowsDirectorySnapshotEntry>,
    evidence: (u64, String),
}

#[cfg(target_os = "windows")]
impl WindowsDirectorySnapshot {
    fn manifest_entries(&self) -> Result<Vec<WindowsDirectoryManifestEntry>, PlatformError> {
        self.entries
            .iter()
            .map(|entry| {
                Ok(WindowsDirectoryManifestEntry {
                    relative_path: windows_manifest_path(&entry.relative_path)?,
                    kind: entry.kind,
                    size: entry.size,
                    sha256: entry.sha256.clone(),
                    link_is_directory: entry.link_is_directory,
                })
            })
            .collect()
    }

    fn journal_entries(&self) -> Result<Vec<WindowsDirectoryJournalEntry>, PlatformError> {
        let mut entries = self
            .entries
            .iter()
            .map(|entry| {
                Ok(WindowsDirectoryJournalEntry {
                    relative_path: windows_manifest_path(&entry.relative_path)?,
                    kind: entry.kind,
                    identity: entry.identity,
                    size: entry.size,
                    sha256: entry.sha256.clone(),
                    link_target: entry.link_target.clone(),
                    link_is_directory: entry.link_is_directory,
                })
            })
            .collect::<Result<Vec<_>, PlatformError>>()?;
        entries.sort_unstable_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(entries)
    }
}

#[cfg(target_os = "windows")]
fn windows_manifest_path(path: &Path) -> Result<Vec<u16>, PlatformError> {
    let mut encoded = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(PlatformError::Io(
                "Windows Move directory manifest contains a non-relative path".into(),
            ));
        };
        if !encoded.is_empty() {
            encoded.push(u16::from(b'\\'));
        }
        encoded.extend(component.encode_wide());
    }
    if !windows_manifest_relative_path_is_safe(&encoded) {
        return Err(PlatformError::Io(
            "Windows Move directory manifest contains an unsafe path".into(),
        ));
    }
    Ok(encoded)
}

#[cfg(target_os = "windows")]
fn capture_windows_directory_snapshot(
    root: &Path,
    root_handle: &std::fs::File,
) -> Result<WindowsDirectorySnapshot, PlatformError> {
    require_windows_move_without_extended_attributes(root_handle)?;
    fn visit(
        root: &Path,
        relative_directory: &Path,
        directory_handle: &std::fs::File,
        entries: &mut Vec<WindowsDirectorySnapshotEntry>,
    ) -> Result<(), PlatformError> {
        for child in crate::windows_fs::directory_children(directory_handle)
            .map_err(|error| PlatformError::Io(error.to_string()))?
        {
            let relative_path = relative_directory.join(child);
            let path = root.join(&relative_path);
            let mut handle = move_recovery::open_reparse_safe_cross_volume_source(&path)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            require_windows_move_without_extended_attributes(&handle)?;
            let identity = move_recovery::stable_identity_from_handle(&handle)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            let kind = windows_cross_volume_source_kind(
                identity.is_regular_file_without_reparse(),
                identity.is_directory_without_reparse(),
                identity.is_symbolic_link(),
            )
            .ok_or_else(|| {
                PlatformError::UnsupportedSafeCrossDeviceMove(path.display().to_string())
            })?;
            if entries.iter().any(|entry| entry.identity == identity) {
                return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
                    path.display().to_string(),
                ));
            }
            let (size, sha256, link_target, link_is_directory) = match kind {
                WindowsCrossVolumeSourceKind::File => {
                    let (size, sha256) = hash_windows_file(&mut handle)?;
                    (size, sha256, Vec::new(), false)
                }
                WindowsCrossVolumeSourceKind::Directory => (0, String::new(), Vec::new(), false),
                WindowsCrossVolumeSourceKind::SymbolicLink => {
                    let (target, is_directory) = read_windows_symbolic_link(&path)?;
                    let (size, sha256) = windows_symbolic_link_evidence(&target);
                    (size, sha256, target, is_directory)
                }
            };
            if !windows_stream_set_is_copy_safe(
                windows_file_streams(&handle)?,
                kind == WindowsCrossVolumeSourceKind::Directory || link_is_directory,
            ) {
                return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
                    path.display().to_string(),
                ));
            }
            let directory_handle = if kind == WindowsCrossVolumeSourceKind::Directory {
                Some(
                    handle
                        .try_clone()
                        .map_err(|error| PlatformError::Io(error.to_string()))?,
                )
            } else {
                None
            };
            entries.push(WindowsDirectorySnapshotEntry {
                relative_path: relative_path.clone(),
                path,
                kind,
                identity,
                size,
                sha256,
                link_target,
                link_is_directory,
                handle,
            });
            if let Some(directory_handle) = directory_handle {
                visit(root, &relative_path, &directory_handle, entries)?;
            }
        }
        Ok(())
    }

    if !windows_stream_set_is_copy_safe(windows_file_streams(root_handle)?, true) {
        return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
            root.display().to_string(),
        ));
    }
    let mut entries = Vec::new();
    visit(root, Path::new(""), root_handle, &mut entries)?;
    let manifest = entries
        .iter()
        .map(|entry| {
            Ok(WindowsDirectoryManifestEntry {
                relative_path: windows_manifest_path(&entry.relative_path)?,
                kind: entry.kind,
                size: entry.size,
                sha256: entry.sha256.clone(),
                link_is_directory: entry.link_is_directory,
            })
        })
        .collect::<Result<Vec<_>, PlatformError>>()?;
    let evidence = windows_directory_manifest_evidence(manifest)?;
    Ok(WindowsDirectorySnapshot { entries, evidence })
}

#[cfg(target_os = "windows")]
fn current_windows_directory_snapshot_evidence(
    snapshot: &mut WindowsDirectorySnapshot,
) -> Result<(u64, String), PlatformError> {
    let manifest = snapshot
        .entries
        .iter_mut()
        .map(|entry| {
            if move_recovery::stable_identity_from_handle(&entry.handle)
                .map_err(|error| PlatformError::Io(error.to_string()))?
                != entry.identity
            {
                return Err(PlatformError::Io(
                    "Windows Move directory entry identity changed".into(),
                ));
            }
            let (size, sha256) = match entry.kind {
                WindowsCrossVolumeSourceKind::File => hash_windows_file(&mut entry.handle)?,
                WindowsCrossVolumeSourceKind::Directory => (0, String::new()),
                WindowsCrossVolumeSourceKind::SymbolicLink => {
                    let (target, is_directory) = read_windows_symbolic_link(&entry.path)?;
                    if target != entry.link_target || is_directory != entry.link_is_directory {
                        return Err(PlatformError::Io(
                            "Windows Move symbolic-link target changed".into(),
                        ));
                    }
                    windows_symbolic_link_evidence(&target)
                }
            };
            Ok(WindowsDirectoryManifestEntry {
                relative_path: windows_manifest_path(&entry.relative_path)?,
                kind: entry.kind,
                size,
                sha256,
                link_is_directory: entry.link_is_directory,
            })
        })
        .collect::<Result<Vec<_>, PlatformError>>()?;
    windows_directory_manifest_evidence(manifest)
}

#[cfg(target_os = "windows")]
fn stage_windows_directory_snapshot(
    source_root_handle: &std::fs::File,
    source_snapshot: &mut WindowsDirectorySnapshot,
    candidate: &Path,
) -> Result<(std::fs::File, WindowsDirectorySnapshot), PlatformError> {
    // The caller creates and identity-pins this private root before tree construction.

    for entry in source_snapshot
        .entries
        .iter()
        .filter(|entry| entry.kind == WindowsCrossVolumeSourceKind::Directory)
    {
        std::fs::create_dir(candidate.join(&entry.relative_path))
            .map_err(|error| PlatformError::Io(error.to_string()))?;
    }
    for entry in source_snapshot
        .entries
        .iter()
        .filter(|entry| entry.kind == WindowsCrossVolumeSourceKind::SymbolicLink)
    {
        create_windows_symbolic_link(
            &candidate.join(&entry.relative_path),
            &entry.link_target,
            entry.link_is_directory,
        )?;
        let (target, is_directory) =
            read_windows_symbolic_link(&candidate.join(&entry.relative_path))?;
        if target != entry.link_target || is_directory != entry.link_is_directory {
            return Err(PlatformError::Io(
                "Windows Move staged symbolic-link verification failed".into(),
            ));
        }
    }
    for entry in source_snapshot
        .entries
        .iter_mut()
        .filter(|entry| entry.kind == WindowsCrossVolumeSourceKind::File)
    {
        entry
            .handle
            .seek(std::io::SeekFrom::Start(0))
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        let mut staged = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(candidate.join(&entry.relative_path))
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        copy_windows_file_and_metadata(&mut entry.handle, &mut staged)?;
        if hash_windows_file(&mut staged)? != (entry.size, entry.sha256.clone()) {
            return Err(PlatformError::Io(
                "Windows Move staged directory file verification failed".into(),
            ));
        }
    }
    for entry in source_snapshot
        .entries
        .iter()
        .rev()
        .filter(|entry| entry.kind == WindowsCrossVolumeSourceKind::Directory)
    {
        let staged = move_recovery::open_reparse_safe_cross_volume_candidate(
            &candidate.join(&entry.relative_path),
        )
        .map_err(|error| PlatformError::Io(error.to_string()))?;
        copy_windows_basic_metadata(&entry.handle, &staged)?;
    }
    let staged_root = move_recovery::open_reparse_safe_cross_volume_candidate(candidate)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    copy_windows_basic_metadata(source_root_handle, &staged_root)?;
    drop(staged_root);

    let staged_root = move_recovery::open_reparse_safe_cross_volume_source(candidate)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut staged_snapshot = capture_windows_directory_snapshot(candidate, &staged_root)?;
    let staged_evidence = current_windows_directory_snapshot_evidence(&mut staged_snapshot)?;
    if staged_evidence != staged_snapshot.evidence
        || current_windows_directory_snapshot_evidence(source_snapshot)? != source_snapshot.evidence
    {
        return Err(PlatformError::Io(
            "Windows Move directory changed during staged copy verification".into(),
        ));
    }
    Ok((staged_root, staged_snapshot))
}

#[cfg(target_os = "windows")]
fn start_windows_cross_volume_move(
    source: &Path,
    destination_dir: &Path,
    target: &Path,
    conflict: ConflictAction,
    mut source_handle: std::fs::File,
    destination_handle: std::fs::File,
    destination_identity: Option<move_recovery::WindowsStableIdentity>,
) -> Result<MovePublication, PlatformError> {
    require_windows_move_without_extended_attributes(&source_handle)?;
    let source_identity = move_recovery::stable_identity_from_handle(&source_handle)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let destination_parent_identity =
        move_recovery::stable_identity_from_handle(&destination_handle)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_kind = windows_cross_volume_source_kind(
        source_identity.is_regular_file_without_reparse(),
        source_identity.is_directory_without_reparse(),
        source_identity.is_symbolic_link(),
    )
    .ok_or_else(|| PlatformError::UnsupportedSafeCrossDeviceMove(source.display().to_string()))?;
    if !destination_parent_identity.is_directory_without_reparse() {
        return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
            source.display().to_string(),
        ));
    }
    let source_parent = source
        .parent()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    let source_parent_identity = move_recovery::stable_identity_of_entry(source_parent)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    if !source_parent_identity.is_directory_without_reparse() {
        return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
            source.display().to_string(),
        ));
    }
    let (source_size, source_sha256, source_manifest, source_link_target, source_link_is_directory) =
        match source_kind {
            WindowsCrossVolumeSourceKind::File => {
                if !windows_stream_set_is_copy_safe(windows_file_streams(&source_handle)?, false) {
                    return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
                        source.display().to_string(),
                    ));
                }
                let (size, sha256) = hash_windows_file(&mut source_handle)?;
                (size, sha256, Vec::new(), None, None)
            }
            WindowsCrossVolumeSourceKind::Directory => {
                let snapshot = capture_windows_directory_snapshot(source, &source_handle)?;
                let (size, sha256) = snapshot.evidence.clone();
                (size, sha256, snapshot.journal_entries()?, None, None)
            }
            WindowsCrossVolumeSourceKind::SymbolicLink => {
                let (target, is_directory) = read_windows_symbolic_link(source)?;
                if !windows_stream_set_is_copy_safe(
                    windows_file_streams(&source_handle)?,
                    is_directory,
                ) {
                    return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
                        source.display().to_string(),
                    ));
                }
                let (size, sha256) = windows_symbolic_link_evidence(&target);
                (size, sha256, Vec::new(), Some(target), Some(is_directory))
            }
        };
    let destination_manifest = if source_kind == WindowsCrossVolumeSourceKind::Directory
        && conflict == ConflictAction::Overwrite
    {
        let target_handle = move_recovery::open_reparse_safe_cross_volume_source(target)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        capture_windows_directory_snapshot(target, &target_handle)?.journal_entries()?
    } else {
        Vec::new()
    };
    if move_recovery::stable_identity_of_entry(source)
        .map_err(|error| PlatformError::Io(error.to_string()))?
        != source_identity
    {
        return Err(PlatformError::Io(
            "Windows Move source identity changed before durable intent".into(),
        ));
    }

    let recovery_root = windows_recovery_root()?;
    std::fs::create_dir_all(&recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let (transaction_id, transaction_path) = loop {
        let mut transaction_id = [0_u8; 16];
        getrandom::fill(&mut transaction_id)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        let candidate = destination_dir.join(format!(
            ".ottrin-move-{}",
            encode_transaction_id(transaction_id)
        ));
        match std::fs::create_dir(&candidate) {
            Ok(()) => break (transaction_id, candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(PlatformError::Io(error.to_string())),
        }
    };
    if let Err(error) = restrict_windows_transaction_directory(&transaction_path) {
        let _ = std::fs::remove_dir(&transaction_path);
        return Err(error);
    }
    let transaction_identity = move_recovery::stable_identity_of_entry(&transaction_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut record = WindowsCrossVolumeRecord {
        schema_major: move_recovery::JOURNAL_SCHEMA_MAJOR,
        schema_minor: move_recovery::JOURNAL_SCHEMA_MINOR,
        transaction_id: encode_transaction_id(transaction_id),
        phase: WindowsCrossVolumePhase::Prepared,
        source: move_recovery::NativePath::encode(source),
        source_parent_identity,
        source_identity,
        source_kind,
        source_size,
        source_sha256,
        source_link_target,
        source_link_is_directory,
        source_manifest,
        destination_parent: move_recovery::NativePath::encode(destination_dir),
        destination_parent_identity,
        destination_identity,
        destination_manifest,
        conflict,
        target_candidates: vec![move_recovery::NativePath::encode(target)],
        transaction_directory: move_recovery::NativePath::encode(&transaction_path),
        transaction_directory_identity: transaction_identity,
        candidate: move_recovery::NativePath::encode(
            &transaction_path.join(WINDOWS_CROSS_VOLUME_CANDIDATE_NAME),
        ),
        candidate_identity: None,
        retained_displaced: (conflict == ConflictAction::Overwrite).then(|| {
            move_recovery::NativePath::encode(&transaction_path.join(WINDOWS_RETAINED_NAME))
        }),
    };
    persist_windows_cross_volume_record(&recovery_root, &record).map_err(|error| {
        PlatformError::Io(format!(
            "durable Windows cross-volume Move intent failed before staging: {error}"
        ))
    })?;
    resume_windows_cross_volume(&recovery_root, &mut record, Some(source_handle))
}

#[cfg(all(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsMoveBoundary {
    BeforeUndoReceipt,
    UndoParentsOpened,
    BeforeUndoRename,
    UndoRenamed,
    JournalPublished,
    FileReplaced,
    FilePublicationFlushed,
    PublicationReclaimed,
    CandidateWritable,
    CandidateUnlocked,
    DisplacedRetained,
    CandidatePublished,
    BeforeSourceCleanup,
    SourceUnlocked,
    SourceClaimed,
    JournalCleanupValidated,
    TeardownPrepared,
    LocalJournalRemoved,
    TransactionDirectoryRemoved,
}

#[cfg(all(target_os = "windows", test))]
type WindowsMoveHook = Box<dyn FnMut(WindowsMoveBoundary, &Path)>;

#[cfg(all(target_os = "windows", test))]
thread_local! {
    static WINDOWS_MOVE_HOOK: std::cell::RefCell<Option<WindowsMoveHook>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(all(target_os = "windows", test))]
fn windows_move_boundary(boundary: WindowsMoveBoundary, path: &Path) {
    if std::env::var("OTTRIN_TEST_WINDOWS_MOVE_EXIT_BOUNDARY")
        .ok()
        .as_deref()
        == Some(format!("{boundary:?}").as_str())
    {
        std::process::exit(87);
    }
    WINDOWS_MOVE_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(boundary, path);
        }
    });
}

/// Normalize only the verified private candidate, never a source or published
/// entry. Both restart reopening and ReplaceFileW need a writable candidate;
/// source metadata is reapplied through the resulting writable handle.
#[cfg(target_os = "windows")]
fn prepare_windows_private_candidate_for_reopen(
    candidate: &Path,
    staged: &mut std::fs::File,
    record: &WindowsCrossVolumeRecord,
) -> Result<(), PlatformError> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY};
    if candidate != record.candidate_path()? {
        return Err(PlatformError::Io(
            "metadata preparation requires the private candidate".into(),
        ));
    }
    if Some(
        move_recovery::stable_identity_from_handle(staged)
            .map_err(|error| PlatformError::Io(error.to_string()))?,
    ) != record.candidate_identity
        || hash_windows_file(staged)? != (record.source_size, record.source_sha256.clone())
    {
        return Err(PlatformError::Io(
            "private candidate changed before metadata recovery".into(),
        ));
    }
    let attributes = staged
        .metadata()
        .map_err(|error| PlatformError::Io(error.to_string()))?
        .file_attributes();
    if attributes & FILE_ATTRIBUTE_READONLY != 0 {
        let writable_attributes = attributes & !FILE_ATTRIBUTE_READONLY;
        let basic = FILE_BASIC_INFO {
            FileAttributes: if writable_attributes == 0 {
                FILE_ATTRIBUTE_NORMAL
            } else {
                writable_attributes
            },
            ..Default::default()
        };
        if unsafe {
            SetFileInformationByHandle(
                staged.as_raw_handle(),
                FileBasicInfo,
                std::ptr::addr_of!(basic).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return Err(PlatformError::Io(format!(
                "private candidate attribute recovery failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        #[cfg(test)]
        windows_move_boundary(WindowsMoveBoundary::CandidateWritable, candidate);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn reopen_windows_staged_file(
    candidate: &Path,
    record: &WindowsCrossVolumeRecord,
) -> Result<std::fs::File, PlatformError> {
    use windows_sys::Win32::{
        Foundation::GENERIC_READ,
        Storage::FileSystem::{
            DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_WRITE_ATTRIBUTES,
        },
    };
    let mut staged = OpenOptions::new()
        .access_mode(GENERIC_READ | FILE_WRITE_ATTRIBUTES | DELETE)
        .share_mode(move_recovery::WINDOWS_CROSS_VOLUME_CANDIDATE_SHARE_MODE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(candidate)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    prepare_windows_private_candidate_for_reopen(candidate, &mut staged, record)?;
    drop(staged);
    move_recovery::open_reparse_safe_cross_volume_candidate(candidate)
        .map_err(|error| PlatformError::Io(error.to_string()))
}

#[cfg(target_os = "windows")]
fn copy_windows_move_source_metadata(
    source: &Path,
    source_handle: &mut Option<std::fs::File>,
    expected_source: move_recovery::WindowsStableIdentity,
    destination: &std::fs::File,
) -> Result<(), PlatformError> {
    if source_handle.is_none() {
        *source_handle = Some(
            move_recovery::open_reparse_safe_cross_volume_source(source)
                .map_err(|error| PlatformError::Io(error.to_string()))?,
        );
    }
    let source_handle = source_handle.as_ref().expect("metadata source opened");
    if move_recovery::stable_identity_from_handle(source_handle)
        .map_err(|error| PlatformError::Io(error.to_string()))?
        != expected_source
    {
        return Err(PlatformError::Io(
            "source changed before metadata publication".into(),
        ));
    }
    copy_windows_basic_metadata(source_handle, destination)
}

/// Reclaim only a verified publication while the complete original source is
/// pinned. Durable intent distinguishes this from an unexplained replacement gap.
#[cfg(target_os = "windows")]
fn reclaim_windows_readonly_publication(
    recovery_root: &Path,
    record: &mut WindowsCrossVolumeRecord,
    source_handle: &mut Option<std::fs::File>,
    target: &Path,
) -> Result<(), PlatformError> {
    let source = record.source_path()?;
    if source_handle.is_none() {
        *source_handle = Some(
            move_recovery::open_reparse_safe_cross_volume_source(&source)
                .map_err(|error| PlatformError::Io(error.to_string()))?,
        );
    }
    let verify = |path: &Path, handle: &mut std::fs::File, expected| -> Result<(), PlatformError> {
        if move_recovery::stable_identity_from_handle(handle)
            .map_err(|error| PlatformError::Io(error.to_string()))?
            != expected
            || observe_windows_identity(path)? != Some(expected)
            || !windows_stream_set_is_copy_safe(windows_file_streams(handle)?, false)
            || hash_windows_file(handle)? != (record.source_size, record.source_sha256.clone())
        {
            return Err(PlatformError::Io(format!(
                "read-only publication recovery lost file certainty: {}",
                path.display()
            )));
        }
        Ok(())
    };
    verify(
        &source,
        source_handle.as_mut().expect("recovery source pinned"),
        record.source_identity,
    )?;
    let mut published = move_recovery::open_reparse_safe_cross_volume_verification(target)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    verify(
        target,
        &mut published,
        record
            .candidate_identity
            .ok_or_else(|| PlatformError::Io("published candidate identity is missing".into()))?,
    )?;
    let transaction = record.transaction_path()?;
    let destination = record.destination_parent_path()?;
    let transaction_handle = move_recovery::open_reparse_safe_directory_for_rename(&transaction)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let destination_handle = move_recovery::open_reparse_safe_directory_for_rename(&destination)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    for (handle, expected) in [
        (&transaction_handle, record.transaction_directory_identity),
        (&destination_handle, record.destination_parent_identity),
    ] {
        if move_recovery::stable_identity_from_handle(handle)
            .map_err(|error| PlatformError::Io(error.to_string()))?
            != expected
        {
            return Err(PlatformError::Io(
                "publication recovery parent identity changed".into(),
            ));
        }
    }
    record.phase = WindowsCrossVolumePhase::ReclaimingPublication;
    persist_windows_cross_volume_record(recovery_root, record)?;
    rename_windows_handle_no_replace(
        &published,
        &transaction_handle,
        std::ffi::OsStr::new(WINDOWS_CROSS_VOLUME_CANDIDATE_NAME),
    )?;
    drop(published);
    transaction_handle
        .sync_all()
        .and_then(|()| destination_handle.sync_all())
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    #[cfg(test)]
    windows_move_boundary(
        WindowsMoveBoundary::PublicationReclaimed,
        &record.candidate_path()?,
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn completed_windows_cross_volume_overwrite(
    recovery_root: &Path,
    record: &mut WindowsCrossVolumeRecord,
) -> Result<Option<MovePublication>, PlatformError> {
    if record.conflict != ConflictAction::Overwrite
        || !matches!(
            record.phase,
            WindowsCrossVolumePhase::Published | WindowsCrossVolumePhase::SourceRemoved
        )
    {
        return Ok(None);
    }
    let transaction = record.transaction_path()?;
    let local = load_windows_cross_volume_record(&transaction.join(WINDOWS_LOCAL_JOURNAL_NAME))?;
    if record.phase != WindowsCrossVolumePhase::SourceRemoved
        && local.phase != WindowsCrossVolumePhase::SourceRemoved
    {
        return Ok(None);
    }
    let invalid = || PlatformError::CommittedCleanupPending {
        transaction: transaction.display().to_string(),
        message:
            "Windows completed retained Move evidence is inconsistent; all entries were preserved"
                .into(),
    };
    // SourceRemoved is written only after the source-removal namespace flush.
    // A crash can leave either journal one write ahead. Reconcile only that
    // exact phase transition; all other evidence must agree without inference
    // from source/target names, which may already describe later files.
    let mut completed = record.clone();
    completed.phase = WindowsCrossVolumePhase::SourceRemoved;
    let mut local_completed = local.clone();
    local_completed.phase = WindowsCrossVolumePhase::SourceRemoved;
    let destination = record.destination_parent_path()?;
    let target = record.target_path()?;
    let retained = record.retained_path()?.ok_or_else(invalid)?;
    let source = record.source_path()?;
    let source_parent = source.parent().ok_or_else(invalid)?;
    if !matches!(
        local.phase,
        WindowsCrossVolumePhase::Published | WindowsCrossVolumePhase::SourceRemoved
    ) || completed != local_completed
        || record.candidate_identity.is_none()
        || record.destination_identity.is_none()
        || transaction
            != destination.join(format!(
                ".ottrin-move-{}",
                encode_transaction_id(record.transaction_id()?)
            ))
        || target.parent() != Some(destination.as_path())
        || retained != transaction.join(WINDOWS_RETAINED_NAME)
        || !windows_staging_candidate_matches_transaction(&transaction, &record.candidate_path()?)
        || observe_windows_identity(&transaction)? != Some(record.transaction_directory_identity)
        || observe_windows_identity(&record.candidate_path()?)?.is_some()
        || observe_windows_identity(&retained)? != record.destination_identity
        || observe_windows_identity(&windows_directory_source_claim_path(source_parent, record)?)?
            .is_some()
    {
        return Err(invalid());
    }
    if record.phase != WindowsCrossVolumePhase::SourceRemoved
        || local.phase != WindowsCrossVolumePhase::SourceRemoved
    {
        *record = completed;
        persist_windows_cross_volume_record(recovery_root, record).map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: transaction.display().to_string(),
                message: format!(
                    "Windows completed retained Move journals could not converge: {error}"
                ),
            }
        })?;
    }
    Ok(Some(MovePublication {
        #[cfg(target_os = "windows")]
        windows_undo: None,
        disposition: MoveDisposition::Moved,
        published_path: Some(target),
        retained_displaced_path: Some(retained),
    }))
}

#[cfg(target_os = "windows")]
fn resume_windows_cross_volume(
    recovery_root: &Path,
    record: &mut WindowsCrossVolumeRecord,
    mut source_handle: Option<std::fs::File>,
) -> Result<MovePublication, PlatformError> {
    if let Some(publication) = completed_windows_cross_volume_overwrite(recovery_root, record)? {
        return Ok(publication);
    }
    if record.phase == WindowsCrossVolumePhase::SourceRemoved {
        return finish_windows_cross_volume_teardown(recovery_root, record);
    }
    let mut staged_handle = None;
    let transaction = record.transaction_path()?;
    let local_path = transaction.join(WINDOWS_LOCAL_JOURNAL_NAME);
    let local = match std::fs::read(&local_path) {
        Ok(bytes) => Some(
            serde_json::from_slice::<WindowsCrossVolumeRecord>(&bytes).map_err(|error| {
                PlatformError::ClaimedRecoveryPending {
                    transaction: transaction.display().to_string(),
                    message: format!("Windows cross-volume local journal is invalid: {error}"),
                }
            })?,
        ),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && record.phase == WindowsCrossVolumePhase::Prepared
                && record.candidate_identity.is_none() =>
        {
            None
        }
        Err(error) => {
            return Err(PlatformError::ClaimedRecoveryPending {
                transaction: transaction.display().to_string(),
                message: format!("Windows cross-volume local journal read failed: {error}"),
            });
        }
    };
    if let Some(local) = &local {
        if !record.same_transaction_evidence(local) {
            return Err(PlatformError::ClaimedRecoveryPending {
                transaction: transaction.display().to_string(),
                message: "Windows cross-volume durable journals disagree".into(),
            });
        }
        let reconciled_targets = reconcile_windows_rename_candidates(
            record.conflict,
            &record.target_candidates,
            &local.target_candidates,
        )
        .ok_or_else(|| PlatformError::ClaimedRecoveryPending {
            transaction: transaction.display().to_string(),
            message: "Windows cross-volume target candidate journals disagree".into(),
        })?;
        let journals_need_convergence = record.target_candidates != reconciled_targets
            || local.target_candidates != reconciled_targets;
        record.target_candidates = reconciled_targets;
        if record.candidate_identity.is_none() {
            record.candidate_identity = local.candidate_identity;
        }
        if journals_need_convergence {
            persist_windows_cross_volume_record(recovery_root, record).map_err(|error| {
                PlatformError::ClaimedRecoveryPending {
                    transaction: transaction.display().to_string(),
                    message: format!(
                        "Windows cross-volume candidate journal convergence failed: {error}"
                    ),
                }
            })?;
        }
    }
    let source = record.source_path()?;
    let source_parent = source
        .parent()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    let claimed_directory_source = windows_directory_source_claim_path(source_parent, record)?;
    let destination_parent = record.destination_parent_path()?;
    let mut target = record.target_path()?;
    let candidate = record.candidate_path()?;
    let retained_displaced = record.retained_path()?;
    if !windows_staging_candidate_matches_transaction(&transaction, &candidate) {
        return Err(PlatformError::ClaimedRecoveryPending {
            transaction: transaction.display().to_string(),
            message: "Windows cross-volume staging candidate escaped its transaction directory"
                .into(),
        });
    }
    let overwrite_evidence_is_complete = record.conflict == ConflictAction::Overwrite
        && record.destination_identity.is_some()
        && retained_displaced.is_some();
    if (record.conflict == ConflictAction::Overwrite) != overwrite_evidence_is_complete
        || (record.conflict != ConflictAction::Overwrite
            && (record.destination_identity.is_some() || retained_displaced.is_some()))
        || (record.source_kind == WindowsCrossVolumeSourceKind::File
            && (!record.source_manifest.is_empty() || !record.destination_manifest.is_empty()))
        || (record.source_kind == WindowsCrossVolumeSourceKind::Directory
            && record.conflict != ConflictAction::Overwrite
            && !record.destination_manifest.is_empty())
        || ((record.source_kind == WindowsCrossVolumeSourceKind::SymbolicLink)
            != (record.source_link_target.is_some() && record.source_link_is_directory.is_some()))
        || (record.source_kind != WindowsCrossVolumeSourceKind::SymbolicLink
            && (record.source_link_target.is_some() || record.source_link_is_directory.is_some()))
    {
        return Err(PlatformError::ClaimedRecoveryPending {
            transaction: transaction.display().to_string(),
            message: "Windows cross-volume conflict evidence is incomplete or inconsistent".into(),
        });
    }
    for (path, expected) in [
        (source_parent, record.source_parent_identity),
        (
            destination_parent.as_path(),
            record.destination_parent_identity,
        ),
        (transaction.as_path(), record.transaction_directory_identity),
    ] {
        if observe_windows_identity(path)? != Some(expected) {
            return Err(PlatformError::ClaimedRecoveryPending {
                transaction: transaction.display().to_string(),
                message: format!(
                    "Windows cross-volume parent identity changed: {}",
                    path.display()
                ),
            });
        }
    }

    if local.is_none() {
        // The central Prepared intent is written first. A process can stop before
        // the local counterpart exists, but no payload work is then authorized.
        // Complete only that empty, identity-bound transaction, never a later
        // missing journal or an occupied name.
        use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        let directory = OpenOptions::new()
            .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&transaction)
            .map_err(|error| PlatformError::ClaimedRecoveryPending {
                transaction: transaction.display().to_string(),
                message: format!(
                    "Windows initial journal repair could not pin its directory: {error}"
                ),
            })?;
        if move_recovery::stable_identity_from_handle(&directory)
            .map_err(|error| PlatformError::Io(error.to_string()))?
            != record.transaction_directory_identity
            || !crate::windows_fs::directory_children(&directory)
                .map_err(|error| PlatformError::Io(error.to_string()))?
                .is_empty()
            || observe_windows_identity(&source)? != Some(record.source_identity)
            || observe_windows_identity(&target)? != record.destination_identity
            || (record.source_kind == WindowsCrossVolumeSourceKind::Directory
                && observe_windows_identity(&claimed_directory_source)?.is_some())
        {
            return Err(PlatformError::ClaimedRecoveryPending {
                transaction: transaction.display().to_string(),
                message: "Windows incomplete initial journal pair has changed or staged entries"
                    .into(),
            });
        }
        write_windows_record_file(&local_path, record, true)?;
    }

    loop {
        let source_observed = observe_windows_identity(&source)?;
        let claimed_directory_source_observed =
            if record.source_kind == WindowsCrossVolumeSourceKind::Directory {
                observe_windows_identity(&claimed_directory_source)?
            } else {
                None
            };
        let target_observed = observe_windows_identity(&target)?;
        let candidate_observed = observe_windows_identity(&candidate)?;
        if classify_windows_incomplete_directory_candidate(
            record.source_kind,
            source_observed == Some(record.source_identity),
            target_observed.is_none(),
            record.phase == WindowsCrossVolumePhase::Prepared,
            record.candidate_identity.is_some(),
            candidate_observed == record.candidate_identity,
            candidate_observed.is_some(),
        ) == Some(WindowsCrossVolumeRestartAction::DiscardIncompleteStaging)
        {
            let cleanup_identity = match (record.candidate_identity, candidate_observed) {
                (Some(expected), Some(observed)) if expected == observed => expected,
                (None, Some(observed)) => observed,
                _ => unreachable!("classified identity-pinnable private candidate"),
            };
            discard_incomplete_windows_directory_candidate(
                &candidate,
                cleanup_identity,
            )
            .map_err(|error| {
                PlatformError::ClaimedRecoveryPending {
                    transaction: transaction.display().to_string(),
                    message: format!(
                        "incomplete private Windows directory staging could not be discarded safely; source and recovery evidence were preserved: {error}"
                    ),
                }
            })?;
            record.candidate_identity = None;
            persist_windows_cross_volume_record(recovery_root, record).map_err(|error| {
                PlatformError::ClaimedRecoveryPending {
                    transaction: transaction.display().to_string(),
                    message: format!(
                        "Windows cross-volume incomplete staging cleanup was not durable: {error}"
                    ),
                }
            })?;
            continue;
        }
        let candidate_is_still_staged = match record.candidate_identity {
            Some(expected) => candidate_observed == Some(expected),
            None => candidate_observed.is_none(),
        };
        if record.conflict == ConflictAction::Rename
            && source_observed == Some(record.source_identity)
            && target_observed.is_some()
            && target_observed != record.candidate_identity
            && candidate_is_still_staged
        {
            target = advance_windows_cross_volume_rename_target(recovery_root, record)?;
            continue;
        }
        let action = if record.conflict == ConflictAction::Overwrite {
            match (record.candidate_identity, record.destination_identity) {
                (Some(expected_staged), Some(expected_displaced)) => {
                    classify_windows_cross_volume_overwrite_restart(
                        record.source_kind != WindowsCrossVolumeSourceKind::File
                            || record.phase == WindowsCrossVolumePhase::ReclaimingPublication,
                        source_observed,
                        target_observed,
                        candidate_observed,
                        observe_windows_identity(
                            retained_displaced
                                .as_deref()
                                .expect("validated retained Overwrite path"),
                        )?,
                        record.source_identity,
                        expected_staged,
                        expected_displaced,
                    )
                }
                (None, Some(expected_displaced))
                    if source_observed == Some(record.source_identity)
                        && target_observed == Some(expected_displaced)
                        && candidate_observed.is_none()
                        && observe_windows_identity(
                            retained_displaced
                                .as_deref()
                                .expect("validated retained Overwrite path"),
                        )?
                        .is_none() =>
                {
                    WindowsCrossVolumeRestartAction::CopyAndVerify
                }
                _ => WindowsCrossVolumeRestartAction::RetainAmbiguous,
            }
        } else {
            classify_windows_cross_volume_restart(
                source_observed,
                target_observed,
                candidate_observed,
                record.source_identity,
                record.candidate_identity,
            )
        };
        if record.source_kind == WindowsCrossVolumeSourceKind::Directory
            && record.conflict == ConflictAction::Overwrite
        {
            let retained_path = retained_displaced
                .as_deref()
                .expect("validated retained Overwrite path");
            if observe_windows_identity(retained_path)? == record.destination_identity {
                let retained_manifest =
                    move_recovery::open_reparse_safe_cross_volume_source(retained_path)
                        .map_err(|error| PlatformError::Io(error.to_string()))
                        .and_then(|handle| {
                            capture_windows_directory_snapshot(retained_path, &handle)
                        })
                        .and_then(|snapshot| snapshot.journal_entries());
                match retained_manifest {
                    Ok(manifest) if manifest == record.destination_manifest => {}
                    Ok(_) | Err(_) if record.phase == WindowsCrossVolumePhase::Published => {
                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: "Windows cross-volume retained entry contents changed after publication"
                                .into(),
                        });
                    }
                    Ok(_) | Err(_) => {
                        return Err(PlatformError::ClaimedRecoveryPending {
                            transaction: transaction.display().to_string(),
                            message: "Windows cross-volume retained entry contents changed before publication"
                                .into(),
                        });
                    }
                }
            }
        }
        let action = require_windows_publication_durability(
            action,
            record.phase == WindowsCrossVolumePhase::Published,
        );
        let action = if record.source_kind == WindowsCrossVolumeSourceKind::Directory {
            if claimed_directory_source_observed.is_some()
                && claimed_directory_source_observed != Some(record.source_identity)
            {
                WindowsCrossVolumeRestartAction::RetainAmbiguous
            } else {
                match classify_windows_directory_cleanup_topology(
                    source_observed == Some(record.source_identity),
                    claimed_directory_source_observed == Some(record.source_identity),
                ) {
                    WindowsDirectoryCleanupTopology::DrainClaimedSource
                        if action == WindowsCrossVolumeRestartAction::Complete =>
                    {
                        WindowsCrossVolumeRestartAction::RemoveSource
                    }
                    WindowsDirectoryCleanupTopology::RetainAmbiguous => {
                        WindowsCrossVolumeRestartAction::RetainAmbiguous
                    }
                    _ => action,
                }
            }
        } else {
            action
        };
        let action = classify_windows_directory_slice_action(record.source_kind, action);
        match action {
            WindowsCrossVolumeRestartAction::DiscardIncompleteStaging => {
                return Err(PlatformError::ClaimedRecoveryPending {
                    transaction: transaction.display().to_string(),
                    message: "incomplete Windows staging cleanup was not resolved before restart classification"
                        .into(),
                });
            }
            WindowsCrossVolumeRestartAction::CopyAndVerify => {
                if source_handle.is_none() {
                    source_handle = Some(
                        move_recovery::open_reparse_safe_cross_volume_source(&source).map_err(
                            |error| PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume source reopen failed: {error}"
                                ),
                            },
                        )?,
                    );
                }
                let retained_source = source_handle
                    .as_ref()
                    .expect("source handle is retained after successful reopen");
                if move_recovery::stable_identity_from_handle(retained_source)
                    .map_err(|error| PlatformError::Io(error.to_string()))?
                    != record.source_identity
                {
                    return Err(PlatformError::ClaimedRecoveryPending {
                        transaction: transaction.display().to_string(),
                        message: "Windows cross-volume source identity changed before copy".into(),
                    });
                }
                let mut retained_directory_snapshots = None;
                let staged = match record.source_kind {
                    WindowsCrossVolumeSourceKind::File => {
                        let mut source_copy = retained_source.try_clone().map_err(|error| {
                            PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume source clone failed: {error}"
                                ),
                            }
                        })?;
                        source_copy
                            .seek(std::io::SeekFrom::Start(0))
                            .map_err(|error| PlatformError::Io(error.to_string()))?;
                        let mut staged = OpenOptions::new()
                            .create_new(true)
                            .write(true)
                            .access_mode(move_recovery::WINDOWS_CROSS_VOLUME_CANDIDATE_ACCESS_MODE)
                            .share_mode(move_recovery::WINDOWS_CROSS_VOLUME_CANDIDATE_SHARE_MODE)
                            .open(&candidate)
                            .map_err(|error| PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume stage creation failed: {error}"
                                ),
                            })?;
                        copy_windows_file_and_metadata(&mut source_copy, &mut staged).map_err(
                            |error| PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: format!("Windows cross-volume stage copy failed: {error}"),
                            },
                        )?;
                        let staged_evidence = hash_windows_file(&mut staged)?;
                        let source_evidence = hash_windows_file(&mut source_copy)?;
                        if staged_evidence != (record.source_size, record.source_sha256.clone())
                            || source_evidence != staged_evidence
                            || observe_windows_identity(&source)? != Some(record.source_identity)
                        {
                            return Err(PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message:
                                    "Windows cross-volume copy verification lost source certainty"
                                        .into(),
                            });
                        }
                        staged
                    }
                    WindowsCrossVolumeSourceKind::Directory => {
                        std::fs::create_dir(&candidate).map_err(|error| {
                            PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume private directory stage creation failed: {error}"
                                ),
                            }
                        })?;
                        let candidate_handle =
                            move_recovery::open_reparse_safe_cross_volume_cleanup_entry(&candidate)
                                .map_err(|error| PlatformError::ClaimedRecoveryPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows cross-volume private directory stage reopen failed: {error}"
                                    ),
                                })?;
                        record.candidate_identity = Some(
                            move_recovery::stable_identity_from_handle(&candidate_handle)
                                .map_err(|error| PlatformError::Io(error.to_string()))?,
                        );
                        persist_windows_cross_volume_record(recovery_root, record).map_err(
                            |error| PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume private directory stage identity was not durable: {error}"
                                ),
                            },
                        )?;
                        drop(candidate_handle);
                        let mut source_snapshot =
                            capture_windows_directory_snapshot(&source, retained_source).map_err(
                                |error| PlatformError::ClaimedRecoveryPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows cross-volume directory snapshot failed: {error}"
                                    ),
                                },
                            )?;
                        if source_snapshot.evidence
                            != (record.source_size, record.source_sha256.clone())
                            || source_snapshot.journal_entries()? != record.source_manifest
                        {
                            return Err(PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: "Windows cross-volume directory changed before staging"
                                    .into(),
                            });
                        }
                        let (staged, staged_snapshot) = stage_windows_directory_snapshot(
                            retained_source,
                            &mut source_snapshot,
                            &candidate,
                        )
                        .map_err(|error| PlatformError::ClaimedRecoveryPending {
                            transaction: transaction.display().to_string(),
                            message: format!(
                                "Windows cross-volume directory staging failed: {error}"
                            ),
                        })?;
                        if staged_snapshot.evidence
                            != (record.source_size, record.source_sha256.clone())
                            || observe_windows_identity(&source)? != Some(record.source_identity)
                        {
                            return Err(PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: "Windows cross-volume staged directory verification lost source certainty"
                                    .into(),
                            });
                        }
                        retained_directory_snapshots = Some((source_snapshot, staged_snapshot));
                        staged
                    }
                    WindowsCrossVolumeSourceKind::SymbolicLink => {
                        let target = record
                            .source_link_target
                            .as_deref()
                            .expect("validated symbolic-link target evidence");
                        let is_directory = record
                            .source_link_is_directory
                            .expect("validated symbolic-link kind evidence");
                        verify_windows_symbolic_link(&source, target, is_directory)?;
                        create_windows_symbolic_link(&candidate, target, is_directory)?;
                        let staged =
                            move_recovery::open_reparse_safe_cross_volume_candidate(&candidate)
                                .map_err(|error| PlatformError::Io(error.to_string()))?;
                        if verify_windows_symbolic_link(&candidate, target, is_directory)?
                            != (record.source_size, record.source_sha256.clone())
                            || observe_windows_identity(&source)? != Some(record.source_identity)
                        {
                            return Err(PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: "Windows cross-volume staged symbolic link lost source certainty"
                                    .into(),
                            });
                        }
                        staged
                    }
                };
                record.candidate_identity = Some(
                    move_recovery::stable_identity_from_handle(&staged)
                        .map_err(|error| PlatformError::Io(error.to_string()))?,
                );
                record.phase = WindowsCrossVolumePhase::Staged;
                persist_windows_cross_volume_record(recovery_root, record).map_err(|error| {
                    PlatformError::ClaimedRecoveryPending {
                        transaction: transaction.display().to_string(),
                        message: format!(
                            "Windows cross-volume staged state was not durable: {error}"
                        ),
                    }
                })?;
                drop(retained_directory_snapshots);
                if record.source_kind == WindowsCrossVolumeSourceKind::File {
                    // Metadata may have made the candidate read-only. Keep the
                    // original writable handle for publication and durability.
                    staged_handle = Some(staged);
                }
                #[cfg(test)]
                if std::env::var_os("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_EXIT_AFTER_STAGED").is_some()
                {
                    std::process::exit(86);
                }
            }
            WindowsCrossVolumeRestartAction::PublishStaged => {
                let recovered_file = staged_handle.is_none()
                    && record.source_kind == WindowsCrossVolumeSourceKind::File;
                let mut staged = match staged_handle.take() {
                    Some(staged) => Ok(staged),
                    None if recovered_file => reopen_windows_staged_file(&candidate, record),
                    None => move_recovery::open_reparse_safe_cross_volume_candidate(&candidate)
                        .map_err(|error| PlatformError::Io(error.to_string())),
                }
                        .map_err(|error| PlatformError::ClaimedRecoveryPending {
                            transaction: transaction.display().to_string(),
                            message: format!(
                                "Windows cross-volume candidate could not be locked for publication: {error}"
                            ),
                        })?;
                let mut _retained_source_snapshot = None;
                let mut _retained_candidate_snapshot = None;
                let staged_evidence = match record.source_kind {
                    WindowsCrossVolumeSourceKind::File => hash_windows_file(&mut staged),
                    WindowsCrossVolumeSourceKind::Directory => {
                        if source_handle.is_none() {
                            source_handle = Some(move_recovery::open_reparse_safe_cross_volume_source(&source)
                                .map_err(|error| PlatformError::Io(error.to_string()))?);
                        }
                        let source_root_handle = source_handle.as_ref().expect("source handle acquired for verification");
                        let source_snapshot = capture_windows_directory_snapshot(&source, source_root_handle).map_err(
                            |error| PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume source directory reverification failed: {error}"
                                ),
                            },
                        )?;
                        let source_manifest = source_snapshot.journal_entries().map_err(|error| {
                            PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume source directory manifest could not be reverified: {error}"
                                ),
                            }
                        })?;
                        if source_snapshot.evidence
                            != (record.source_size, record.source_sha256.clone())
                            || source_manifest != record.source_manifest
                            || !move_recovery::stable_identity_from_handle(&staged)
                                .map_err(|error| PlatformError::ClaimedRecoveryPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows cross-volume staged directory identity probe failed: {error}"
                                    ),
                                })?
                                .is_directory_without_reparse()
                        {
                            return Err(PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: "Windows cross-volume source or staged directory changed before publication"
                                    .into(),
                            });
                        }
                        _retained_source_snapshot = Some(source_snapshot);
                        let candidate_snapshot =
                            capture_windows_directory_snapshot(&candidate, &staged)?;
                        let evidence = candidate_snapshot.evidence.clone();
                        _retained_candidate_snapshot = Some(candidate_snapshot);
                        Ok(evidence)
                    }
                    WindowsCrossVolumeSourceKind::SymbolicLink => {
                        let target = record
                            .source_link_target
                            .as_deref()
                            .expect("validated symbolic-link target evidence");
                        let is_directory = record
                            .source_link_is_directory
                            .expect("validated symbolic-link kind evidence");
                        let source_evidence =
                            verify_windows_symbolic_link(&source, target, is_directory)?;
                        let candidate_evidence =
                            verify_windows_symbolic_link(&candidate, target, is_directory)?;
                        if source_evidence != candidate_evidence {
                            return Err(PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: "Windows cross-volume source or staged symbolic link changed before publication"
                                    .into(),
                            });
                        }
                        Ok(candidate_evidence)
                    }
                }
                .map_err(|error| PlatformError::ClaimedRecoveryPending {
                    transaction: transaction.display().to_string(),
                    message: format!(
                        "Windows cross-volume candidate could not be reverified before publication: {error}"
                    ),
                })?;
                if move_recovery::stable_identity_from_handle(&staged)
                    .map_err(|error| PlatformError::Io(error.to_string()))?
                    != record
                        .candidate_identity
                        .expect("classified staged identity")
                    || staged_evidence != (record.source_size, record.source_sha256.clone())
                    || observe_windows_identity(&source)? != Some(record.source_identity)
                {
                    return Err(PlatformError::ClaimedRecoveryPending {
                        transaction: transaction.display().to_string(),
                        message: "Windows cross-volume identities changed before publication"
                            .into(),
                    });
                }
                if recovered_file && record.conflict != ConflictAction::Overwrite {
                    copy_windows_move_source_metadata(
                        &source,
                        &mut source_handle,
                        record.source_identity,
                        &staged,
                    )
                    .map_err(|error| {
                        PlatformError::ClaimedRecoveryPending {
                            transaction: transaction.display().to_string(),
                            message: format!("staged metadata recovery failed: {error}"),
                        }
                    })?;
                }
                let destination_handle =
                    move_recovery::open_reparse_safe_directory_for_rename(&destination_parent)
                        .map_err(|error| PlatformError::Io(error.to_string()))?;
                let opened_destination_identity = move_recovery::stable_identity_from_handle(
                    &destination_handle,
                )
                .map_err(|error| PlatformError::ClaimedRecoveryPending {
                    transaction: transaction.display().to_string(),
                    message: format!(
                        "Windows cross-volume destination identity probe failed: {error}"
                    ),
                })?;
                if !windows_destination_parent_is_pinned(
                    opened_destination_identity,
                    record.destination_parent_identity,
                ) {
                    return Err(PlatformError::ClaimedRecoveryPending {
                        transaction: transaction.display().to_string(),
                        message: "Windows cross-volume destination parent changed before publication; source and staged candidate were preserved".into(),
                    });
                }
                // Windows refuses a directory rename while descendant files are open.
                // Keep the root identity pinned, then reverify the published tree before
                // any source removal; uncertainty preserves the complete source.
                if record.source_kind == WindowsCrossVolumeSourceKind::Directory {
                    drop(_retained_candidate_snapshot.take());
                    #[cfg(test)]
                    windows_move_boundary(WindowsMoveBoundary::CandidateUnlocked, &candidate);
                }
                let publication = if record.conflict == ConflictAction::Overwrite {
                    let retained = retained_displaced
                        .as_deref()
                        .expect("validated retained Overwrite path");
                    let mut published = if record.source_kind != WindowsCrossVolumeSourceKind::File
                        || target_observed.is_none()
                    {
                        let transaction_handle =
                            move_recovery::open_reparse_safe_directory_for_rename(&transaction)
                                .map_err(|error| PlatformError::ClaimedRecoveryPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows cross-volume transaction directory could not be locked for publication: {error}"
                                    ),
                                })?;
                        if target_observed == record.destination_identity {
                            let displaced =
                                move_recovery::open_reparse_safe_cross_volume_candidate(&target)
                                    .map_err(|error| PlatformError::ClaimedRecoveryPending {
                                        transaction: transaction.display().to_string(),
                                        message: format!(
                                            "Windows cross-volume displaced directory could not be locked for retention: {error}"
                                        ),
                                    })?;
                            if move_recovery::stable_identity_from_handle(&displaced)
                                .map_err(|error| PlatformError::Io(error.to_string()))?
                                != record
                                    .destination_identity
                                    .expect("validated displaced destination identity")
                            {
                                return Err(PlatformError::ClaimedRecoveryPending {
                                    transaction: transaction.display().to_string(),
                                    message: "Windows cross-volume displaced directory changed before retention"
                                        .into(),
                                });
                            }
                            publish_windows_cross_volume_directory_overwrite_with(
                                || {
                                    rename_windows_handle_no_replace(
                                        &displaced,
                                        &transaction_handle,
                                        std::ffi::OsStr::new(WINDOWS_RETAINED_NAME),
                                    )
                                },
                                || {
                                    displaced.sync_all().map_err(|error| {
                                        PlatformError::Io(format!(
                                            "Windows cross-volume retained entry flush failed: {error}"
                                        ))
                                    })?;
                                    destination_handle.sync_all().map_err(|error| {
                                        PlatformError::Io(format!(
                                            "Windows cross-volume displaced namespace flush failed: {error}"
                                        ))
                                    })?;
                                    transaction_handle.sync_all().map_err(|error| {
                                        PlatformError::Io(format!(
                                            "Windows cross-volume retained namespace flush failed: {error}"
                                        ))
                                    })?;
                                    #[cfg(test)]
                                    windows_move_boundary(
                                        WindowsMoveBoundary::DisplacedRetained,
                                        retained,
                                    );
                                    Ok(())
                                },
                                || {
                                    rename_windows_handle_no_replace(
                                        &staged,
                                        &destination_handle,
                                        target.file_name().ok_or_else(|| {
                                            PlatformError::InvalidDestination(
                                                target.display().to_string(),
                                            )
                                        })?,
                                    )
                                },
                                || {
                                    staged.sync_all().map_err(|error| {
                                        PlatformError::Io(format!(
                                            "Windows cross-volume published directory flush failed: {error}"
                                        ))
                                    })
                                },
                                || {
                                    destination_handle.sync_all().map_err(|error| {
                                        PlatformError::Io(format!(
                                            "Windows cross-volume publication namespace flush failed: {error}"
                                        ))
                                    })?;
                                    transaction_handle.sync_all().map_err(|error| {
                                        PlatformError::Io(format!(
                                            "Windows cross-volume transaction namespace flush failed: {error}"
                                        ))
                                    })
                                },
                            )?;
                        } else {
                            let retained_handle =
                                move_recovery::open_reparse_safe_cross_volume_candidate(retained)
                                    .map_err(|error| PlatformError::ClaimedRecoveryPending {
                                        transaction: transaction.display().to_string(),
                                        message: format!(
                                            "Windows cross-volume retained entry could not be locked before recovered publication: {error}"
                                        ),
                                    })?;
                            retained_handle.sync_all().map_err(|error| {
                                PlatformError::ClaimedRecoveryPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows cross-volume retained entry was not durable before recovered publication: {error}"
                                    ),
                                }
                            })?;
                            destination_handle.sync_all().map_err(|error| {
                                PlatformError::ClaimedRecoveryPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows cross-volume displaced namespace was not durable before recovered publication: {error}"
                                    ),
                                }
                            })?;
                            transaction_handle.sync_all().map_err(|error| {
                                PlatformError::ClaimedRecoveryPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows cross-volume retained namespace was not durable before recovered publication: {error}"
                                    ),
                                }
                            })?;
                            publish_windows_cross_volume_candidate_with(
                                || {
                                    rename_windows_handle_no_replace(
                                        &staged,
                                        &destination_handle,
                                        target.file_name().ok_or_else(|| {
                                            PlatformError::InvalidDestination(
                                                target.display().to_string(),
                                            )
                                        })?,
                                    )
                                },
                                || {
                                    staged.sync_all().map_err(|error| {
                                        PlatformError::Io(format!(
                                            "Windows cross-volume recovered entry publication flush failed: {error}"
                                        ))
                                    })
                                },
                                || {
                                    destination_handle.sync_all().map_err(|error| {
                                        PlatformError::Io(format!(
                                            "Windows cross-volume recovered publication namespace flush failed: {error}"
                                        ))
                                    })?;
                                    transaction_handle.sync_all().map_err(|error| {
                                        PlatformError::Io(format!(
                                            "Windows cross-volume recovered transaction namespace flush failed: {error}"
                                        ))
                                    })
                                },
                            )?;
                        }
                        // Handle-relative rename preserves this exclusive handle.
                        // Reopening its new pathname would conflict with our own lock.
                        staged
                    } else {
                        prepare_windows_private_candidate_for_reopen(
                            &candidate,
                            &mut staged,
                            record,
                        )
                        .map_err(|error| {
                            PlatformError::ClaimedRecoveryPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Overwrite candidate metadata preparation failed: {error}"
                                ),
                            }
                        })?;
                        drop(staged);
                        replace_windows_file_retaining_displaced(
                            &target,
                            &candidate,
                            retained,
                            &transaction,
                            record
                                .candidate_identity
                                .expect("classified staged identity"),
                            record
                                .destination_identity
                                .expect("validated displaced destination identity"),
                        )?;
                        #[cfg(test)]
                        windows_move_boundary(WindowsMoveBoundary::FileReplaced, &target);
                        move_recovery::open_reparse_safe_cross_volume_candidate(&target).map_err(
                            |error| PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume Overwrite target could not be locked after publication: {error}"
                                ),
                            },
                        )?
                    };
                    let published_evidence = match record.source_kind {
                        WindowsCrossVolumeSourceKind::File => hash_windows_file(&mut published)?,
                        WindowsCrossVolumeSourceKind::Directory => {
                            capture_windows_directory_snapshot(&target, &published)?.evidence
                        }
                        WindowsCrossVolumeSourceKind::SymbolicLink => {
                            let expected_target = record
                                .source_link_target
                                .as_deref()
                                .expect("validated symbolic-link target evidence");
                            let expected_is_directory = record
                                .source_link_is_directory
                                .expect("validated symbolic-link kind evidence");
                            verify_windows_symbolic_link(
                                &target,
                                expected_target,
                                expected_is_directory,
                            )?
                        }
                    };
                    if move_recovery::stable_identity_from_handle(&published)
                        .map_err(|error| PlatformError::Io(error.to_string()))?
                        != record
                            .candidate_identity
                            .expect("classified staged identity")
                        || published_evidence != (record.source_size, record.source_sha256.clone())
                    {
                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: "Windows cross-volume Overwrite target changed before durability flush"
                                .into(),
                        });
                    }
                    if record.source_kind == WindowsCrossVolumeSourceKind::File {
                        copy_windows_move_source_metadata(
                            &source,
                            &mut source_handle,
                            record.source_identity,
                            &published,
                        )
                        .map_err(|error| {
                            PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Overwrite source metadata publication failed: {error}"
                                ),
                            }
                        })?;
                        #[cfg(test)]
                        windows_move_boundary(WindowsMoveBoundary::FilePublicationFlushed, &target);
                    } else {
                        published.sync_all().map_err(|error| {
                            PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume Overwrite publication flush failed: {error}"
                                ),
                            }
                        })?;
                    }
                    let retained_handle = move_recovery::open_reparse_safe_cross_volume_candidate(
                        retained,
                    )
                    .map_err(|error| {
                        PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: format!(
                                "Windows cross-volume retained destination reopen failed: {error}"
                            ),
                        }
                    })?;
                    if move_recovery::stable_identity_from_handle(&retained_handle)
                        .map_err(|error| PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: format!(
                                "Windows cross-volume retained destination identity check failed: {error}"
                            ),
                        })?
                        != record
                            .destination_identity
                            .expect("validated displaced destination identity")
                    {
                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message:
                                "Windows cross-volume retained destination changed before durability flush"
                                    .into(),
                        });
                    }
                    retained_handle.sync_all().map_err(|error| {
                        PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: format!(
                                "Windows cross-volume retained destination flush failed: {error}"
                            ),
                        }
                    })?;
                    destination_handle.sync_all().map_err(|error| {
                        PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: format!(
                                "Windows cross-volume Overwrite namespace flush failed: {error}"
                            ),
                        }
                    })
                } else {
                    // Retain this handle if Rename must select another target.
                    staged_handle = Some(
                        staged
                            .try_clone()
                            .map_err(|error| PlatformError::Io(error.to_string()))?,
                    );
                    publish_windows_cross_volume_candidate_with(
                        || {
                            rename_windows_handle_no_replace(
                                &staged,
                                &destination_handle,
                                target.file_name().ok_or_else(|| {
                                    PlatformError::InvalidDestination(target.display().to_string())
                                })?,
                            )
                        },
                        || {
                            staged.sync_all().map_err(|error| {
                                PlatformError::Io(format!(
                                    "Windows cross-volume published candidate flush failed: {error}"
                                ))
                            })
                        },
                        || {
                            destination_handle.sync_all().map_err(|error| {
                                PlatformError::Io(format!(
                                    "Windows cross-volume publication namespace flush failed: {error}"
                                ))
                            })
                        },
                    )
                };
                match publication {
                    Ok(()) => {}
                    Err(PlatformError::DestinationExists(_))
                        if record.conflict == ConflictAction::Rename =>
                    {
                        target = advance_windows_cross_volume_rename_target(recovery_root, record)?;
                        continue;
                    }
                    Err(error) => {
                        return Err(PlatformError::ClaimedRecoveryPending {
                            transaction: transaction.display().to_string(),
                            message: format!(
                                "Windows cross-volume publication needs recovery: {error}"
                            ),
                        });
                    }
                }
                drop(staged_handle.take());
                record.phase = WindowsCrossVolumePhase::Published;
                persist_windows_cross_volume_record(recovery_root, record).map_err(|error| {
                    PlatformError::CommittedCleanupPending {
                        transaction: transaction.display().to_string(),
                        message: format!(
                            "Windows cross-volume published state was not durable: {error}"
                        ),
                    }
                })?;
                #[cfg(test)]
                if record.source_kind == WindowsCrossVolumeSourceKind::Directory {
                    windows_move_boundary(WindowsMoveBoundary::CandidatePublished, &target);
                }
                drop((_retained_source_snapshot, _retained_candidate_snapshot));
            }
            WindowsCrossVolumeRestartAction::FlushPublished => {
                if record.source_kind == WindowsCrossVolumeSourceKind::File
                    && std::fs::symlink_metadata(&target)
                        .map_err(|error| PlatformError::Io(error.to_string()))?
                        .permissions()
                        .readonly()
                {
                    reclaim_windows_readonly_publication(
                        recovery_root,
                        record,
                        &mut source_handle,
                        &target,
                    )
                    .map_err(|error| {
                        PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: format!("read-only publication reclaim failed: {error}"),
                        }
                    })?;
                    continue;
                }
                let mut published =
                    move_recovery::open_reparse_safe_cross_volume_candidate(&target).map_err(
                        |error| PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: format!(
                                "Windows cross-volume published candidate could not be locked for durability recovery: {error}"
                            ),
                        },
                    )?;
                let mut _retained_recovery_source_snapshot = None;
                let mut _retained_published_snapshot = None;
                let published_evidence = match record.source_kind {
                    WindowsCrossVolumeSourceKind::File => hash_windows_file(&mut published),
                    WindowsCrossVolumeSourceKind::Directory => {
                        if source_handle.is_none() {
                            source_handle = Some(move_recovery::open_reparse_safe_cross_volume_source(&source)
                                .map_err(|error| PlatformError::Io(error.to_string()))?);
                        }
                        let source_root_handle = source_handle.as_ref().expect("source handle acquired for verification");
                        let source_snapshot = capture_windows_directory_snapshot(&source, source_root_handle)?;
                        let source_manifest = source_snapshot.journal_entries()?;
                        if source_snapshot.evidence
                            != (record.source_size, record.source_sha256.clone())
                            || source_manifest != record.source_manifest
                        {
                            return Err(PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: "Windows cross-volume source directory changed before publication durability recovery".into(),
                            });
                        }
                        let published_snapshot = capture_windows_directory_snapshot(&target, &published)?;
                        let evidence = published_snapshot.evidence.clone();
                        _retained_recovery_source_snapshot = Some(source_snapshot);
                        _retained_published_snapshot = Some(published_snapshot);
                        Ok(evidence)
                    }
                    WindowsCrossVolumeSourceKind::SymbolicLink => {
                        let expected_target = record.source_link_target.as_deref().ok_or_else(|| {
                            PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: "Windows cross-volume symbolic-link target evidence is missing"
                                    .into(),
                            }
                        })?;
                        let expected_is_directory =
                            record.source_link_is_directory.ok_or_else(|| {
                                PlatformError::CommittedCleanupPending {
                                    transaction: transaction.display().to_string(),
                                    message: "Windows cross-volume symbolic-link kind evidence is missing"
                                        .into(),
                                }
                            })?;
                        let source_evidence = verify_windows_symbolic_link(
                            &source,
                            expected_target,
                            expected_is_directory,
                        )?;
                        let published_evidence = verify_windows_symbolic_link(
                            &target,
                            expected_target,
                            expected_is_directory,
                        )?;
                        if source_evidence != published_evidence {
                            return Err(PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: "Windows cross-volume symbolic-link evidence changed before publication durability recovery"
                                    .into(),
                            });
                        }
                        Ok(published_evidence)
                    }
                }
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: transaction.display().to_string(),
                    message: format!(
                        "Windows cross-volume published candidate could not be reverified for durability recovery: {error}"
                    ),
                })?;
                if move_recovery::stable_identity_from_handle(&published)
                    .map_err(|error| PlatformError::Io(error.to_string()))?
                    != record
                        .candidate_identity
                        .expect("classified published candidate identity")
                    || published_evidence != (record.source_size, record.source_sha256.clone())
                    || observe_windows_identity(&source)? != Some(record.source_identity)
                {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: transaction.display().to_string(),
                        message: "Windows cross-volume identities changed before publication durability recovery"
                            .into(),
                    });
                }
                let destination_handle =
                    move_recovery::open_reparse_safe_directory_for_rename(&destination_parent)
                        .map_err(|error| PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: format!(
                                "Windows cross-volume destination parent could not be locked for durability recovery: {error}"
                            ),
                        })?;
                if move_recovery::stable_identity_from_handle(&destination_handle)
                    .map_err(|error| PlatformError::Io(error.to_string()))?
                    != record.destination_parent_identity
                {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: transaction.display().to_string(),
                        message: "Windows cross-volume destination parent changed before durability recovery".into(),
                    });
                }
                let retained = if record.conflict == ConflictAction::Overwrite {
                    let retained_path = retained_displaced.as_ref().ok_or_else(|| {
                        PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: "Windows cross-volume retained destination is missing during durability recovery".into(),
                        }
                    })?;
                    let retained =
                        move_recovery::open_reparse_safe_cross_volume_candidate(retained_path)
                            .map_err(|error| PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume retained destination could not be locked for durability recovery: {error}"
                                ),
                            })?;
                    if move_recovery::stable_identity_from_handle(&retained)
                        .map_err(|error| PlatformError::Io(error.to_string()))?
                        != record.destination_identity.ok_or_else(|| {
                            PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: "Windows cross-volume retained destination identity is missing during durability recovery".into(),
                            }
                        })?
                    {
                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: "Windows cross-volume retained destination changed before durability recovery".into(),
                        });
                    }
                    Some(retained)
                } else {
                    None
                };
                let source_kind = record.source_kind;
                let expected_source = record.source_identity;
                flush_windows_cross_volume_recovered_publication_with(
                    || {
                        let flushed = if source_kind == WindowsCrossVolumeSourceKind::File {
                            copy_windows_move_source_metadata(
                                &source,
                                &mut source_handle,
                                expected_source,
                                &published,
                            )
                        } else {
                            published
                                .sync_all()
                                .map_err(|error| PlatformError::Io(error.to_string()))
                        };
                        flushed.map_err(|error| {
                            PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume publication durability recovery failed: {error}"
                                ),
                            }
                        })
                    },
                    || {
                        retained.as_ref().map_or(Ok(()), |retained| {
                            retained.sync_all().map_err(|error| {
                                PlatformError::CommittedCleanupPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows cross-volume retained destination durability recovery failed: {error}"
                                    ),
                                }
                            })
                        })
                    },
                    || {
                        destination_handle.sync_all().map_err(|error| {
                            PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume publication namespace durability recovery failed: {error}"
                                ),
                            }
                        })
                    },
                    || {
                        record.phase = WindowsCrossVolumePhase::Published;
                        persist_windows_cross_volume_record(recovery_root, record).map_err(|error| {
                            PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume recovered published state was not durable: {error}"
                                ),
                            }
                        })
                    },
                )?;
            }
            WindowsCrossVolumeRestartAction::RemoveSource => {
                #[cfg(test)]
                windows_move_boundary(WindowsMoveBoundary::BeforeSourceCleanup, &target);
                let cleanup_error = |message: String| PlatformError::CommittedCleanupPending {
                    transaction: transaction.display().to_string(),
                    message,
                };
                let mut published = move_recovery::open_reparse_safe_cross_volume_verification(
                    &target,
                )
                .map_err(|error| {
                    cleanup_error(format!(
                        "published entry lock failed before source cleanup: {error}"
                    ))
                })?;
                if Some(
                    move_recovery::stable_identity_from_handle(&published)
                        .map_err(|error| cleanup_error(error.to_string()))?,
                ) != record.candidate_identity
                {
                    return Err(cleanup_error(
                        "published entry identity changed; source was preserved".into(),
                    ));
                }
                let mut published_snapshot = None;
                let published_evidence = match record.source_kind {
                    WindowsCrossVolumeSourceKind::File => {
                        if !windows_stream_set_is_copy_safe(
                            windows_file_streams(&published)
                                .map_err(|error| cleanup_error(error.to_string()))?,
                            false,
                        ) {
                            return Err(cleanup_error(
                                "published file streams changed; source was preserved".into(),
                            ));
                        }
                        hash_windows_file(&mut published)
                    }
                    WindowsCrossVolumeSourceKind::Directory => {
                        capture_windows_directory_snapshot(&target, &published).map(|snapshot| {
                            let evidence = snapshot.evidence.clone();
                            published_snapshot = Some(snapshot);
                            evidence
                        })
                    }
                    WindowsCrossVolumeSourceKind::SymbolicLink => {
                        let expected_target =
                            record.source_link_target.as_deref().ok_or_else(|| {
                                cleanup_error(
                                    "published link evidence is missing its target".into(),
                                )
                            })?;
                        let expected_is_directory =
                            record.source_link_is_directory.ok_or_else(|| {
                                cleanup_error("published link evidence is missing its kind".into())
                            })?;
                        verify_windows_symbolic_link(
                            &target,
                            expected_target,
                            expected_is_directory,
                        )
                    }
                }
                .map_err(|error| {
                    cleanup_error(format!(
                        "published entry verification failed before source cleanup: {error}"
                    ))
                })?;
                if published_evidence != (record.source_size, record.source_sha256.clone()) {
                    return Err(cleanup_error(
                        "published entry contents changed; source was preserved".into(),
                    ));
                }
                // Keep the verified destination locked through all destructive source work.
                let _published_guards = (published, published_snapshot);
                if record.source_kind == WindowsCrossVolumeSourceKind::Directory {
                    let source_now = observe_windows_identity(&source)?;
                    let claimed_now = observe_windows_identity(&claimed_directory_source)?;
                    let topology =
                        if claimed_now.is_some() && claimed_now != Some(record.source_identity) {
                            WindowsDirectoryCleanupTopology::RetainAmbiguous
                        } else {
                            classify_windows_directory_cleanup_topology(
                                source_now == Some(record.source_identity),
                                claimed_now == Some(record.source_identity),
                            )
                        };
                    match topology {
                        WindowsDirectoryCleanupTopology::ClaimSource => {
                            if source_handle.is_none() {
                                source_handle = Some(
                                    move_recovery::open_reparse_safe_cross_volume_source(&source)
                                        .map_err(|error| PlatformError::CommittedCleanupPending {
                                        transaction: transaction.display().to_string(),
                                        message: format!(
                                            "Windows directory source claim reopen failed: {error}"
                                        ),
                                    })?,
                                );
                            }
                            let retained_source = source_handle
                                .as_ref()
                                .expect("directory source handle retained for claim");
                            if move_recovery::stable_identity_from_handle(retained_source)
                                .map_err(|error| PlatformError::CommittedCleanupPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows directory source claim identity probe failed: {error}"
                                    ),
                                })?
                                != record.source_identity
                                || !windows_stream_set_is_copy_safe(windows_file_streams(retained_source)?, true)
                            {
                                return Err(PlatformError::CommittedCleanupPending {
                                    transaction: transaction.display().to_string(),
                                    message: "Windows directory source changed before private claim"
                                        .into(),
                                });
                            }
                            let source_snapshot =
                                capture_windows_directory_snapshot(&source, retained_source)
                                    .map_err(|error| PlatformError::CommittedCleanupPending {
                                        transaction: transaction.display().to_string(),
                                        message: format!(
                                            "Windows directory source claim scan failed: {error}"
                                        ),
                                    })?;
                            if source_snapshot.evidence
                                != (record.source_size, record.source_sha256.clone())
                                || source_snapshot.journal_entries()? != record.source_manifest
                            {
                                return Err(PlatformError::CommittedCleanupPending {
                                    transaction: transaction.display().to_string(),
                                    message:
                                        "Windows directory source changed before private claim"
                                            .into(),
                                });
                            }
                            let source_parent_handle =
                                move_recovery::open_reparse_safe_directory_for_rename(source_parent)
                                    .map_err(|error| {
                                        PlatformError::CommittedCleanupPending {
                                            transaction: transaction.display().to_string(),
                                            message: format!(
                                                "Windows directory source parent claim open failed: {error}"
                                            ),
                                        }
                                    })?;
                            if move_recovery::stable_identity_from_handle(&source_parent_handle)
                                .map_err(|error| PlatformError::CommittedCleanupPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows directory source parent claim probe failed: {error}"
                                    ),
                                })?
                                != record.source_parent_identity
                            {
                                return Err(PlatformError::CommittedCleanupPending {
                                    transaction: transaction.display().to_string(),
                                    message:
                                        "Windows directory source parent changed before private claim"
                                            .into(),
                                });
                            }
                            // The claimed tree is re-opened and checked against the durable
                            // identity/content manifest before removing any entry.
                            drop(source_snapshot);
                            #[cfg(test)]
                            windows_move_boundary(WindowsMoveBoundary::SourceUnlocked, &source);
                            rename_windows_handle_no_replace(
                                retained_source,
                                &source_parent_handle,
                                claimed_directory_source.file_name().ok_or_else(|| {
                                    PlatformError::InvalidSource(
                                        claimed_directory_source.display().to_string(),
                                    )
                                })?,
                            )
                            .map_err(|error| {
                                PlatformError::CommittedCleanupPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows directory source private claim failed: {error}"
                                    ),
                                }
                            })?;
                            source_parent_handle.sync_all().map_err(|error| {
                                PlatformError::CommittedCleanupPending {
                                    transaction: transaction.display().to_string(),
                                    message: format!(
                                        "Windows directory source claim namespace flush failed: {error}"
                                    ),
                                }
                            })?;
                            #[cfg(test)]
                            windows_move_boundary(
                                WindowsMoveBoundary::SourceClaimed,
                                &claimed_directory_source,
                            );
                            continue;
                        }
                        WindowsDirectoryCleanupTopology::DrainClaimedSource => {
                            let claimed_handle = match source_handle.take() {
                                Some(handle)
                                    if move_recovery::stable_identity_from_handle(&handle)
                                        .map_err(|error| {
                                            PlatformError::CommittedCleanupPending {
                                                transaction: transaction.display().to_string(),
                                                message: format!(
                                                    "retained claimed source handle probe failed: {error}"
                                                ),
                                            }
                                        })?
                                        == record.source_identity =>
                                {
                                    handle
                                }
                                Some(_) | None => {
                                    move_recovery::open_reparse_safe_cross_volume_source(
                                        &claimed_directory_source,
                                    )
                                    .map_err(|error| {
                                        PlatformError::CommittedCleanupPending {
                                            transaction: transaction.display().to_string(),
                                            message: format!(
                                                "Windows claimed directory source reopen failed: {error}"
                                            ),
                                        }
                                    })?
                                }
                            };
                            drain_windows_claimed_source_directory(
                                &claimed_directory_source,
                                claimed_handle,
                                record,
                                &transaction,
                            )?;
                            continue;
                        }
                        WindowsDirectoryCleanupTopology::Complete => continue,
                        WindowsDirectoryCleanupTopology::RetainAmbiguous => {
                            return Err(PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: "Windows directory source cleanup topology is ambiguous; source and recovery evidence were preserved".into(),
                            });
                        }
                    }
                }
                if source_handle.is_none() {
                    source_handle = Some(
                        move_recovery::open_reparse_safe_cross_volume_source(&source).map_err(
                            |error| PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume source cleanup reopen failed: {error}"
                                ),
                            },
                        )?,
                    );
                }
                let removal_handle = source_handle
                    .as_ref()
                    .expect("source handle is retained after successful reopen")
                    .try_clone()
                    .map_err(|error| PlatformError::CommittedCleanupPending {
                        transaction: transaction.display().to_string(),
                        message: format!(
                            "Windows cross-volume source cleanup clone failed: {error}"
                        ),
                    })?;
                remove_windows_source_by_handle_if_still_named(
                    &source,
                    &removal_handle,
                    record,
                    &transaction,
                )?;
                drop(removal_handle);
                drop(source_handle.take());
            }
            WindowsCrossVolumeRestartAction::Complete => {
                let source_parent_handle =
                    move_recovery::open_reparse_safe_directory_for_rename(source_parent).map_err(
                        |error| PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: format!(
                                "Windows cross-volume source parent could not be locked for removal durability: {error}"
                            ),
                        },
                    )?;
                if move_recovery::stable_identity_from_handle(&source_parent_handle)
                    .map_err(|error| PlatformError::Io(error.to_string()))?
                    != record.source_parent_identity
                {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: transaction.display().to_string(),
                        message: "Windows cross-volume source parent changed before removal durability flush".into(),
                    });
                }
                complete_windows_cross_volume_source_removal_with(
                    || {
                        source_parent_handle.sync_all().map_err(|error| {
                            PlatformError::CommittedCleanupPending {
                                transaction: transaction.display().to_string(),
                                message: format!(
                                    "Windows cross-volume source removal namespace flush failed: {error}"
                                ),
                            }
                        })
                    },
                    || Ok(()),
                )?;
                if record.conflict == ConflictAction::Overwrite {
                    record.phase = WindowsCrossVolumePhase::SourceRemoved;
                    persist_windows_cross_volume_record(recovery_root, record).map_err(|error| {
                        PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: format!("Windows retained Move source removal could not be recorded: {error}"),
                        }
                    })?;
                    return completed_windows_cross_volume_overwrite(recovery_root, record)?
                        .ok_or_else(|| PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: "Windows retained Move completion record is missing".into(),
                        });
                }
                record.phase = WindowsCrossVolumePhase::SourceRemoved;
                // The terminal central receipt survives until teardown is durable.
                // The local Published evidence stays unchanged until its removal.
                write_windows_record_file(
                    &windows_cross_volume_journal_path(recovery_root, record.transaction_id()?),
                    record,
                    false,
                )
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: transaction.display().to_string(),
                    message: format!(
                        "Windows completed Move evidence could not be persisted: {error}"
                    ),
                })?;
                #[cfg(test)]
                windows_move_boundary(WindowsMoveBoundary::TeardownPrepared, &transaction);
                return finish_windows_cross_volume_teardown(recovery_root, record);
            }
            WindowsCrossVolumeRestartAction::RetainAmbiguous => {
                return Err(PlatformError::ClaimedRecoveryPending {
                    transaction: transaction.display().to_string(),
                    message: "Windows cross-volume topology is ambiguous; source and evidence were preserved".into(),
                });
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn start_windows_same_volume_overwrite(
    source: &Path,
    destination_dir: &Path,
    target: &Path,
    source_handle: std::fs::File,
) -> Result<MovePublication, PlatformError> {
    let source_parent = source
        .parent()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    let source_parent_handle = move_recovery::open_reparse_safe_directory_for_rename(source_parent)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_parent_identity = move_recovery::stable_identity_from_handle(&source_parent_handle)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_identity = move_recovery::stable_identity_from_handle(&source_handle)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let destination_parent_handle =
        move_recovery::open_reparse_safe_directory_for_rename(destination_dir)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
    let destination_parent_identity =
        move_recovery::stable_identity_from_handle(&destination_parent_handle)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
    let destination_identity = move_recovery::stable_identity_of_entry(target)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_kind = windows_same_volume_kind_from_identity(source_identity);
    let destination_kind = windows_same_volume_kind_from_identity(destination_identity);
    if source_kind.is_none()
        || destination_kind.is_none()
        || !source_parent_identity.is_directory_without_reparse()
        || !destination_parent_identity.is_directory_without_reparse()
    {
        return Err(PlatformError::UnsupportedSafeMove(
            source.display().to_string(),
        ));
    }
    if source_identity == destination_identity {
        return Err(PlatformError::UnsupportedSafeMove(
            source.display().to_string(),
        ));
    }
    if source_identity.volume_serial_number() != destination_identity.volume_serial_number() {
        return Err(PlatformError::UnsupportedSafeCrossDeviceMove(
            source.display().to_string(),
        ));
    }

    let recovery_root = windows_recovery_root()?;
    std::fs::create_dir_all(&recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let (transaction_id, transaction_path) = loop {
        let mut transaction_id = [0_u8; 16];
        getrandom::fill(&mut transaction_id)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        let candidate = destination_dir.join(format!(
            ".ottrin-move-{}",
            encode_transaction_id(transaction_id)
        ));
        match std::fs::create_dir(&candidate) {
            Ok(()) => break (transaction_id, candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(PlatformError::Io(error.to_string())),
        }
    };
    if let Err(error) = restrict_windows_transaction_directory(&transaction_path) {
        let _ = std::fs::remove_dir(&transaction_path);
        return Err(error);
    }
    let transaction_handle =
        move_recovery::open_reparse_safe_directory_for_rename(&transaction_path)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
    let transaction_directory_identity =
        move_recovery::stable_identity_from_handle(&transaction_handle)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
    if !transaction_directory_identity.is_directory_without_reparse() {
        drop(transaction_handle);
        let _ = std::fs::remove_dir(&transaction_path);
        return Err(PlatformError::UnsupportedSafeMove(
            source.display().to_string(),
        ));
    }
    let replacement = transaction_path.join(WINDOWS_REPLACEMENT_NAME);
    let retained_displaced = transaction_path.join(WINDOWS_RETAINED_NAME);
    let record = WindowsOverwriteRecord {
        schema_major: move_recovery::JOURNAL_SCHEMA_MAJOR,
        schema_minor: move_recovery::JOURNAL_SCHEMA_MINOR,
        transaction_id: encode_transaction_id(transaction_id),
        phase: WindowsOverwritePhase::Prepared,
        source: move_recovery::NativePath::encode(source),
        source_parent_identity,
        source_identity,
        destination_parent: move_recovery::NativePath::encode(destination_dir),
        destination_parent_identity,
        target: move_recovery::NativePath::encode(target),
        destination_identity,
        transaction_directory: move_recovery::NativePath::encode(&transaction_path),
        transaction_directory_identity,
        replacement: move_recovery::NativePath::encode(&replacement),
        retained_displaced: move_recovery::NativePath::encode(&retained_displaced),
        retained_identity: destination_identity,
    };
    persist_windows_overwrite_record(&recovery_root, &record).map_err(|error| {
        PlatformError::Io(format!(
            "durable Windows Overwrite intent failed before source mutation: {error}"
        ))
    })?;
    // The durable record binds the originally opened source identity. Release
    // its selection handle before recovery acquires the claim/publication
    // handles: ReplaceFileW and exclusive retained-rename handles cannot coexist
    // with that open DELETE handle. Recovery revalidates the recorded identity
    // before any claim, so this handoff cannot admit a pathname replacement.
    drop(source_handle);
    resume_windows_overwrite(&recovery_root, record)
}

#[cfg(target_os = "windows")]
fn resume_windows_overwrite(
    recovery_root: &Path,
    mut record: WindowsOverwriteRecord,
) -> Result<MovePublication, PlatformError> {
    let transaction = record.transaction_path()?;
    let local_record = load_windows_overwrite_record(&transaction.join(WINDOWS_LOCAL_JOURNAL_NAME))
        .map_err(|error| PlatformError::ClaimedRecoveryPending {
            transaction: transaction.display().to_string(),
            message: format!("Windows Overwrite local journal could not be validated: {error}"),
        })?;
    if !local_record.same_transaction_evidence(&record)
        || record.retained_identity != record.destination_identity
    {
        return Err(PlatformError::ClaimedRecoveryPending {
            transaction: transaction.display().to_string(),
            message: "Windows Overwrite durable journals or retained identity disagree".to_string(),
        });
    }
    let source = record.source_path()?;
    let source_parent = source
        .parent()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    let destination_parent = record.destination_parent_path()?;
    let target = record.target_path()?;
    let replacement = record.replacement_path()?;
    let retained = record.retained_path()?;
    for (path, expected) in [
        (source_parent, record.source_parent_identity),
        (
            destination_parent.as_path(),
            record.destination_parent_identity,
        ),
        (transaction.as_path(), record.transaction_directory_identity),
    ] {
        if observe_windows_identity(path)? != Some(expected) {
            return Err(PlatformError::ClaimedRecoveryPending {
                transaction: transaction.display().to_string(),
                message: format!(
                    "Windows Overwrite parent identity changed: {}",
                    path.display()
                ),
            });
        }
    }

    loop {
        let action = classify_windows_overwrite_restart(
            observe_windows_identity(&source)?,
            observe_windows_identity(&target)?,
            observe_windows_identity(&replacement)?,
            observe_windows_identity(&retained)?,
            record.source_identity,
            record.destination_identity,
        );
        match action {
            WindowsOverwriteRestartAction::ClaimSource => {
                let source_handle = move_recovery::open_reparse_safe_entry_for_rename(&source)
                    .map_err(|error| PlatformError::Io(error.to_string()))?;
                if move_recovery::stable_identity_from_handle(&source_handle)
                    .map_err(|error| PlatformError::Io(error.to_string()))?
                    != record.source_identity
                {
                    return Err(PlatformError::ClaimedRecoveryPending {
                        transaction: transaction.display().to_string(),
                        message: "Windows Overwrite source changed before private claim"
                            .to_string(),
                    });
                }
                let transaction_handle =
                    move_recovery::open_reparse_safe_directory_for_rename(&transaction)
                        .map_err(|error| PlatformError::Io(error.to_string()))?;
                rename_windows_handle_no_replace(
                    &source_handle,
                    &transaction_handle,
                    std::ffi::OsStr::new(WINDOWS_REPLACEMENT_NAME),
                )
                .map_err(|error| PlatformError::ClaimedRecoveryPending {
                    transaction: transaction.display().to_string(),
                    message: format!("Windows Overwrite source claim failed: {error}"),
                })?;
                if observe_windows_identity(&replacement)? != Some(record.source_identity) {
                    return Err(PlatformError::ClaimedRecoveryPending {
                        transaction: transaction.display().to_string(),
                        message: "Windows Overwrite private claim identity is ambiguous"
                            .to_string(),
                    });
                }
                record.phase = WindowsOverwritePhase::SourceClaimed;
                persist_windows_overwrite_record(recovery_root, &record).map_err(|error| {
                    PlatformError::ClaimedRecoveryPending {
                        transaction: transaction.display().to_string(),
                        message: format!(
                            "Windows Overwrite claimed state was not durable: {error}"
                        ),
                    }
                })?;
            }
            WindowsOverwriteRestartAction::PublishClaimed => {
                let source_kind = windows_same_volume_kind_from_identity(record.source_identity)
                    .ok_or_else(|| PlatformError::ClaimedRecoveryPending {
                        transaction: transaction.display().to_string(),
                        message: "Windows Overwrite source kind is unsupported".to_string(),
                    })?;
                let displaced_kind =
                    windows_same_volume_kind_from_identity(record.destination_identity)
                        .ok_or_else(|| PlatformError::ClaimedRecoveryPending {
                            transaction: transaction.display().to_string(),
                            message: "Windows Overwrite displaced kind is unsupported".to_string(),
                        })?;
                match windows_same_volume_overwrite_publication(source_kind, displaced_kind) {
                    WindowsSameVolumeOverwritePublication::ReplaceFile => {
                        replace_windows_file_retaining_displaced(
                            &target,
                            &replacement,
                            &retained,
                            &transaction,
                            record.source_identity,
                            record.destination_identity,
                        )?;
                    }
                    WindowsSameVolumeOverwritePublication::RetainedRename => {
                        publish_windows_entry_retaining_displaced_by_rename(
                            &target,
                            &replacement,
                            &retained,
                            &transaction,
                            record.source_identity,
                            record.destination_identity,
                        )?;
                    }
                }
                record.phase = WindowsOverwritePhase::Published;
                persist_windows_overwrite_record(recovery_root, &record).map_err(|error| {
                    PlatformError::CommittedCleanupPending {
                        transaction: transaction.display().to_string(),
                        message: format!(
                            "Windows Overwrite publication state was not durable: {error}"
                        ),
                    }
                })?;
            }
            WindowsOverwriteRestartAction::PublishAfterRetain => {
                let publication = windows_same_volume_kind_from_identity(record.source_identity)
                    .zip(windows_same_volume_kind_from_identity(
                        record.destination_identity,
                    ))
                    .map(|(source, displaced)| {
                        windows_same_volume_overwrite_publication(source, displaced)
                    });
                if publication != Some(WindowsSameVolumeOverwritePublication::RetainedRename) {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: transaction.display().to_string(),
                        message: "Windows Overwrite retained topology has unsupported entry kinds"
                            .to_string(),
                    });
                }
                publish_windows_entry_retaining_displaced_by_rename(
                    &target,
                    &replacement,
                    &retained,
                    &transaction,
                    record.source_identity,
                    record.destination_identity,
                )?;
                record.phase = WindowsOverwritePhase::Published;
                persist_windows_overwrite_record(recovery_root, &record).map_err(|error| {
                    PlatformError::CommittedCleanupPending {
                        transaction: transaction.display().to_string(),
                        message: format!(
                            "Windows Overwrite resumed publication state was not durable: {error}"
                        ),
                    }
                })?;
            }
            WindowsOverwriteRestartAction::FinishPublished => {
                if record.phase != WindowsOverwritePhase::Published {
                    record.phase = WindowsOverwritePhase::Published;
                    persist_windows_overwrite_record(recovery_root, &record).map_err(|error| {
                        PlatformError::CommittedCleanupPending {
                            transaction: transaction.display().to_string(),
                            message: format!(
                                "Windows Overwrite restart topology was proven but not persisted: {error}"
                            ),
                        }
                    })?;
                }
                return Ok(MovePublication {
                    #[cfg(target_os = "windows")]
                    windows_undo: None,
                    disposition: MoveDisposition::Moved,
                    published_path: Some(target),
                    retained_displaced_path: Some(retained),
                });
            }
            WindowsOverwriteRestartAction::RetainAmbiguous => {
                return Err(PlatformError::CommittedCleanupPending {
                    transaction: transaction.display().to_string(),
                    message:
                        "Windows Overwrite restart topology is ambiguous; all entries were retained"
                            .to_string(),
                });
            }
        }
    }
}

/// Atomically publishes one already-private regular-file replacement while retaining the
/// displaced destination at another already-private path on the same volume.
///
/// The caller owns transaction creation, durable intent, and later cleanup. Keeping this
/// primitive separate lets Windows Overwrite remain fail-closed until those existing recovery
/// seams have Windows implementations. `ReplaceFileW` is pathname-based, so the caller must have
/// identity-verified both private names before entry; observed topology, not the API return bit,
/// decides whether publication completed.
#[cfg(target_os = "windows")]
fn replace_windows_file_retaining_displaced(
    target: &Path,
    replacement: &Path,
    retained_displaced: &Path,
    transaction: &Path,
    expected_source: move_recovery::WindowsStableIdentity,
    expected_displaced: move_recovery::WindowsStableIdentity,
) -> Result<(), PlatformError> {
    for path in [target, replacement] {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(PlatformError::UnsupportedSafeMove(
                path.display().to_string(),
            ));
        }
    }
    match std::fs::symlink_metadata(retained_displaced) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => {
            return Err(PlatformError::DestinationExists(
                retained_displaced.display().to_string(),
            ));
        }
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    }

    if observe_windows_identity(replacement)? != Some(expected_source)
        || observe_windows_identity(target)? != Some(expected_displaced)
    {
        return Err(PlatformError::ClaimedRecoveryPending {
            transaction: transaction.display().to_string(),
            message: "Windows Overwrite inputs changed before retained replacement".to_string(),
        });
    }

    let target_wide = windows_wide_path(target)?;
    let replacement_wide = windows_wide_path(replacement)?;
    let retained_wide = windows_wide_path(retained_displaced)?;
    let replaced = unsafe {
        ReplaceFileW(
            target_wide.as_ptr(),
            replacement_wide.as_ptr(),
            retained_wide.as_ptr(),
            0,
            ptr::null(),
            ptr::null(),
        )
    };
    let replace_error = (replaced == 0).then(std::io::Error::last_os_error);
    let observe_after_replace = |path: &Path| {
        observe_windows_identity(path).map_err(|error| PlatformError::CommittedCleanupPending {
            transaction: transaction.display().to_string(),
            message: format!(
                "Windows retained replacement topology could not be observed after the syscall: {error}"
            ),
        })
    };
    let observed = classify_windows_replace_topology(
        observe_after_replace(target)?,
        observe_after_replace(replacement)?,
        observe_after_replace(retained_displaced)?,
        expected_source,
        expected_displaced,
    );
    match observed {
        WindowsReplaceTopology::PublishedAndRetained => Ok(()),
        WindowsReplaceTopology::NotPublished => {
            let message = replace_error.map_or_else(
                || "Windows retained replacement reported success without publication".to_string(),
                |error| format!("Windows retained replacement was not published: {error}"),
            );
            Err(PlatformError::ClaimedRecoveryPending {
                transaction: transaction.display().to_string(),
                message,
            })
        }
        WindowsReplaceTopology::PublishedAmbiguous | WindowsReplaceTopology::Ambiguous => {
            Err(PlatformError::CommittedCleanupPending {
                transaction: transaction.display().to_string(),
                message: replace_error.map_or_else(
                    || "Windows retained replacement produced ambiguous topology".to_string(),
                    |error| {
                        format!(
                            "Windows retained replacement failed with ambiguous topology: {error}"
                        )
                    },
                ),
            })
        }
    }
}

#[cfg(target_os = "windows")]
fn publish_windows_entry_retaining_displaced_by_rename(
    target: &Path,
    replacement: &Path,
    retained: &Path,
    transaction: &Path,
    expected_source: move_recovery::WindowsStableIdentity,
    expected_displaced: move_recovery::WindowsStableIdentity,
) -> Result<(), PlatformError> {
    let target_parent = target
        .parent()
        .ok_or_else(|| PlatformError::CommittedCleanupPending {
            transaction: transaction.display().to_string(),
            message: "Windows Overwrite target has no parent".to_string(),
        })?;
    let target_name = target
        .file_name()
        .ok_or_else(|| PlatformError::CommittedCleanupPending {
            transaction: transaction.display().to_string(),
            message: "Windows Overwrite target has no file name".to_string(),
        })?;
    let retained_name =
        retained
            .file_name()
            .ok_or_else(|| PlatformError::CommittedCleanupPending {
                transaction: transaction.display().to_string(),
                message: "Windows Overwrite retained entry has no file name".to_string(),
            })?;
    let target_parent_handle = move_recovery::open_reparse_safe_directory_for_rename(target_parent)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let transaction_handle = move_recovery::open_reparse_safe_directory_for_rename(transaction)
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    match (
        observe_windows_identity(target)?,
        observe_windows_identity(retained)?,
    ) {
        (Some(target_identity), None) if target_identity == expected_displaced => {
            let displaced_handle = move_recovery::open_reparse_safe_cross_volume_candidate(target)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            if move_recovery::stable_identity_from_handle(&displaced_handle)
                .map_err(|error| PlatformError::Io(error.to_string()))?
                != expected_displaced
            {
                return Err(PlatformError::CommittedCleanupPending {
                    transaction: transaction.display().to_string(),
                    message: "Windows Overwrite displaced entry changed before retention"
                        .to_string(),
                });
            }
            rename_windows_handle_no_replace(&displaced_handle, &transaction_handle, retained_name)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            displaced_handle
                .sync_all()
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            target_parent_handle
                .sync_all()
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            transaction_handle
                .sync_all()
                .map_err(|error| PlatformError::Io(error.to_string()))?;
        }
        (None, Some(retained_identity)) if retained_identity == expected_displaced => {}
        _ => {
            return Err(PlatformError::CommittedCleanupPending {
                transaction: transaction.display().to_string(),
                message: "Windows Overwrite retained-rename topology changed before publication"
                    .to_string(),
            });
        }
    }

    let replacement_handle = move_recovery::open_reparse_safe_cross_volume_candidate(replacement)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    if move_recovery::stable_identity_from_handle(&replacement_handle)
        .map_err(|error| PlatformError::Io(error.to_string()))?
        != expected_source
    {
        return Err(PlatformError::CommittedCleanupPending {
            transaction: transaction.display().to_string(),
            message: "Windows Overwrite claimed source changed before publication".to_string(),
        });
    }
    rename_windows_handle_no_replace(&replacement_handle, &target_parent_handle, target_name)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    replacement_handle
        .sync_all()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    transaction_handle
        .sync_all()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    target_parent_handle
        .sync_all()
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    // Publication and namespace flushes are complete. Close our exclusive
    // writer before reopening the published name for the topology check.
    drop(replacement_handle);
    if observe_windows_identity(target)? != Some(expected_source)
        || observe_windows_identity(replacement)?.is_some()
        || observe_windows_identity(retained)? != Some(expected_displaced)
    {
        return Err(PlatformError::CommittedCleanupPending {
            transaction: transaction.display().to_string(),
            message: "Windows Overwrite retained-rename publication topology is ambiguous"
                .to_string(),
        });
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(super) fn move_entry_transactional(
    source: &Path,
    destination_dir: &Path,
    _conflict: ConflictAction,
) -> Result<MovePublication, PlatformError> {
    std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
    reject_directory_copy_into_itself(source, destination_dir)?;
    Err(PlatformError::UnsupportedSafeMove(
        source.display().to_string(),
    ))
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TestPoint {
    Prepare,
    Claim,
    ClassifyCrossDevice,
    PendingDestination,
    DestinationPublished,
    Publication,
    DestinationParentSynced,
    BeforeSourceCleanup,
    AfterSourceRevalidation,
    SourceCleanupQuarantined,
}

#[cfg(target_os = "linux")]
fn move_with_hooks(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
    mut hook: impl FnMut(TestPoint) -> Result<(), &'static str>,
) -> Result<MovePublication, PlatformError> {
    #[cfg(test)]
    let _test_move_guard = TEST_MOVE_SERIALIZER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let source_name = source
        .file_name()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;

    let source_parent_path = source
        .parent()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    let source_parent_path = std::fs::canonicalize(source_parent_path)
        .map_err(|_| PlatformError::InvalidSource(source_parent_path.display().to_string()))?;
    let destination_dir = std::fs::canonicalize(destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;
    let source_parent = move_recovery::LinuxSourceParentDirectory::open(&source_parent_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_entry_metadata = std::fs::symlink_metadata(source).ok();
    let source_entry_identity = source_entry_metadata
        .as_ref()
        .map(stable_identity_from_metadata);
    let source_parent_metadata = std::fs::symlink_metadata(&source_parent_path)
        .map_err(|_| PlatformError::InvalidSource(source_parent_path.display().to_string()))?;
    let source_parent_identity = move_recovery::LinuxStableIdentity::new(
        source_parent_metadata.dev(),
        source_parent_metadata.ino(),
        if source_parent_metadata.is_dir() {
            move_recovery::LinuxFileType::Directory
        } else {
            move_recovery::LinuxFileType::Other
        },
    );

    let destination_parent_metadata = std::fs::symlink_metadata(&destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;
    let is_cross_device = match hook(TestPoint::ClassifyCrossDevice) {
        Ok(()) => source_parent_metadata.dev() != destination_parent_metadata.dev(),
        Err("force cross-device move") => true,
        Err(message) => {
            return Err(PlatformError::Io(format!(
                "move test hook failed: {message}"
            )));
        }
    };

    let destination_parent = move_recovery::LinuxSourceParentDirectory::open(&destination_dir)
        .map_err(|_| PlatformError::InvalidDestination(destination_dir.display().to_string()))?;

    if !is_cross_device
        && (conflict == ConflictAction::Skip || conflict == ConflictAction::Rename)
        && let Some(disposition) = recover_same_filesystem_pending_destination(
            source,
            &destination_dir,
            source_name,
            &source_parent,
            &source_parent_path,
            source_parent_identity,
            source_entry_identity,
            &destination_parent,
            conflict,
        )?
    {
        return Ok(MovePublication {
            #[cfg(target_os = "windows")]
            windows_undo: None,
            disposition,
            published_path: None,
            retained_displaced_path: None,
            published_content: None,
        });
    }
    if is_cross_device
        && (conflict == ConflictAction::Skip || conflict == ConflictAction::Rename)
        && let Some(publication) = recover_cross_filesystem_pending_destination(
            source,
            &destination_dir,
            source_name,
            &source_parent,
            &source_parent_path,
            source_parent_identity,
            source_entry_identity,
            &destination_parent,
            conflict,
            &mut hook,
        )?
    {
        return Ok(publication);
    }

    reject_directory_copy_into_itself(source, &destination_dir)?;

    let source_metadata = source_entry_metadata
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    if !is_cross_device
        && destination_dir.join(source_name).exists()
        && conflict == ConflictAction::Skip
    {
        return Ok(MovePublication {
            #[cfg(target_os = "windows")]
            windows_undo: None,
            disposition: MoveDisposition::Skipped,
            published_path: None,
            retained_displaced_path: None,
            published_content: None,
        });
    }
    let source_entry_identity = move_recovery::LinuxStableIdentity::new(
        source_metadata.dev(),
        source_metadata.ino(),
        if source_metadata.is_file() {
            move_recovery::LinuxFileType::Regular
        } else if source_metadata.is_dir() {
            move_recovery::LinuxFileType::Directory
        } else if source_metadata.file_type().is_symlink() {
            move_recovery::LinuxFileType::Symlink
        } else {
            move_recovery::LinuxFileType::Other
        },
    );

    let recovery_root = default_recovery_root()?;
    std::fs::create_dir_all(&recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut recovery_transaction_id = [0_u8; 16];
    getrandom::fill(&mut recovery_transaction_id)
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    let local_transaction = source_parent
        .create_transaction(recovery_transaction_id)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_transaction_identity = local_transaction.observed_identity();
    let local_path = source_parent_path.join(local_transaction.name());
    let local = source_parent
        .open_created_transaction(&local_transaction)
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    let prepared = move_recovery::PreparedRecord::new_linux(
        recovery_transaction_id,
        &source_parent_path,
        Path::new(source_name),
        source_parent_identity,
        source_entry_identity,
        local_transaction.name(),
        source_transaction_identity,
    )
    .map_err(|error| PlatformError::Io(error.to_string()))?;

    let central = move_recovery::LinuxJournalDirectory::open_configured(&recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut recovery = central
        .try_lock_recovery()
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    recovery
        .prepare_with_local(&local, &prepared)
        .map_err(|error| {
            PlatformError::Io(format!(
                "prepare durable state failed before source claim: {error}"
            ))
        })?;
    hook(TestPoint::Prepare)
        .map_err(|message| PlatformError::Io(format!("move test hook failed: {message}")))?;

    recovery
        .advance_to_claim_intent_with_local(&local, &prepared)
        .map_err(|error| {
            PlatformError::Io(format!(
                "claim-intent durable state failed before source claim: {error}"
            ))
        })?;

    let _ = recovery
        .claim_source_after_intent(&source_parent, &local, recovery_transaction_id)
        .map_err(|error| PlatformError::ClaimedRecoveryPending {
            transaction: local_path.display().to_string(),
            message: format!("source claim failed: {error}"),
        })?;

    recovery
        .advance_to_claimed_with_local(&local, recovery_transaction_id)
        .map_err(|error| PlatformError::ClaimedRecoveryPending {
            transaction: local_path.display().to_string(),
            message: format!("claimed durable state failed: {error}"),
        })?;
    hook(TestPoint::Claim).map_err(|message| PlatformError::ClaimedRecoveryPending {
        transaction: local_path.display().to_string(),
        message: format!("move test hook failed before publication: {message}"),
    })?;
    let claimed_path = local_path.join("claimed");
    let mut destination_name = source_name.to_os_string();
    let mut destination_target = destination_dir.join(&destination_name);

    if is_cross_device {
        let destination_transaction = destination_parent
            .create_transaction(recovery_transaction_id)
            .map_err(|error| PlatformError::ClaimedRecoveryPending {
                transaction: local_path.display().to_string(),
                message: format!("destination transaction creation failed: {error}"),
            })?;
        let destination_transaction_identity = destination_transaction.observed_identity();
        let destination_transaction_path = destination_dir.join(destination_transaction.name());
        let mut staged_destination_identity = None;
        let mut retained_displaced_identity = None;
        let destination_tree_snapshot = source_tree_snapshot_for_path(&claimed_path)?;

        loop {
            if staged_destination_identity.is_none() {
                let staged_destination_path =
                    destination_transaction_path.join(CROSS_DEVICE_STAGED_NAME);
                copy_entry(&claimed_path, &staged_destination_path).map_err(|error| {
                    PlatformError::ClaimedRecoveryPending {
                        transaction: local_path.display().to_string(),
                        message: format!("cross-device stage copy failed: {error}"),
                    }
                })?;
                flush_tree_bottom_up(&staged_destination_path).map_err(|error| {
                    PlatformError::ClaimedRecoveryPending {
                        transaction: local_path.display().to_string(),
                        message: format!("cross-device stage flush failed: {error}"),
                    }
                })?;
                let staged_metadata = std::fs::symlink_metadata(&staged_destination_path)
                    .map_err(|error| PlatformError::Io(error.to_string()))?;
                staged_destination_identity = Some(stable_identity_from_metadata(&staged_metadata));
            }

            let destination_transaction_path_path = destination_transaction_path.as_path();
            let staged_identity = staged_destination_identity
                .ok_or_else(|| PlatformError::InvalidSource(claimed_path.display().to_string()))?;
            let expected_displaced_destination_identity = if conflict == ConflictAction::Overwrite {
                match std::fs::symlink_metadata(&destination_target) {
                    Ok(metadata) => Some(stable_identity_from_metadata(&metadata)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => {
                        return Err(PlatformError::ClaimedRecoveryPending {
                            transaction: local_path.display().to_string(),
                            message: format!(
                                "cross-device overwrite destination identity probe failed: {error}"
                            ),
                        });
                    }
                }
            } else {
                None
            };
            recovery
                .advance_to_pending_destination_with_local_with_metadata(
                    &local,
                    recovery_transaction_id,
                    &destination_target,
                    move_recovery::PendingDestinationMetadata {
                        destination_transaction_name: Some(
                            destination_transaction.name().to_path_buf(),
                        ),
                        destination_transaction_identity: Some(destination_transaction_identity),
                        source_tree_snapshot: Some(destination_tree_snapshot.clone()),
                        staged_destination_identity: Some(staged_identity),
                        expected_displaced_destination_identity,
                    },
                )
                .map_err(|error| PlatformError::ClaimedRecoveryPending {
                    transaction: local_path.display().to_string(),
                    message: format!("pending destination persistence failed: {error}"),
                })?;
            hook(TestPoint::PendingDestination).map_err(|message| {
                PlatformError::ClaimedRecoveryPending {
                    transaction: local_path.display().to_string(),
                    message: format!("move test hook failed before publication: {message}"),
                }
            })?;
            let staged_destination_path =
                destination_transaction_path.join(CROSS_DEVICE_STAGED_NAME);
            let staged_tree_snapshot = source_tree_snapshot_for_path(&staged_destination_path)
                .map_err(|error| PlatformError::ClaimedRecoveryPending {
                    transaction: local_path.display().to_string(),
                    message: format!(
                        "cross-device staged destination verification failed: {error}"
                    ),
                })?;
            if !destination_tree_snapshot.has_same_paths_and_content(&staged_tree_snapshot) {
                return Err(PlatformError::ClaimedRecoveryPending {
                    transaction: local_path.display().to_string(),
                    message: "cross-device staged destination does not match source snapshot"
                        .to_string(),
                });
            }
            if !verify_source_tree_snapshot(&claimed_path, &destination_tree_snapshot).map_err(
                |error| PlatformError::ClaimedRecoveryPending {
                    transaction: local_path.display().to_string(),
                    message: format!(
                        "cross-device source verification failed before publication: {error}"
                    ),
                },
            )? {
                return Err(PlatformError::ClaimedRecoveryPending {
                    transaction: local_path.display().to_string(),
                    message: "cross-device source snapshot changed before publication".to_string(),
                });
            }
            flush_tree_bottom_up(destination_transaction_path_path).map_err(|error| {
                PlatformError::ClaimedRecoveryPending {
                    transaction: local_path.display().to_string(),
                    message: format!("cross-device transaction flush failed: {error}"),
                }
            })?;
            let destination_transaction_directory = destination_parent
                .open_created_transaction(&destination_transaction)
                .map_err(|error| PlatformError::ClaimedRecoveryPending {
                    transaction: local_path.display().to_string(),
                    message: format!("destination transaction reopen failed: {error}"),
                })?;
            match publish_claimed_to_destination_with_no_replace(
                &destination_transaction_directory,
                &destination_parent,
                std::ffi::OsStr::new(CROSS_DEVICE_STAGED_NAME),
                destination_name.as_os_str(),
            ) {
                Ok(()) => {
                    break;
                }
                Err(publish_error) if publish_error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if conflict == ConflictAction::Skip {
                        restore_claimed_to_source(
                            &source_parent,
                            &local,
                            source_name,
                        )
                        .map_err(|restore_error| PlatformError::ClaimedRecoveryPending {
                            transaction: local_path.display().to_string(),
                            message: format!(
                                "skip-mode publication conflict but claim could not be restored: {restore_error}"
                            ),
                        })?;
                        let _ = cleanup_transaction_journals(
                            &recovery_root,
                            &local_path,
                            &recovery_transaction_id,
                        );
                        let _ = std::fs::remove_dir_all(destination_transaction_path_path);
                        return Ok(MovePublication {
                            #[cfg(target_os = "windows")]
                            windows_undo: None,
                            disposition: MoveDisposition::Skipped,
                            published_path: None,
                            retained_displaced_path: None,
                            published_content: None,
                        });
                    }
                    if conflict == ConflictAction::Overwrite {
                        let Some(expected_displaced_destination_identity) =
                            expected_displaced_destination_identity
                        else {
                            // The target appeared after the absent observation. Persist its exact
                            // identity before attempting the destructive exchange.
                            continue;
                        };
                        publish_claimed_to_destination_with_exchange(
                            &destination_transaction_directory,
                            &destination_parent,
                            std::ffi::OsStr::new(CROSS_DEVICE_STAGED_NAME),
                            destination_name.as_os_str(),
                        )
                        .map_err(|publish_error| {
                            PlatformError::ClaimedRecoveryPending {
                                transaction: local_path.display().to_string(),
                                message: format!("publication failed: {publish_error}"),
                            }
                        })?;
                        retained_displaced_identity = Some(expected_displaced_destination_identity);
                        break;
                    }
                    destination_name = next_available_name(&destination_target)
                        .file_name()
                        .expect("candidate has a file name")
                        .to_os_string();
                    destination_target = destination_dir.join(&destination_name);
                    if conflict == ConflictAction::Rename {
                        continue;
                    }
                    return Err(PlatformError::ClaimedRecoveryPending {
                        transaction: local_path.display().to_string(),
                        message: "publication conflict occurred for unsupported conflict mode"
                            .to_string(),
                    });
                }
                Err(publish_error) => {
                    return Err(PlatformError::ClaimedRecoveryPending {
                        transaction: local_path.display().to_string(),
                        message: format!("publication failed: {publish_error}"),
                    });
                }
            }
        }

        hook(TestPoint::DestinationPublished).map_err(|message| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("move test hook failed after destination publication: {message}"),
            }
        })?;
        destination_parent
            .sync()
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("destination parent flush failed after publication: {error}"),
            })?;
        hook(TestPoint::DestinationParentSynced).map_err(|message| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("move test hook failed after destination-parent sync: {message}"),
            }
        })?;

        hook(TestPoint::Publication).map_err(|message| {
            if conflict == ConflictAction::Overwrite {
                PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!(
                        "move test hook failed after cross-device overwrite publication: {message}"
                    ),
                }
            } else {
                PlatformError::Io(format!("move test hook failed: {message}"))
            }
        })?;

        let destination_identity = staged_destination_identity
            .ok_or_else(|| PlatformError::InvalidSource(claimed_path.display().to_string()))?;
        let destination_metadata =
            std::fs::symlink_metadata(&destination_target).map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("cross-device destination identity check failed: {error}"),
                }
            })?;
        if destination_identity != stable_identity_from_metadata(&destination_metadata) {
            return Err(PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "cross-device destination identity changed before source cleanup"
                    .to_string(),
            });
        }
        if let Some(expected_displaced_identity) = retained_displaced_identity {
            let retained_displaced_path =
                destination_transaction_path.join(CROSS_DEVICE_STAGED_NAME);
            let observed_displaced_identity = stable_identity_from_metadata(
                &std::fs::symlink_metadata(&retained_displaced_path).map_err(|error| {
                    PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!(
                            "cross-device retained displaced destination probe failed: {error}"
                        ),
                    }
                })?,
            );
            if observed_displaced_identity != expected_displaced_identity {
                return Err(PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message:
                        "cross-device displaced destination identity changed during publication"
                            .to_string(),
                });
            }
        }
        if !verify_source_tree_snapshot(&claimed_path, &destination_tree_snapshot)
            .map_err(|error| PlatformError::Io(error.to_string()))?
        {
            return Err(PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "cross-device source snapshot changed before cleanup".to_string(),
            });
        }

        if let Some(observed_displaced_identity) = retained_displaced_identity {
            recovery
                .advance_to_published_with_observed_displaced_identity_with_local(
                    &local,
                    recovery_transaction_id,
                    observed_displaced_identity,
                )
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("overwrite publication durable state failed: {error}"),
                })?;
        } else {
            recovery
                .advance_to_published_with_local(&local, recovery_transaction_id)
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("publication durable state failed: {error}"),
                })?;
        }
        hook(TestPoint::BeforeSourceCleanup).map_err(|message| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("move test hook failed: {message}"),
            }
        })?;

        if !verify_source_tree_snapshot(&claimed_path, &destination_tree_snapshot).map_err(
            |error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!(
                    "source identity revalidation failed at cross-device cleanup boundary: {error}"
                ),
            },
        )? {
            return Err(PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "cross-device source snapshot changed at cleanup boundary".to_string(),
            });
        }

        hook(TestPoint::AfterSourceRevalidation).map_err(|message| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("move test hook failed after source revalidation: {message}"),
            }
        })?;
        let cleanup_path = local_path.join(SOURCE_CLEANUP_NAME);
        local
            .rename_within_no_replace(
                std::ffi::OsStr::new("claimed"),
                std::ffi::OsStr::new(SOURCE_CLEANUP_NAME),
            )
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("cross-device source cleanup quarantine failed: {error}"),
            })?;
        if !verify_source_tree_snapshot(&cleanup_path, &destination_tree_snapshot).map_err(
            |error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("quarantined source identity revalidation failed: {error}"),
            },
        )? {
            let restore = local.rename_within_no_replace(
                std::ffi::OsStr::new(SOURCE_CLEANUP_NAME),
                std::ffi::OsStr::new("claimed"),
            );
            return Err(PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: match restore {
                    Ok(()) => "cross-device source changed during cleanup; replacement restored"
                        .to_string(),
                    Err(error) => format!(
                        "cross-device source changed during cleanup and remains quarantined: {error}"
                    ),
                },
            });
        }

        let cleanup_authority = local
            .claimed_source_cleanup_authority(
                std::ffi::OsStr::new(SOURCE_CLEANUP_NAME),
                source_entry_identity,
            )
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!(
                    "cross-device source cleanup authority could not be retained: {error}"
                ),
            })?;
        hook(TestPoint::SourceCleanupQuarantined).map_err(|message| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!(
                    "move test hook failed after source cleanup quarantine: {message}"
                ),
            }
        })?;
        cleanup_authority
            .unlink()
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("cross-device source cleanup failed: {error}"),
            })?;

        let retained_displaced_path = retained_displaced_identity
            .map(|_| destination_transaction_path.join(CROSS_DEVICE_STAGED_NAME));
        if retained_displaced_path.is_none() {
            let _ = std::fs::remove_dir_all(destination_transaction_path);
            let _ =
                cleanup_transaction_journals(&recovery_root, &local_path, &recovery_transaction_id);
        }
        return Ok(MovePublication {
            #[cfg(target_os = "windows")]
            windows_undo: None,
            disposition: MoveDisposition::Moved,
            published_path: Some(destination_target),
            retained_displaced_path,
            published_content: Some(destination_tree_snapshot),
        });
    }

    loop {
        let expected_displaced_destination_identity = if conflict == ConflictAction::Overwrite {
            Some(stable_identity_from_metadata(
                &std::fs::symlink_metadata(&destination_target).map_err(|error| {
                    PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!("overwrite destination identity probe failed: {error}"),
                    }
                })?,
            ))
        } else {
            None
        };
        recovery
            .advance_to_pending_destination_with_local_with_metadata(
                &local,
                recovery_transaction_id,
                &destination_target,
                move_recovery::PendingDestinationMetadata {
                    expected_displaced_destination_identity,
                    ..Default::default()
                },
            )
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("pending destination persistence failed: {error}"),
            })?;
        hook(TestPoint::PendingDestination)
            .map_err(|message| PlatformError::Io(format!("move test hook failed: {message}")))?;

        match publish_claimed_to_destination_with_no_replace(
            &local,
            &destination_parent,
            std::ffi::OsStr::new("claimed"),
            destination_name.as_os_str(),
        ) {
            Ok(()) => {
                break;
            }
            Err(publish_error) if publish_error.kind() == std::io::ErrorKind::AlreadyExists => {
                if conflict == ConflictAction::Skip {
                    restore_claimed_to_source(
                        &source_parent,
                        &local,
                        source_name,
                    )
                    .map_err(|restore_error| PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!(
                            "skip-mode publication conflict but claim could not be restored: {restore_error}"
                        ),
                    })?;
                    let _ = cleanup_transaction_journals(
                        &recovery_root,
                        &local_path,
                        &recovery_transaction_id,
                    );
                    return Ok(MovePublication {
                        #[cfg(target_os = "windows")]
                        windows_undo: None,
                        disposition: MoveDisposition::Skipped,
                        published_path: None,
                        retained_displaced_path: None,
                        published_content: None,
                    });
                }

                if conflict == ConflictAction::Overwrite {
                    publish_claimed_to_destination_with_exchange(
                        &local,
                        &destination_parent,
                        std::ffi::OsStr::new("claimed"),
                        destination_name.as_os_str(),
                    )
                    .map_err(|publish_error| {
                        PlatformError::ClaimedRecoveryPending {
                            transaction: local_path.display().to_string(),
                            message: format!("publication failed: {publish_error}"),
                        }
                    })?;
                    break;
                }

                destination_name = next_available_name(&destination_target)
                    .file_name()
                    .expect("candidate has a file name")
                    .to_os_string();
                destination_target = destination_dir.join(&destination_name);
                if conflict == ConflictAction::Rename {
                    continue;
                }
                return Err(PlatformError::ClaimedRecoveryPending {
                    transaction: local_path.display().to_string(),
                    message: "publication conflict occurred for unsupported conflict mode"
                        .to_string(),
                });
            }
            Err(publish_error) => {
                return Err(PlatformError::ClaimedRecoveryPending {
                    transaction: local_path.display().to_string(),
                    message: format!("publication failed: {publish_error}"),
                });
            }
        }
    }

    hook(TestPoint::Publication).map_err(|message| {
        if conflict == ConflictAction::Overwrite {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("move test hook failed after publication: {message}"),
            }
        } else {
            PlatformError::Io(format!("move test hook failed: {message}"))
        }
    })?;

    if conflict == ConflictAction::Overwrite {
        let retained_displaced_path = local_path.join("claimed");
        let displaced_identity = stable_identity_from_metadata(
            &std::fs::symlink_metadata(&retained_displaced_path).map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("retained displaced destination probe failed: {error}"),
                }
            })?,
        );
        recovery
            .advance_to_published_with_observed_displaced_identity_with_local(
                &local,
                recovery_transaction_id,
                displaced_identity,
            )
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("overwrite publication durable state failed: {error}"),
            })?;
        Ok(MovePublication {
            #[cfg(target_os = "windows")]
            windows_undo: None,
            disposition: MoveDisposition::Moved,
            published_path: Some(destination_target),
            retained_displaced_path: Some(retained_displaced_path),
            published_content: None,
        })
    } else {
        recovery
            .advance_to_published_with_local(&local, recovery_transaction_id)
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("publication durable state failed: {error}"),
            })?;

        let _ = cleanup_transaction_journals(&recovery_root, &local_path, &recovery_transaction_id);
        Ok(MovePublication {
            #[cfg(target_os = "windows")]
            windows_undo: None,
            disposition: MoveDisposition::Moved,
            published_path: Some(destination_target),
            retained_displaced_path: None,
            published_content: None,
        })
    }
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn recover_same_filesystem_pending_destination(
    source: &Path,
    destination_dir: &Path,
    source_name: &std::ffi::OsStr,
    source_parent: &move_recovery::LinuxSourceParentDirectory,
    source_parent_path: &Path,
    source_parent_identity: move_recovery::LinuxStableIdentity,
    source_entry_identity: Option<move_recovery::LinuxStableIdentity>,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    conflict: ConflictAction,
) -> Result<Option<MoveDisposition>, PlatformError> {
    let recovery_root = default_recovery_root()?;
    let discovered = discover_retained_move_transactions_at(&recovery_root)?;
    if !discovered
        .iter()
        .any(|entry| entry.phase == move_recovery::JournalPhase::PendingDestination)
    {
        return Ok(None);
    }

    let expected_source_name = Path::new(source_name);
    let central = move_recovery::LinuxJournalDirectory::open_configured_read_only(&recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut recovery = central
        .try_lock_recovery()
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    for entry in discovered {
        if entry.phase != move_recovery::JournalPhase::PendingDestination {
            continue;
        }

        let local_path = source_parent_path.join(format!(
            ".ottrin-move-{}",
            transaction_id_hex(&entry.transaction_id)
        ));
        let central_pending = match std::fs::read(recovery_root.join(format!(
            "{}.journal",
            transaction_id_hex(&entry.transaction_id)
        )))
        .map_err(|error| PlatformError::Io(error.to_string()))
        .and_then(|bytes| {
            move_recovery::PendingDestinationRecord::parse(&bytes)
                .map_err(|error| PlatformError::Io(error.to_string()))
        }) {
            Ok(pending) => pending,
            Err(_) => continue,
        };
        let local_name = match central_pending.source_transaction_name() {
            Ok(name) => name,
            Err(_) => continue,
        };
        let local = match source_parent
            .open_existing_transaction(&local_name, central_pending.source_transaction_identity())
        {
            Ok(local) => local,
            Err(_) => continue,
        };

        let state =
            match central.load_pending_destination_restart_state(&local, entry.transaction_id) {
                Ok(state) => state,
                Err(_) => continue,
            };
        let pending = match state {
            move_recovery::LinuxPendingDestinationRestartState::MatchingPair(pending) => pending,
            move_recovery::LinuxPendingDestinationRestartState::CentralPendingDestinationLocalClaimed {
                pending,
                ..
            } => {
                recovery
                    .resume_pending_destination_with_local(&local, entry.transaction_id)
                    .map_err(|error| PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!(
                            "pending-destination restart persistence failed during recovery: {error}"
                        ),
                    })?;
                pending
            }
        };

        if pending.source_parent_identity() != source_parent_identity {
            continue;
        }
        if !pending.original_name_matches_path(expected_source_name) {
            continue;
        }
        if let Some(expected_identity) = source_entry_identity
            && pending.expected_source_identity() != expected_identity
        {
            continue;
        }
        if pending.has_cross_device_destination_state() {
            continue;
        }
        if let Some(disposition) = replay_same_filesystem_pending_destination_with_record(
            source,
            source_parent,
            destination_parent,
            destination_dir,
            &recovery_root,
            entry.transaction_id,
            &local_path,
            &local,
            &mut recovery,
            pending,
            conflict,
        )? {
            return Ok(Some(disposition));
        }
    }

    Ok(None)
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn recover_cross_filesystem_pending_destination(
    source: &Path,
    destination_dir: &Path,
    source_name: &std::ffi::OsStr,
    source_parent: &move_recovery::LinuxSourceParentDirectory,
    source_parent_path: &Path,
    source_parent_identity: move_recovery::LinuxStableIdentity,
    source_entry_identity: Option<move_recovery::LinuxStableIdentity>,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    conflict: ConflictAction,
    hook: &mut impl FnMut(TestPoint) -> Result<(), &'static str>,
) -> Result<Option<MovePublication>, PlatformError> {
    let recovery_root = default_recovery_root()?;
    let discovered = discover_retained_move_transactions_at(&recovery_root)?;
    if !discovered
        .iter()
        .any(|entry| entry.phase == move_recovery::JournalPhase::PendingDestination)
    {
        return Ok(None);
    }

    let expected_source_name = Path::new(source_name);
    let central = move_recovery::LinuxJournalDirectory::open_configured_read_only(&recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut recovery = central
        .try_lock_recovery()
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    for entry in discovered {
        if entry.phase != move_recovery::JournalPhase::PendingDestination {
            continue;
        }

        let local_path = source_parent_path.join(format!(
            ".ottrin-move-{}",
            transaction_id_hex(&entry.transaction_id)
        ));
        let central_pending = match std::fs::read(recovery_root.join(format!(
            "{}.journal",
            transaction_id_hex(&entry.transaction_id)
        )))
        .map_err(|error| PlatformError::Io(error.to_string()))
        .and_then(|bytes| {
            move_recovery::PendingDestinationRecord::parse(&bytes)
                .map_err(|error| PlatformError::Io(error.to_string()))
        }) {
            Ok(pending) => pending,
            Err(_) => continue,
        };
        let local_name = match central_pending.source_transaction_name() {
            Ok(name) => name,
            Err(_) => continue,
        };
        let local = match source_parent
            .open_existing_transaction(&local_name, central_pending.source_transaction_identity())
        {
            Ok(local) => local,
            Err(_) => continue,
        };

        let state =
            match central.load_pending_destination_restart_state(&local, entry.transaction_id) {
                Ok(state) => state,
                Err(_) => continue,
            };
        let pending = match state {
            move_recovery::LinuxPendingDestinationRestartState::MatchingPair(pending) => pending,
            move_recovery::LinuxPendingDestinationRestartState::CentralPendingDestinationLocalClaimed {
                pending,
                ..
            } => {
                recovery
                    .resume_pending_destination_with_local(&local, entry.transaction_id)
                    .map_err(|error| PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!(
                            "pending-destination restart persistence failed during recovery: {error}"
                        ),
                    })?;
                pending
            }
        };

        if pending.source_parent_identity() != source_parent_identity {
            continue;
        }
        if !pending.original_name_matches_path(expected_source_name) {
            continue;
        }
        if let Some(expected_identity) = source_entry_identity
            && pending.expected_source_identity() != expected_identity
        {
            continue;
        }
        if !pending.has_cross_device_destination_state() {
            continue;
        }
        if let Some(disposition) = replay_cross_filesystem_pending_destination_with_record(
            source,
            source_parent,
            destination_parent,
            destination_dir,
            &recovery_root,
            entry.transaction_id,
            &local_path,
            &local,
            &mut recovery,
            pending,
            conflict,
            hook,
        )? {
            return Ok(Some(disposition));
        }
    }

    Ok(None)
}

#[cfg(target_os = "linux")]
pub(super) fn recover_retained_move_pending_destination(
    transaction_id: [u8; 16],
) -> Result<(), PlatformError> {
    let recovery_root = default_recovery_root()?;
    recover_retained_move_pending_destination_at(&recovery_root, transaction_id)
}

#[cfg(target_os = "linux")]
fn recover_published_cross_device_move_at(
    recovery_root: &Path,
    transaction_id: [u8; 16],
) -> Result<(), PlatformError> {
    let transaction_id_hex = transaction_id_hex(&transaction_id);
    let central = move_recovery::LinuxJournalDirectory::open_configured(recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut recovery = central
        .try_lock_recovery()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let central_journal =
        std::fs::read(recovery_root.join(format!("{transaction_id_hex}.journal")))
            .map_err(|error| PlatformError::Io(error.to_string()))?;
    let published = move_recovery::PublishedRecord::parse(&central_journal).map_err(|error| {
        PlatformError::Io(format!("failed to parse published Move journal: {error}"))
    })?;
    let source_parent_path = published
        .source_parent()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_parent_metadata = std::fs::symlink_metadata(&source_parent_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    if stable_identity_from_metadata(&source_parent_metadata) != published.source_parent_identity()
    {
        return Err(PlatformError::Io(format!(
            "source parent identity changed for published Move transaction {transaction_id_hex}"
        )));
    }
    let source_parent = move_recovery::LinuxSourceParentDirectory::open(&source_parent_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let local_name = published
        .source_transaction_name()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let local_path = source_parent_path.join(&local_name);
    let local = source_parent
        .open_existing_transaction(&local_name, published.source_transaction_identity())
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    recovery
        .resume_published_with_local(&local, transaction_id)
        .map_err(|error| {
            PlatformError::Io(format!("failed to resume published Move journals: {error}"))
        })?;
    let published = match central
        .load_published_restart_state(&local, transaction_id)
        .map_err(|error| PlatformError::Io(error.to_string()))?
    {
        move_recovery::LinuxPublishedRestartState::MatchingPair(record) => record,
        _ => {
            return Err(PlatformError::Io(format!(
                "published Move transaction {transaction_id_hex} requires manual review"
            )));
        }
    };
    let retained_displaced_identity = published.observed_displaced_destination_identity();
    let destination = published
        .pending_destination_name()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let expected_destination_identity =
        published.staged_destination_identity().ok_or_else(|| {
            PlatformError::Io("published Move is missing destination identity evidence".to_string())
        })?;
    let destination_metadata = std::fs::symlink_metadata(&destination)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    if stable_identity_from_metadata(&destination_metadata) != expected_destination_identity {
        return Err(PlatformError::Io(
            "published Move destination identity changed before source cleanup".to_string(),
        ));
    }
    let claimed_path = local_path.join("claimed");
    let claimed_metadata = match std::fs::symlink_metadata(&claimed_path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    };
    let cleanup_path = local_path.join(SOURCE_CLEANUP_NAME);
    let cleanup_metadata = match std::fs::symlink_metadata(&cleanup_path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    };
    if claimed_metadata.is_some() && cleanup_metadata.is_some() {
        return Err(PlatformError::Io(
            "published Move has both claimed and quarantined source entries".to_string(),
        ));
    }
    let retained_source = claimed_metadata
        .as_ref()
        .map(|metadata| (&claimed_path, metadata))
        .or_else(|| {
            cleanup_metadata
                .as_ref()
                .map(|metadata| (&cleanup_path, metadata))
        });
    if let Some((retained_source_path, retained_source_metadata)) = retained_source {
        if stable_identity_from_metadata(retained_source_metadata)
            != published.claimed_source_identity()
        {
            return Err(PlatformError::Io(
                "published Move retained source identity changed before cleanup".to_string(),
            ));
        }
        let source_snapshot = published.source_tree_snapshot().ok_or_else(|| {
            PlatformError::Io("published Move is missing source snapshot evidence".to_string())
        })?;
        if !verify_source_tree_snapshot(retained_source_path, &source_snapshot)
            .map_err(|error| PlatformError::Io(error.to_string()))?
        {
            return Err(PlatformError::Io(
                "published Move retained source content changed before cleanup".to_string(),
            ));
        }
    } else {
        let original_name = published
            .original_name()
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        let original_path = source_parent_path.join(original_name);
        if std::fs::symlink_metadata(&original_path).is_ok() {
            return Err(PlatformError::Io(
                "published Move source reappeared after claimed source cleanup".to_string(),
            ));
        }
    }
    let destination_transaction_name = published
        .destination_transaction_name()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let destination_transaction_identity = published
        .destination_transaction_identity()
        .ok_or_else(|| {
            PlatformError::Io(
                "published Move is missing destination transaction identity".to_string(),
            )
        })?;
    let destination_transaction_path = destination
        .parent()
        .ok_or_else(|| PlatformError::Io("published Move destination has no parent".to_string()))?
        .join(destination_transaction_name);
    let destination_transaction_metadata = std::fs::symlink_metadata(&destination_transaction_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    if stable_identity_from_metadata(&destination_transaction_metadata)
        != destination_transaction_identity
    {
        return Err(PlatformError::Io(
            "published Move destination transaction identity changed before cleanup".to_string(),
        ));
    }

    if let Some(expected_displaced_identity) = retained_displaced_identity {
        let retained_displaced_path = destination_transaction_path.join(CROSS_DEVICE_STAGED_NAME);
        let retained_displaced_metadata = std::fs::symlink_metadata(&retained_displaced_path)
            .map_err(|error| {
                PlatformError::Io(format!(
                    "published overwrite retained destination is unavailable: {error}"
                ))
            })?;
        if stable_identity_from_metadata(&retained_displaced_metadata)
            != expected_displaced_identity
        {
            return Err(PlatformError::Io(
                "published overwrite retained destination identity changed".to_string(),
            ));
        }
    }

    if let Some((retained_source_path, _)) = retained_source {
        let retained_name = retained_source_path.file_name().ok_or_else(|| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "published source cleanup name is unavailable".to_string(),
            }
        })?;
        local
            .claimed_source_cleanup_authority(retained_name, published.claimed_source_identity())
            .and_then(move_recovery::LinuxClaimedSourceCleanupAuthority::unlink)
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("published source cleanup failed: {error}"),
            })?;
    }
    if retained_displaced_identity.is_some() {
        return Ok(());
    }
    std::fs::remove_dir_all(&destination_transaction_path).map_err(|error| {
        PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!("destination transaction cleanup failed: {error}"),
        }
    })?;
    cleanup_transaction_journals(recovery_root, &local_path, &transaction_id)
}

#[cfg(target_os = "linux")]
pub(super) fn recover_retained_move_pending_destination_at(
    recovery_root: &Path,
    transaction_id: [u8; 16],
) -> Result<(), PlatformError> {
    let transaction_id_hex = transaction_id_hex(&transaction_id);
    let discovered = discover_retained_move_transactions_at(recovery_root)?;
    let discovered_phase = discovered
        .iter()
        .find(|entry| entry.transaction_id == transaction_id)
        .map(|entry| entry.phase);
    if discovered_phase == Some(move_recovery::JournalPhase::Published) {
        return recover_published_cross_device_move_at(recovery_root, transaction_id);
    }
    if discovered_phase != Some(move_recovery::JournalPhase::PendingDestination) {
        return Err(PlatformError::Io(format!(
            "retained Move transaction {transaction_id_hex} is not recoverable",
        )));
    }

    let central = move_recovery::LinuxJournalDirectory::open_configured(recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut recovery = central
        .try_lock_recovery()
        .map_err(|error| PlatformError::Io(error.to_string()))?;

    let central_journal_path = recovery_root.join(format!("{transaction_id_hex}.journal"));
    let central_journal = std::fs::read(&central_journal_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let pending =
        move_recovery::PendingDestinationRecord::parse(&central_journal).map_err(|error| {
            PlatformError::Io(format!("failed to parse retained Move journal: {error}"))
        })?;

    let source_parent_path = pending.source_parent().map_err(|error| {
        PlatformError::Io(format!("failed to decode source parent hint: {error}"))
    })?;

    let source_parent_metadata = std::fs::symlink_metadata(&source_parent_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let source_parent_identity = move_recovery::LinuxStableIdentity::new(
        source_parent_metadata.dev(),
        source_parent_metadata.ino(),
        if source_parent_metadata.is_dir() {
            move_recovery::LinuxFileType::Directory
        } else {
            move_recovery::LinuxFileType::Other
        },
    );
    if source_parent_identity != pending.source_parent_identity() {
        return Err(PlatformError::Io(format!(
            "source parent identity changed for retained Move transaction {transaction_id_hex}"
        )));
    }

    let source_parent = move_recovery::LinuxSourceParentDirectory::open(&source_parent_path)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let local_path = source_parent_path.join(format!(".ottrin-move-{transaction_id_hex}"));
    let local_name = pending.source_transaction_name().map_err(|error| {
        PlatformError::Io(format!("failed to decode source transaction name: {error}"))
    })?;
    let local = source_parent
        .open_existing_transaction(&local_name, pending.source_transaction_identity())
        .map_err(|error| {
            PlatformError::Io(format!(
                "unable to open source transaction directory for {transaction_id_hex}: {error}"
            ))
        })?;

    let pending = match central
        .load_pending_destination_restart_state(&local, transaction_id)
        .map_err(|error| PlatformError::Io(format!("failed to load pending destination state: {error}")))?
    {
        move_recovery::LinuxPendingDestinationRestartState::MatchingPair(pending) => pending,
        move_recovery::LinuxPendingDestinationRestartState::CentralPendingDestinationLocalClaimed {
            pending,
            ..
        } => {
            recovery
                .resume_pending_destination_with_local(&local, transaction_id)
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("recovery restart persistence failed: {error}"),
                })?;
            pending
        }
    };

    let source_name = pending
        .original_name()
        .map_err(|error| PlatformError::Io(format!("failed to decode original name: {error}")))?;
    let source_name = source_name.file_name().ok_or_else(|| {
        PlatformError::Io("invalid original source path for retained Move entry".into())
    })?;
    let claimed_path = local_path.join("claimed");
    let claimed_identity = match std::fs::symlink_metadata(&claimed_path) {
        Ok(metadata) => Some(stable_identity_from_metadata(&metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    };
    let mut cross_device_transaction_to_cleanup = None;
    let mut source_already_restored = false;
    if let Some(destination_transaction_identity) = pending.destination_transaction_identity() {
        let expected_displaced_identity = pending
            .expected_displaced_destination_identity()
            .ok_or_else(|| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message:
                    "cross-device overwrite recovery is missing displaced destination evidence"
                        .to_string(),
            })?;
        let staged_destination_identity =
            pending.staged_destination_identity().ok_or_else(|| {
                PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: "cross-device overwrite recovery is missing staged source evidence"
                        .to_string(),
                }
            })?;
        let destination_target = pending.pending_destination_name().map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("cross-device overwrite destination path decode failed: {error}"),
            }
        })?;
        if !destination_target.is_absolute() {
            return Err(PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "cross-device overwrite recovery destination is not absolute".to_string(),
            });
        }
        let destination_parent_path =
            destination_target
                .parent()
                .ok_or_else(|| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: "cross-device overwrite recovery destination has no parent"
                        .to_string(),
                })?;
        let destination_name = destination_target.file_name().ok_or_else(|| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "cross-device overwrite recovery destination has no name".to_string(),
            }
        })?;
        let destination_transaction_name =
            pending.destination_transaction_name().map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("cross-device transaction name decode failed: {error}"),
                }
            })?;
        let destination_parent = move_recovery::LinuxSourceParentDirectory::open(
            destination_parent_path,
        )
        .map_err(|error| PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!("cross-device destination parent open failed: {error}"),
        })?;
        let destination_transaction = destination_parent
            .open_existing_transaction(
                &destination_transaction_name,
                destination_transaction_identity,
            )
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("cross-device destination transaction open failed: {error}"),
            })?;
        let staged_path = destination_parent_path
            .join(&destination_transaction_name)
            .join(CROSS_DEVICE_STAGED_NAME);
        let destination_identity = stable_identity_from_metadata(
            &std::fs::symlink_metadata(&destination_target).map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("cross-device overwrite destination probe failed: {error}"),
                }
            })?,
        );
        let staged_identity =
            stable_identity_from_metadata(&std::fs::symlink_metadata(&staged_path).map_err(
                |error| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("cross-device retained destination probe failed: {error}"),
                },
            )?);

        if destination_identity == staged_destination_identity
            && staged_identity == expected_displaced_identity
        {
            publish_claimed_to_destination_with_exchange(
                &destination_transaction,
                &destination_parent,
                std::ffi::OsStr::new(CROSS_DEVICE_STAGED_NAME),
                destination_name,
            )
            .map_err(|error| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("cross-device overwrite rollback exchange failed: {error}"),
            })?;
        } else if destination_identity != expected_displaced_identity
            || staged_identity != staged_destination_identity
        {
            return Err(PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message:
                    "cross-device overwrite entries no longer match retained recovery evidence"
                        .to_string(),
            });
        }

        if claimed_identity.is_none() {
            let source_identity = stable_identity_from_metadata(
                &std::fs::symlink_metadata(source_parent_path.join(source_name)).map_err(
                    |error| PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!("cross-device source restore probe failed: {error}"),
                    },
                )?,
            );
            if source_identity != pending.claimed_source_identity() {
                return Err(PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: "cross-device source no longer matches retained recovery evidence"
                        .to_string(),
                });
            }
            source_already_restored = true;
        }

        cross_device_transaction_to_cleanup =
            Some(destination_parent_path.join(&destination_transaction_name));
    }
    if let Some(claimed_identity) = claimed_identity {
        if claimed_identity != pending.claimed_source_identity() {
            let expected_displaced = pending
                .expected_displaced_destination_identity()
                .filter(|identity| *identity == claimed_identity)
                .ok_or_else(|| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: "claimed identity no longer matches retained recovery evidence"
                        .to_string(),
                })?;
            let destination_target = pending.pending_destination_name().map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("overwrite destination path decode failed: {error}"),
                }
            })?;
            if !destination_target.is_absolute() {
                return Err(PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message:
                        "overwrite recovery journal does not contain an absolute destination path"
                            .to_string(),
                });
            }
            let destination_identity = stable_identity_from_metadata(
                &std::fs::symlink_metadata(&destination_target).map_err(|error| {
                    PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!("overwrite destination identity probe failed: {error}"),
                    }
                })?,
            );
            if destination_identity != pending.claimed_source_identity()
                || expected_displaced != claimed_identity
            {
                return Err(PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message:
                    "overwrite destination identity no longer matches retained recovery evidence"
                        .to_string(),
            });
            }
            match std::fs::symlink_metadata(source_parent_path.join(source_name)) {
                Ok(_) => {
                    return Err(PlatformError::DestinationExists(
                        source_parent_path.join(source_name).display().to_string(),
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(PlatformError::Io(error.to_string())),
            }
            super::exchange_paths(&claimed_path, &destination_target).map_err(|error| {
                PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!("overwrite recovery exchange failed: {error}"),
                }
            })?;
        }
    } else if !source_already_restored {
        return Err(PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: "claimed source is missing before recovery completed".to_string(),
        });
    }

    if !source_already_restored {
        restore_claimed_to_source(&source_parent, &local, source_name).map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("restore claimed source failed: {error}"),
            }
        })?;
    }

    if let Some(destination_transaction_path) = cross_device_transaction_to_cleanup {
        std::fs::remove_dir_all(&destination_transaction_path).map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: format!("cross-device transaction cleanup failed: {error}"),
            }
        })?;
    }

    let _ = cleanup_transaction_journals(recovery_root, &local_path, &transaction_id);
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn replay_cross_filesystem_pending_destination_with_record(
    source: &Path,
    source_parent: &move_recovery::LinuxSourceParentDirectory,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    destination_dir: &Path,
    recovery_root: &Path,
    transaction_id: [u8; 16],
    local_path: &Path,
    local: &move_recovery::LinuxJournalDirectory,
    recovery: &mut move_recovery::LinuxRecoveryLock<'_>,
    pending: move_recovery::PendingDestinationRecord,
    conflict: ConflictAction,
    hook: &mut impl FnMut(TestPoint) -> Result<(), &'static str>,
) -> Result<Option<MovePublication>, PlatformError> {
    let source_name = pending
        .original_name()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let claimed_source_identity = pending.claimed_source_identity();
    let destination_transaction_name = pending.destination_transaction_name().map_err(|error| {
        PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!("cross-device destination transaction name decode failed: {error}"),
        }
    })?;
    let destination_transaction_identity =
        pending.destination_transaction_identity().ok_or_else(|| {
            PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "cross-device destination transaction identity is missing".to_string(),
            }
        })?;
    let source_tree_snapshot =
        pending
            .source_tree_snapshot()
            .ok_or_else(|| PlatformError::CommittedCleanupPending {
                transaction: local_path.display().to_string(),
                message: "cross-device source-tree snapshot is missing".to_string(),
            })?;
    let staged_destination_identity = pending.staged_destination_identity().ok_or_else(|| {
        PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: "cross-device staged destination identity is missing".to_string(),
        }
    })?;
    let mut destination_name = pending
        .pending_destination_name()
        .map_err(|error| PlatformError::Io(error.to_string()))?
        .file_name()
        .expect("pending destination has a file name")
        .to_os_string();
    let mut destination_target = destination_dir.join(&destination_name);
    let destination_transaction_path = destination_dir.join(&destination_transaction_name);
    let claimed_path = local_path.join("claimed");

    let pending_destination_metadata = move_recovery::PendingDestinationMetadata {
        destination_transaction_name: Some(destination_transaction_name.clone()),
        destination_transaction_identity: Some(destination_transaction_identity),
        source_tree_snapshot: Some(source_tree_snapshot.clone()),
        staged_destination_identity: Some(staged_destination_identity),
        expected_displaced_destination_identity: None,
    };

    loop {
        match std::fs::symlink_metadata(&claimed_path) {
            Ok(metadata) => {
                if stable_identity_from_metadata(&metadata) != claimed_source_identity {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message:
                            "recovered cross-device claimed identity changed before publication"
                                .to_string(),
                    });
                }
                let destination_transaction_metadata =
                    std::fs::symlink_metadata(&destination_transaction_path).map_err(|error| {
                        PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: format!(
                                "recovery destination transaction metadata failed: {error}"
                            ),
                        }
                    })?;
                if stable_identity_from_metadata(&destination_transaction_metadata)
                    != destination_transaction_identity
                {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message:
                            "recovery destination transaction identity changed before publication"
                                .to_string(),
                    });
                }
                let staged_destination_metadata = std::fs::symlink_metadata(
                    destination_transaction_path.join(CROSS_DEVICE_STAGED_NAME),
                )
                .map_err(|error| PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: format!(
                        "recovery staged destination metadata unavailable before publication: {error}"
                    ),
                })?;
                if stable_identity_from_metadata(&staged_destination_metadata)
                    != staged_destination_identity
                {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: "recovery staged destination identity changed before publication"
                            .to_string(),
                    });
                }
                let staged_destination_path =
                    destination_transaction_path.join(CROSS_DEVICE_STAGED_NAME);
                let staged_tree_snapshot = source_tree_snapshot_for_path(&staged_destination_path)
                    .map_err(|error| PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!(
                            "recovery staged destination content verification failed: {error}"
                        ),
                    })?;
                if !source_tree_snapshot.has_same_paths_and_content(&staged_tree_snapshot) {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message:
                            "recovery staged destination content does not match source snapshot"
                                .to_string(),
                    });
                }

                let destination_transaction_directory = destination_parent
                    .open_existing_transaction(
                        &destination_transaction_name,
                        destination_transaction_identity,
                    )
                    .map_err(|error| {
                        PlatformError::Io(format!(
                            "recovery could not reopen destination transaction directory: {error}"
                        ))
                    })?;
                match publish_claimed_to_destination_with_no_replace(
                    &destination_transaction_directory,
                    destination_parent,
                    std::ffi::OsStr::new(CROSS_DEVICE_STAGED_NAME),
                    destination_name.as_os_str(),
                ) {
                    Ok(()) => {
                        let destination_metadata = std::fs::symlink_metadata(&destination_target)
                            .map_err(|error| {
                            PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: format!(
                                    "cross-device destination identity check failed: {error}"
                                ),
                            }
                        })?;
                        if stable_identity_from_metadata(&destination_metadata)
                            != staged_destination_identity
                        {
                            return Err(PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message:
                                    "cross-device destination identity changed after publication"
                                        .to_string(),
                            });
                        }
                        sync_replayed_destination_parent(destination_parent, local_path, hook)?;
                        if !verify_source_tree_snapshot(&claimed_path, &source_tree_snapshot)
                            .map_err(|error| PlatformError::Io(error.to_string()))?
                        {
                            return Err(PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: "cross-device source snapshot changed before cleanup"
                                    .to_string(),
                            });
                        }

                        let cleanup_authority = local
                            .claimed_source_cleanup_authority(
                                std::ffi::OsStr::new("claimed"),
                                claimed_source_identity,
                            )
                            .map_err(|error| PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: format!(
                                    "cross-device source cleanup authority could not be retained: {error}"
                                ),
                            })?;
                        recovery
                            .advance_to_published_with_local(local, transaction_id)
                            .map_err(|error| PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: format!(
                                    "cross-device publication durable state failed: {error}"
                                ),
                            })?;
                        cleanup_authority.unlink().map_err(|error| {
                            PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: format!("cross-device source cleanup failed: {error}"),
                            }
                        })?;
                        let _ = std::fs::remove_dir_all(&destination_transaction_path);
                        let _ = cleanup_transaction_journals(
                            recovery_root,
                            local_path,
                            &transaction_id,
                        );
                        return Ok(Some(MovePublication {
                            #[cfg(target_os = "windows")]
                            windows_undo: None,
                            disposition: MoveDisposition::Moved,
                            published_path: Some(destination_target),
                            retained_displaced_path: None,
                            published_content: Some(source_tree_snapshot.clone()),
                        }));
                    }
                    Err(publish_error)
                        if publish_error.kind() == std::io::ErrorKind::AlreadyExists =>
                    {
                        let destination_metadata = match std::fs::symlink_metadata(
                            &destination_target,
                        ) {
                            Ok(metadata) => metadata,
                            Err(error) => {
                                return Err(PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "cross-device destination conflict check failed: {error}"
                                    ),
                                });
                            }
                        };
                        if stable_identity_from_metadata(&destination_metadata)
                            == staged_destination_identity
                        {
                            sync_replayed_destination_parent(destination_parent, local_path, hook)?;
                            if !verify_source_tree_snapshot(&claimed_path, &source_tree_snapshot)
                                .map_err(|error| PlatformError::Io(error.to_string()))?
                            {
                                return Err(PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: "cross-device source snapshot changed before cleanup"
                                        .to_string(),
                                });
                            }
                            let cleanup_authority = local
                                .claimed_source_cleanup_authority(
                                    std::ffi::OsStr::new("claimed"),
                                    claimed_source_identity,
                                )
                                .map_err(|error| PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "cross-device source cleanup authority could not be retained: {error}"
                                    ),
                                })?;
                            recovery
                                .advance_to_published_with_local(local, transaction_id)
                                .map_err(|error| PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "cross-device publication durable state failed: {error}"
                                    ),
                                })?;
                            cleanup_authority.unlink().map_err(|error| {
                                PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!("cross-device source cleanup failed: {error}"),
                                }
                            })?;
                            let _ = std::fs::remove_dir_all(&destination_transaction_path);
                            let _ = cleanup_transaction_journals(
                                recovery_root,
                                local_path,
                                &transaction_id,
                            );
                            return Ok(Some(MovePublication {
                                #[cfg(target_os = "windows")]
                                windows_undo: None,
                                disposition: MoveDisposition::Moved,
                                published_path: Some(destination_target),
                                retained_displaced_path: None,
                                published_content: Some(source_tree_snapshot.clone()),
                            }));
                        }

                        if conflict == ConflictAction::Skip {
                            if source.exists() {
                                return Err(PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message:
                                        "source reappeared before recovery skip-mode restoration"
                                            .to_string(),
                                });
                            }

                            restore_claimed_to_source(
                                source_parent,
                                local,
                                source_name.file_name().unwrap_or_default(),
                            )
                            .map_err(|restore_error| {
                                PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "skip-mode recovery restoration failed: {restore_error}"
                                    ),
                                }
                            })?;
                            let _ = cleanup_transaction_journals(
                                recovery_root,
                                local_path,
                                &transaction_id,
                            );
                            return Ok(Some(MovePublication {
                                #[cfg(target_os = "windows")]
                                windows_undo: None,
                                disposition: MoveDisposition::Skipped,
                                published_path: None,
                                retained_displaced_path: None,
                                published_content: None,
                            }));
                        }

                        if conflict == ConflictAction::Rename {
                            destination_name = next_available_name(&destination_target)
                                .file_name()
                                .expect("candidate has a file name")
                                .to_os_string();
                            destination_target = destination_dir.join(&destination_name);
                            let displaced_destination_identity =
                                stable_identity_from_metadata(&destination_metadata);
                            recovery
                                .advance_to_pending_destination_with_local_with_metadata(
                                    local,
                                    transaction_id,
                                    destination_name.as_ref(),
                                    move_recovery::PendingDestinationMetadata {
                                        expected_displaced_destination_identity: Some(
                                            displaced_destination_identity,
                                        ),
                                        ..pending_destination_metadata.clone()
                                    },
                                )
                                .map_err(|error| PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "recovery pending-destination advancement failed: {error}"
                                    ),
                                })?;
                            continue;
                        }

                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: "recovery publication conflict for unsupported mode"
                                .to_string(),
                        });
                    }
                    Err(publish_error) => {
                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: format!("recovery publication failed: {publish_error}"),
                        });
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(PlatformError::CommittedCleanupPending {
                    transaction: local_path.display().to_string(),
                    message: "recovery found no claimed source after cross-device publication"
                        .to_string(),
                });
            }
            Err(error) => {
                return Err(PlatformError::Io(error.to_string()));
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn sync_replayed_destination_parent(
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    local_path: &Path,
    hook: &mut impl FnMut(TestPoint) -> Result<(), &'static str>,
) -> Result<(), PlatformError> {
    hook(TestPoint::DestinationPublished).map_err(|message| {
        PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!(
                "move test hook failed after replay destination publication: {message}"
            ),
        }
    })?;
    destination_parent
        .sync()
        .map_err(|error| PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!("destination parent flush failed after replay publication: {error}"),
        })?;
    hook(TestPoint::DestinationParentSynced).map_err(|message| {
        PlatformError::CommittedCleanupPending {
            transaction: local_path.display().to_string(),
            message: format!(
                "move test hook failed after replay destination-parent sync: {message}"
            ),
        }
    })
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn replay_same_filesystem_pending_destination_with_record(
    source: &Path,
    source_parent: &move_recovery::LinuxSourceParentDirectory,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    destination_dir: &Path,
    recovery_root: &Path,
    transaction_id: [u8; 16],
    local_path: &Path,
    local: &move_recovery::LinuxJournalDirectory,
    recovery: &mut move_recovery::LinuxRecoveryLock<'_>,
    pending: move_recovery::PendingDestinationRecord,
    conflict: ConflictAction,
) -> Result<Option<MoveDisposition>, PlatformError> {
    let source_name = pending
        .original_name()
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let claimed_source_identity = pending.claimed_source_identity();
    let mut destination_name = pending
        .pending_destination_name()
        .map_err(|error| PlatformError::Io(error.to_string()))?
        .file_name()
        .expect("pending destination has a file name")
        .to_os_string();
    let mut destination_target = destination_dir.join(&destination_name);
    let claimed_path = local_path.join("claimed");

    loop {
        match std::fs::symlink_metadata(&claimed_path) {
            Ok(metadata) => {
                if stable_identity_from_metadata(&metadata) != claimed_source_identity {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: "recovered claimed identity changed before publication"
                            .to_string(),
                    });
                }

                match publish_claimed_to_destination_with_no_replace(
                    local,
                    destination_parent,
                    std::ffi::OsStr::new("claimed"),
                    destination_name.as_os_str(),
                ) {
                    Ok(()) => {
                        recovery
                            .advance_to_published_with_local(local, transaction_id)
                            .map_err(|error| PlatformError::CommittedCleanupPending {
                                transaction: local_path.display().to_string(),
                                message: format!(
                                    "recovery publication durable state failed: {error}"
                                ),
                            })?;
                        let _ = cleanup_transaction_journals(
                            recovery_root,
                            local_path,
                            &transaction_id,
                        );
                        return Ok(Some(MoveDisposition::Moved));
                    }
                    Err(publish_error)
                        if publish_error.kind() == std::io::ErrorKind::AlreadyExists =>
                    {
                        let destination_metadata =
                            match std::fs::symlink_metadata(&destination_target) {
                                Ok(metadata) => metadata,
                                Err(error) => {
                                    return Err(PlatformError::CommittedCleanupPending {
                                        transaction: local_path.display().to_string(),
                                        message: format!(
                                            "recovery destination conflict check failed: {error}"
                                        ),
                                    });
                                }
                            };
                        if stable_identity_from_metadata(&destination_metadata)
                            == claimed_source_identity
                        {
                            recovery
                                .advance_to_published_with_local(local, transaction_id)
                                .map_err(|error| PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "recovery publication durable state failed: {error}"
                                    ),
                                })?;
                            let _ = cleanup_transaction_journals(
                                recovery_root,
                                local_path,
                                &transaction_id,
                            );
                            return Ok(Some(MoveDisposition::Moved));
                        }

                        if conflict == ConflictAction::Skip {
                            if source.exists() {
                                return Err(PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message:
                                        "source reappeared before recovery skip-mode restoration"
                                            .to_string(),
                                });
                            }

                            restore_claimed_to_source(
                                source_parent,
                                local,
                                source_name.file_name().unwrap_or_default(),
                            )
                            .map_err(|restore_error| {
                                PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "skip-mode recovery restoration failed: {restore_error}"
                                    ),
                                }
                            })?;
                            let _ = cleanup_transaction_journals(
                                recovery_root,
                                local_path,
                                &transaction_id,
                            );
                            return Ok(Some(MoveDisposition::Skipped));
                        }

                        if conflict == ConflictAction::Rename {
                            destination_name = next_available_name(&destination_target)
                                .file_name()
                                .expect("candidate has a file name")
                                .to_os_string();
                            destination_target = destination_dir.join(&destination_name);
                            recovery
                                .advance_to_pending_destination_with_local(
                                    local,
                                    transaction_id,
                                    &destination_target,
                                )
                                .map_err(|error| PlatformError::CommittedCleanupPending {
                                    transaction: local_path.display().to_string(),
                                    message: format!(
                                        "recovery pending-destination advancement failed: {error}"
                                    ),
                                })?;
                            continue;
                        }

                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: "recovery publication conflict for unsupported mode"
                                .to_string(),
                        });
                    }
                    Err(publish_error) => {
                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: format!("recovery publication failed: {publish_error}"),
                        });
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let destination_metadata = match std::fs::symlink_metadata(&destination_target) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        return Err(PlatformError::CommittedCleanupPending {
                            transaction: local_path.display().to_string(),
                            message: format!(
                                "recovery destination check failed after publication: {error}"
                            ),
                        });
                    }
                };
                if stable_identity_from_metadata(&destination_metadata) != claimed_source_identity {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message:
                            "recovery destination identity does not match claimed source identity"
                                .to_string(),
                    });
                }
                recovery
                    .advance_to_published_with_local(local, transaction_id)
                    .map_err(|error| PlatformError::CommittedCleanupPending {
                        transaction: local_path.display().to_string(),
                        message: format!("recovery publication durable state failed: {error}"),
                    })?;
                let _ = cleanup_transaction_journals(recovery_root, local_path, &transaction_id);
                return Ok(Some(MoveDisposition::Moved));
            }
            Err(error) => {
                return Err(PlatformError::Io(error.to_string()));
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn transaction_id_hex(transaction_id: &[u8; 16]) -> String {
    transaction_id
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(target_os = "linux")]
fn cleanup_transaction_journals(
    recovery_root: &Path,
    local_path: &Path,
    transaction_id: &[u8; 16],
) -> Result<(), PlatformError> {
    let transaction_id = transaction_id_hex(transaction_id);
    let central_journal = recovery_root.join(format!("{transaction_id}.journal"));
    let local_journal = local_path.join("journal");
    std::fs::remove_file(central_journal).ok();
    std::fs::remove_file(local_journal).ok();

    #[cfg(test)]
    {
        std::fs::remove_dir_all(local_path)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
    }

    #[cfg(not(test))]
    {
        let _ = std::fs::remove_dir(local_path);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn cleanup_completed_overwrite_undo(
    retained_displaced_path: &Path,
) -> Result<(), PlatformError> {
    let local_path = retained_displaced_path.parent().ok_or_else(|| {
        PlatformError::InvalidPathMoveUndoReceipt(
            "retained Move overwrite entry has no transaction parent".to_string(),
        )
    })?;
    let transaction_id = local_path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(|name| name.strip_prefix(".ottrin-move-"))
        .filter(|id| id.len() == 32 && id.chars().all(|character| character.is_ascii_hexdigit()))
        .ok_or_else(|| {
            PlatformError::InvalidPathMoveUndoReceipt(
                "retained Move overwrite transaction name is invalid".to_string(),
            )
        })?;
    let central_journal = default_recovery_root()?.join(format!("{transaction_id}.journal"));
    match std::fs::remove_file(local_path.join("journal")) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    }
    match std::fs::remove_dir(local_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    }
    match std::fs::remove_file(&central_journal) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PlatformError::Io(error.to_string())),
    }
}

#[cfg(target_os = "linux")]
fn publish_claimed_to_destination_with_no_replace(
    claimed: &move_recovery::LinuxJournalDirectory,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    source_name: &std::ffi::OsStr,
    destination_name: &std::ffi::OsStr,
) -> std::io::Result<()> {
    claimed.rename_to_parent_no_replace(source_name, destination_parent, destination_name)
}

#[cfg(target_os = "linux")]
fn publish_claimed_to_destination_with_exchange(
    claimed: &move_recovery::LinuxJournalDirectory,
    destination_parent: &move_recovery::LinuxSourceParentDirectory,
    source_name: &std::ffi::OsStr,
    destination_name: &std::ffi::OsStr,
) -> std::io::Result<()> {
    claimed.rename_to_parent_exchange(source_name, destination_parent, destination_name)
}

#[cfg(target_os = "linux")]
fn restore_claimed_to_source(
    source_parent: &move_recovery::LinuxSourceParentDirectory,
    claimed: &move_recovery::LinuxJournalDirectory,
    source_name: &std::ffi::OsStr,
) -> Result<(), String> {
    claimed
        .rename_to_parent_no_replace(std::ffi::OsStr::new("claimed"), source_parent, source_name)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn stable_identity_from_metadata(
    metadata: &std::fs::Metadata,
) -> move_recovery::LinuxStableIdentity {
    let file_type = metadata.file_type();
    move_recovery::LinuxStableIdentity::new(
        metadata.dev(),
        metadata.ino(),
        if metadata.is_file() {
            move_recovery::LinuxFileType::Regular
        } else if metadata.is_dir() {
            move_recovery::LinuxFileType::Directory
        } else if file_type.is_symlink() {
            move_recovery::LinuxFileType::Symlink
        } else {
            move_recovery::LinuxFileType::Other
        },
    )
}

#[cfg(target_os = "linux")]
pub(super) fn source_tree_snapshot_for_path(
    source_path: &Path,
) -> Result<move_recovery::SourceTreeSnapshot, PlatformError> {
    source_tree_snapshot_with_observer(source_path, &mut |_, _| Ok(()))
}

/// The observer sees the same opened objects whose contents are sampled. Copy
/// uses it for filesystem object evidence; Move retains its ordinary snapshot.
#[cfg(target_os = "linux")]
pub(super) fn source_tree_snapshot_with_observer(
    source_path: &Path,
    observer: &mut impl FnMut(&Path, &rustix::fd::OwnedFd) -> Result<(), PlatformError>,
) -> Result<move_recovery::SourceTreeSnapshot, PlatformError> {
    use rustix::fs::{Mode, OFlags};
    let opened = rustix::fs::open(
        source_path,
        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| PlatformError::Io(error.to_string()))?;
    let mut entries = Vec::new();
    collect_source_tree_snapshot(&opened, Path::new(""), &mut entries, observer)?;
    let opened_metadata = std::fs::File::from(
        rustix::io::dup(&opened).map_err(|e| PlatformError::Io(e.to_string()))?,
    )
    .metadata()
    .map_err(|e| PlatformError::Io(e.to_string()))?;
    let named_metadata =
        std::fs::symlink_metadata(source_path).map_err(|e| PlatformError::Io(e.to_string()))?;
    use rustix::fd::AsRawFd;
    let opened_mount = super::local_storage::local_path_mount_identity(
        &PathBuf::from(format!("/proc/self/fd/{}", opened.as_raw_fd())),
        true,
    )
    .map_err(|e| PlatformError::Io(e.to_string()))?;
    let named_mount = super::local_storage::local_path_mount_identity(source_path, false)
        .map_err(|e| PlatformError::Io(e.to_string()))?;
    if stable_identity_from_metadata(&opened_metadata)
        != stable_identity_from_metadata(&named_metadata)
        || opened_mount != named_mount
    {
        return Err(PlatformError::Io(
            "source binding changed while content evidence was read".into(),
        ));
    }
    entries.sort_by(|(left, _, _), (right, _, _)| left.cmp(right));
    Ok(move_recovery::SourceTreeSnapshot::from_evidenced_entries(
        entries,
    ))
}

#[cfg(target_os = "linux")]
fn collect_source_tree_snapshot(
    opened: &rustix::fd::OwnedFd,
    relative: &Path,
    entries: &mut Vec<(
        PathBuf,
        move_recovery::LinuxStableIdentity,
        move_recovery::SourceContentEvidence,
    )>,
    observer: &mut impl FnMut(&Path, &rustix::fd::OwnedFd) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    use rustix::fd::AsRawFd;
    use rustix::fs::{Dir, Mode, OFlags, openat};
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let io_error = |error: std::io::Error| PlatformError::Io(error.to_string());
    let metadata =
        std::fs::File::from(rustix::io::dup(opened).map_err(|e| PlatformError::Io(e.to_string()))?)
            .metadata()
            .map_err(io_error)?;
    let identity = stable_identity_from_metadata(&metadata);
    observer(relative, opened)?;
    let content = if metadata.is_file() {
        // The kernel FD link addresses the already opened regular object, even
        // if its former pathname is replaced. Never open a substituted FIFO.
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(format!("/proc/self/fd/{}", opened.as_raw_fd()))
            .map_err(io_error)?;
        let before_read = file.metadata().map_err(io_error)?;
        if stable_identity_from_metadata(&before_read) != identity {
            return Err(PlatformError::Io(
                "source changed while content evidence was opened".into(),
            ));
        }
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(io_error)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let after_read = file.metadata().map_err(io_error)?;
        if stable_identity_from_metadata(&after_read) != identity
            || after_read.len() != before_read.len()
            || after_read.mtime() != before_read.mtime()
            || after_read.mtime_nsec() != before_read.mtime_nsec()
            || after_read.ctime() != before_read.ctime()
            || after_read.ctime_nsec() != before_read.ctime_nsec()
        {
            return Err(PlatformError::Io(
                "source changed while content evidence was read".into(),
            ));
        }
        move_recovery::SourceContentEvidence::Regular {
            size: before_read.len(),
            sha256_hex: format!("{:x}", hasher.finalize()),
        }
    } else if metadata.is_dir() {
        move_recovery::SourceContentEvidence::Directory
    } else if metadata.file_type().is_symlink() {
        let target = rustix::fs::readlinkat(opened, "", Vec::new())
            .map_err(|e| PlatformError::Io(e.to_string()))?;
        move_recovery::SourceContentEvidence::Symlink {
            target: move_recovery::NativePath::encode(Path::new(&std::ffi::OsString::from_vec(
                target.into_bytes(),
            ))),
        }
    } else {
        return Err(PlatformError::InvalidSource(relative.display().to_string()));
    };
    entries.push((relative.to_path_buf(), identity, content));
    if metadata.is_dir() {
        let directory = openat(
            opened,
            ".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| PlatformError::Io(e.to_string()))?;
        let mut read = Dir::read_from(&directory).map_err(|e| PlatformError::Io(e.to_string()))?;
        while let Some(entry) = read.read() {
            let entry = entry.map_err(|e| PlatformError::Io(e.to_string()))?;
            let bytes = entry.file_name().to_bytes();
            if matches!(bytes, b"." | b"..") {
                continue;
            }
            let name = std::ffi::OsString::from_vec(bytes.to_vec());
            let child = openat(
                &directory,
                &name,
                OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|e| PlatformError::Io(e.to_string()))?;
            collect_source_tree_snapshot(&child, &relative.join(name), entries, observer)?;
        }
        let after = std::fs::File::from(directory)
            .metadata()
            .map_err(io_error)?;
        if after.mtime() != metadata.mtime()
            || after.mtime_nsec() != metadata.mtime_nsec()
            || after.ctime() != metadata.ctime()
            || after.ctime_nsec() != metadata.ctime_nsec()
        {
            return Err(PlatformError::Io(
                "source directory changed while content evidence was read".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_source_tree_snapshot(
    source_path: &Path,
    snapshot: &move_recovery::SourceTreeSnapshot,
) -> Result<bool, PlatformError> {
    let current = source_tree_snapshot_for_path(source_path)?;
    Ok(current == *snapshot)
}

#[cfg(target_os = "linux")]
fn flush_tree_bottom_up(path: &Path) -> Result<(), PlatformError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| PlatformError::Io(error.to_string()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        return std::fs::File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|error| PlatformError::Io(error.to_string()));
    }
    if metadata.is_dir() {
        for entry in
            std::fs::read_dir(path).map_err(|error| PlatformError::Io(error.to_string()))?
        {
            let entry = entry.map_err(|error| PlatformError::Io(error.to_string()))?;
            flush_tree_bottom_up(&entry.path())?;
        }
        return std::fs::File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| PlatformError::Io(error.to_string()));
    }
    Err(PlatformError::InvalidSource(path.display().to_string()))
}

#[cfg(target_os = "linux")]
pub(super) fn default_recovery_root() -> Result<std::path::PathBuf, PlatformError> {
    #[cfg(test)]
    if let Some(root) = TEST_RECOVERY_ROOT.with(|root| root.borrow().clone()) {
        return Ok(root);
    }

    let base = std::env::var_os("XDG_STATE_HOME")
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                std::path::Path::new(&home)
                    .join(".local")
                    .join("state")
                    .to_path_buf()
                    .to_string_lossy()
                    .into_owned()
                    .into()
            })
        })
        .map(std::path::PathBuf::from)
        .ok_or_else(|| PlatformError::Io("XDG_STATE_HOME and HOME are unavailable".to_string()))?;

    Ok(base.join("ottrin").join("moves"))
}

#[cfg(all(any(target_os = "linux", target_os = "windows"), test))]
thread_local! {
    static TEST_RECOVERY_ROOT: std::cell::RefCell<Option<std::path::PathBuf>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(all(any(target_os = "linux", target_os = "windows"), test))]
pub(super) fn replace_test_recovery_root(
    root: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    TEST_RECOVERY_ROOT
        .with(|configured_root| std::mem::replace(&mut *configured_root.borrow_mut(), root))
}

/// Reads retained Move recovery status from an existing configured recovery root.
///
/// This is intentionally incapable of creating an anchor, journal, transaction directory, or
/// cleanup side effect. A missing root simply means there are no durable Move transactions yet.
#[cfg(target_os = "linux")]
pub(super) fn discover_retained_move_transactions()
-> Result<Vec<move_recovery::LinuxDiscoveredMoveTransaction>, PlatformError> {
    discover_retained_move_transactions_at(&default_recovery_root()?)
}

/// Reads retained Move recovery status from an existing configured recovery root.
///
/// This is intentionally incapable of creating an anchor, journal, transaction directory, or
/// cleanup side effect. A missing root simply means there are no durable Move transactions yet.
#[cfg(target_os = "linux")]
pub(super) fn discover_retained_move_transactions_at(
    recovery_root: &Path,
) -> Result<Vec<move_recovery::LinuxDiscoveredMoveTransaction>, PlatformError> {
    match std::fs::symlink_metadata(recovery_root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    }
    let central = move_recovery::LinuxJournalDirectory::open_configured_read_only(recovery_root)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    central
        .discover_move_transactions()
        .map_err(|error| PlatformError::Io(error.to_string()))
}

#[cfg(target_os = "linux")]
#[cfg(test)]
pub(super) fn move_entry_transactional_with_test_hook(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
    hook: impl FnMut(TestPoint) -> Result<(), &'static str>,
) -> Result<MoveDisposition, PlatformError> {
    move_with_hooks(source, destination_dir, conflict, hook)
        .map(|publication| publication.disposition)
}

#[cfg(test)]
mod windows_source_tests {
    use super::{
        WINDOWS_INCOMPLETE_STAGING_DISPOSITION_FLAGS, WINDOWS_RECOVERY_LOCK_SHARE_MODE,
        WindowsCrossVolumePreflight, WindowsCrossVolumeRestartAction,
        WindowsIncompleteStagingEntryKind, WindowsJournalKind, WindowsMoveRoute,
        WindowsOverwriteRestartAction, WindowsReplaceTopology, WindowsSourceRemovalDecision,
        classify_windows_cross_volume_overwrite_restart, classify_windows_cross_volume_preflight,
        classify_windows_cross_volume_restart, classify_windows_directory_cleanup_topology,
        classify_windows_directory_slice_action, classify_windows_incomplete_directory_candidate,
        classify_windows_overwrite_restart, classify_windows_replace_topology,
        classify_windows_source_removal, complete_windows_cross_volume_source_removal_with,
        flush_windows_cross_volume_recovered_publication_with, next_windows_rename_candidate,
        parse_windows_journal_name, publish_windows_cross_volume_candidate_with,
        publish_windows_cross_volume_directory_overwrite_with, reconcile_windows_rename_candidates,
        require_windows_publication_durability, windows_cross_volume_source_kind,
        windows_destination_parent_is_pinned, windows_directory_cleanup_order,
        windows_incomplete_staging_entry_kind, windows_move_route, windows_recovery_lock_path,
        windows_same_volume_source_kind, windows_staging_candidate_matches_transaction,
        windows_stream_set_is_copy_safe,
    };

    use crate::{ConflictAction, PlatformError, move_recovery};

    #[cfg(target_os = "windows")]
    struct WindowsMoveHookReset;

    #[cfg(target_os = "windows")]
    impl Drop for WindowsMoveHookReset {
        fn drop(&mut self) {
            super::WINDOWS_MOVE_HOOK.with(|hook| *hook.borrow_mut() = None);
        }
    }

    #[test]
    fn windows_move_route_distinguishes_same_and_cross_volume_paths() {
        assert_eq!(windows_move_route(41, 41), WindowsMoveRoute::SameVolume);
        assert_eq!(windows_move_route(41, 42), WindowsMoveRoute::CrossVolume);
    }

    #[test]
    fn windows_recovery_discovery_accepts_only_exact_durable_journal_names() {
        let id = [0xab; 16];
        let encoded = "abababababababababababababababab";

        assert_eq!(
            parse_windows_journal_name(std::ffi::OsStr::new(&format!("{encoded}.journal"))),
            Some((id, WindowsJournalKind::SameVolumeOverwrite))
        );
        assert_eq!(
            parse_windows_journal_name(std::ffi::OsStr::new(&format!("{encoded}.cross-journal"))),
            Some((id, WindowsJournalKind::CrossVolume))
        );
        for invalid in [
            format!("{encoded}.journal.tmp"),
            format!("x{encoded}.journal"),
            format!("{encoded}f.journal"),
            "not-hexadecimal-but-32-bytes!!.journal".to_string(),
            "move-recovery.lock".to_string(),
        ] {
            assert_eq!(
                parse_windows_journal_name(std::ffi::OsStr::new(&invalid)),
                None
            );
        }
    }

    #[test]
    fn windows_retained_move_discovery_does_not_initialize_missing_state() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-windows-retained-move-discovery-{}",
            getrandom::u64().unwrap()
        ));
        assert!(
            super::discover_retained_windows_move_transactions_at(&root)
                .unwrap()
                .is_empty()
        );
        assert!(!root.exists());
    }

    #[test]
    fn windows_same_volume_source_kind_accepts_links_but_rejects_other_reparse_points() {
        assert_eq!(
            windows_same_volume_source_kind(true, false, false),
            Some(super::WindowsSameVolumeSourceKind::File)
        );
        assert_eq!(
            windows_same_volume_source_kind(false, true, false),
            Some(super::WindowsSameVolumeSourceKind::Directory)
        );
        assert_eq!(
            windows_same_volume_source_kind(false, false, true),
            Some(super::WindowsSameVolumeSourceKind::Link)
        );
        assert_eq!(windows_same_volume_source_kind(true, false, true), None);
        assert_eq!(windows_same_volume_source_kind(false, true, true), None);
        assert_eq!(windows_same_volume_source_kind(false, false, false), None);
    }

    #[test]
    fn windows_same_volume_overwrite_uses_rename_publication_for_non_file_entries() {
        assert_eq!(
            super::windows_same_volume_overwrite_publication(
                super::WindowsSameVolumeSourceKind::File,
                super::WindowsSameVolumeSourceKind::File,
            ),
            super::WindowsSameVolumeOverwritePublication::ReplaceFile
        );
        for kind in [
            super::WindowsSameVolumeSourceKind::Directory,
            super::WindowsSameVolumeSourceKind::Link,
        ] {
            assert_eq!(
                super::windows_same_volume_overwrite_publication(
                    kind,
                    super::WindowsSameVolumeSourceKind::File,
                ),
                super::WindowsSameVolumeOverwritePublication::RetainedRename
            );
            assert_eq!(
                super::windows_same_volume_overwrite_publication(
                    super::WindowsSameVolumeSourceKind::File,
                    kind,
                ),
                super::WindowsSameVolumeOverwritePublication::RetainedRename
            );
        }
    }

    #[test]
    fn windows_cross_volume_source_kind_accepts_supported_entries_only() {
        assert_eq!(
            windows_cross_volume_source_kind(true, false, false),
            Some(super::WindowsCrossVolumeSourceKind::File)
        );
        assert_eq!(
            windows_cross_volume_source_kind(false, true, false),
            Some(super::WindowsCrossVolumeSourceKind::Directory)
        );
        assert_eq!(
            windows_cross_volume_source_kind(false, false, true),
            Some(super::WindowsCrossVolumeSourceKind::SymbolicLink)
        );
        assert_eq!(windows_cross_volume_source_kind(true, false, true), None);
        assert_eq!(windows_cross_volume_source_kind(false, true, true), None);
        assert_eq!(windows_cross_volume_source_kind(false, false, false), None);
    }

    #[test]
    fn windows_directory_manifest_evidence_is_order_independent_and_path_bound() {
        let file = super::WindowsDirectoryManifestEntry {
            relative_path: vec![b'n' as u16, b'e' as u16, b's' as u16, b't' as u16],
            kind: super::WindowsCrossVolumeSourceKind::File,
            size: 7,
            sha256: "payload-digest".into(),
            link_is_directory: false,
        };
        let directory = super::WindowsDirectoryManifestEntry {
            relative_path: vec![b'd' as u16, b'i' as u16, b'r' as u16],
            kind: super::WindowsCrossVolumeSourceKind::Directory,
            size: 0,
            sha256: String::new(),
            link_is_directory: false,
        };

        let evidence =
            super::windows_directory_manifest_evidence(vec![file.clone(), directory.clone()])
                .expect("manifest evidence");
        let reversed = super::windows_directory_manifest_evidence(vec![directory, file.clone()])
            .expect("reversed manifest evidence");
        let renamed = super::windows_directory_manifest_evidence(vec![
            super::WindowsDirectoryManifestEntry {
                relative_path: vec![
                    b'o' as u16,
                    b't' as u16,
                    b'h' as u16,
                    b'e' as u16,
                    b'r' as u16,
                ],
                ..file
            },
        ])
        .expect("renamed manifest evidence");

        assert_eq!(evidence, reversed);
        assert_eq!(evidence.0, 7);
        assert_ne!(evidence.1, renamed.1);
    }

    #[test]
    fn windows_symbolic_link_evidence_preserves_raw_utf16_target() {
        let relative = [b'.' as u16, b'.' as u16, b'\\' as u16, b't' as u16];
        let absolute = [b'C' as u16, b':' as u16, b'\\' as u16, b't' as u16];

        let relative_evidence = super::windows_symbolic_link_evidence(&relative);
        let absolute_evidence = super::windows_symbolic_link_evidence(&absolute);

        assert_eq!(relative_evidence.0, 8);
        assert_eq!(absolute_evidence.0, 8);
        assert_ne!(relative_evidence.1, absolute_evidence.1);
    }

    #[test]
    fn windows_symbolic_link_snapshot_requires_target_and_kind_to_match() {
        let expected_target = [b'.' as u16, b'.' as u16, b'\\' as u16, b't' as u16];

        assert!(super::windows_symbolic_link_snapshot_matches(
            &expected_target,
            true,
            &expected_target,
            true,
        ));
        assert!(!super::windows_symbolic_link_snapshot_matches(
            &expected_target,
            true,
            &[b't' as u16],
            true,
        ));
        assert!(!super::windows_symbolic_link_snapshot_matches(
            &expected_target,
            true,
            &expected_target,
            false,
        ));
    }

    #[test]
    fn windows_directory_manifest_accepts_and_binds_nested_symbolic_links() {
        let link = super::WindowsDirectoryManifestEntry {
            relative_path: vec![b'l' as u16, b'i' as u16, b'n' as u16, b'k' as u16],
            kind: super::WindowsCrossVolumeSourceKind::SymbolicLink,
            size: 8,
            sha256: "link-target-digest".into(),
            link_is_directory: false,
        };
        let evidence = super::windows_directory_manifest_evidence(vec![link.clone()])
            .expect("nested symbolic link should be valid tree evidence");
        let changed_target = super::windows_directory_manifest_evidence(vec![
            super::WindowsDirectoryManifestEntry {
                sha256: "different-target-digest".into(),
                ..link.clone()
            },
        ])
        .expect("changed nested symbolic link should remain valid tree evidence");
        let changed_kind = super::windows_directory_manifest_evidence(vec![
            super::WindowsDirectoryManifestEntry {
                link_is_directory: true,
                ..link
            },
        ])
        .expect("changed nested symbolic-link kind should remain valid tree evidence");

        assert_eq!(evidence.0, 8);
        assert_ne!(evidence.1, changed_target.1);
        assert_ne!(evidence.1, changed_kind.1);
    }

    #[test]
    fn windows_directory_manifest_rejects_paths_that_escape_or_alias_tree_entries() {
        for relative_path in [
            vec![b'\\' as u16, b'a' as u16],
            vec![b'a' as u16, b'\\' as u16],
            vec![b'a' as u16, b'\\' as u16, b'\\' as u16, b'b' as u16],
            vec![b'.' as u16],
            vec![b'.' as u16, b'.' as u16],
            vec![b'a' as u16, b'\\' as u16, b'.' as u16, b'.' as u16],
        ] {
            let result = super::windows_directory_manifest_evidence(vec![
                super::WindowsDirectoryManifestEntry {
                    relative_path,
                    kind: super::WindowsCrossVolumeSourceKind::File,
                    size: 7,
                    sha256: "payload-digest".into(),
                    link_is_directory: false,
                },
            ]);
            assert!(result.is_err());
        }
    }

    #[test]
    fn windows_directory_manifest_rejects_missing_or_nondirectory_ancestors() {
        let nested_file = super::WindowsDirectoryManifestEntry {
            relative_path: vec![
                b'p' as u16,
                b'a' as u16,
                b'r' as u16,
                b'e' as u16,
                b'n' as u16,
                b't' as u16,
                b'\\' as u16,
                b'f' as u16,
                b'i' as u16,
                b'l' as u16,
                b'e' as u16,
            ],
            kind: super::WindowsCrossVolumeSourceKind::File,
            size: 7,
            sha256: "payload-digest".into(),
            link_is_directory: false,
        };

        assert!(super::windows_directory_manifest_evidence(vec![nested_file.clone()]).is_err());
        assert!(
            super::windows_directory_manifest_evidence(vec![
                super::WindowsDirectoryManifestEntry {
                    relative_path: vec![
                        b'p' as u16,
                        b'a' as u16,
                        b'r' as u16,
                        b'e' as u16,
                        b'n' as u16,
                        b't' as u16,
                    ],
                    kind: super::WindowsCrossVolumeSourceKind::File,
                    size: 1,
                    sha256: "ancestor-digest".into(),
                    link_is_directory: false,
                },
                nested_file,
            ])
            .is_err()
        );
    }

    #[test]
    fn windows_directory_slice_allows_verified_publication_but_keeps_source_removal_fail_closed() {
        assert_eq!(
            classify_windows_directory_slice_action(
                super::WindowsCrossVolumeSourceKind::Directory,
                WindowsCrossVolumeRestartAction::CopyAndVerify,
            ),
            WindowsCrossVolumeRestartAction::CopyAndVerify
        );
        assert_eq!(
            classify_windows_directory_slice_action(
                super::WindowsCrossVolumeSourceKind::Directory,
                WindowsCrossVolumeRestartAction::DiscardIncompleteStaging,
            ),
            WindowsCrossVolumeRestartAction::DiscardIncompleteStaging
        );
        assert_eq!(
            classify_windows_directory_slice_action(
                super::WindowsCrossVolumeSourceKind::File,
                WindowsCrossVolumeRestartAction::PublishStaged,
            ),
            WindowsCrossVolumeRestartAction::PublishStaged
        );
        assert_eq!(
            classify_windows_directory_slice_action(
                super::WindowsCrossVolumeSourceKind::Directory,
                WindowsCrossVolumeRestartAction::PublishStaged,
            ),
            WindowsCrossVolumeRestartAction::PublishStaged
        );
        assert_eq!(
            classify_windows_directory_slice_action(
                super::WindowsCrossVolumeSourceKind::Directory,
                WindowsCrossVolumeRestartAction::FlushPublished,
            ),
            WindowsCrossVolumeRestartAction::FlushPublished
        );
        assert_eq!(
            classify_windows_directory_slice_action(
                super::WindowsCrossVolumeSourceKind::Directory,
                WindowsCrossVolumeRestartAction::RemoveSource,
            ),
            WindowsCrossVolumeRestartAction::RemoveSource
        );
    }

    #[test]
    fn windows_directory_cleanup_claims_once_and_resumes_only_the_pinned_claim() {
        use super::WindowsDirectoryCleanupTopology;

        assert_eq!(
            classify_windows_directory_cleanup_topology(true, false),
            WindowsDirectoryCleanupTopology::ClaimSource,
        );
        assert_eq!(
            classify_windows_directory_cleanup_topology(false, true),
            WindowsDirectoryCleanupTopology::DrainClaimedSource,
        );
        assert_eq!(
            classify_windows_directory_cleanup_topology(false, false),
            WindowsDirectoryCleanupTopology::Complete,
        );
        assert_eq!(
            classify_windows_directory_cleanup_topology(true, true),
            WindowsDirectoryCleanupTopology::RetainAmbiguous,
        );
    }

    #[test]
    fn windows_directory_cleanup_accepts_only_manifest_subsets_and_orders_children_first() {
        use super::{WindowsCrossVolumeSourceKind, WindowsDirectoryManifestEntry};

        let entry = |path: &[u8], kind, size, sha256: &str| WindowsDirectoryManifestEntry {
            relative_path: path.iter().copied().map(u16::from).collect(),
            kind,
            size,
            sha256: sha256.into(),
            link_is_directory: false,
        };
        let journal = vec![
            entry(b"parent", WindowsCrossVolumeSourceKind::Directory, 0, ""),
            entry(
                b"parent\\child",
                WindowsCrossVolumeSourceKind::Directory,
                0,
                "",
            ),
            entry(
                b"parent\\child\\file",
                WindowsCrossVolumeSourceKind::File,
                7,
                "digest",
            ),
            entry(b"removed", WindowsCrossVolumeSourceKind::File, 3, "old"),
        ];
        let observed = journal[..3].to_vec();

        let order = windows_directory_cleanup_order(&journal, &observed)
            .expect("a remaining exact manifest subset is resumable");
        assert_eq!(order, vec![2, 1, 0]);

        let mut replacement = observed.clone();
        replacement[2].sha256 = "replacement".into();
        assert!(windows_directory_cleanup_order(&journal, &replacement).is_none());

        let mut added = observed;
        added.push(entry(
            b"unexpected",
            WindowsCrossVolumeSourceKind::File,
            1,
            "new",
        ));
        assert!(windows_directory_cleanup_order(&journal, &added).is_none());
    }

    #[test]
    fn windows_incomplete_private_directory_candidate_is_discarded_before_retry() {
        // A candidate created before its identity reaches the paired journal is
        // still recoverable when the exact private name can be identity-pinned
        // during cleanup. A recorded identity remains authoritative when present.
        assert_eq!(
            classify_windows_incomplete_directory_candidate(
                super::WindowsCrossVolumeSourceKind::Directory,
                true,
                true,
                true,
                true,
                true,
                true,
            ),
            Some(WindowsCrossVolumeRestartAction::DiscardIncompleteStaging),
        );
        assert_eq!(
            classify_windows_incomplete_directory_candidate(
                super::WindowsCrossVolumeSourceKind::Directory,
                true,
                true,
                false,
                true,
                true,
                true,
            ),
            None,
        );
        assert_eq!(
            classify_windows_incomplete_directory_candidate(
                super::WindowsCrossVolumeSourceKind::Directory,
                true,
                true,
                true,
                false,
                false,
                true,
            ),
            Some(WindowsCrossVolumeRestartAction::DiscardIncompleteStaging),
        );
        assert_eq!(
            classify_windows_incomplete_directory_candidate(
                super::WindowsCrossVolumeSourceKind::Directory,
                true,
                true,
                true,
                true,
                false,
                true,
            ),
            None,
        );
        assert_eq!(
            classify_windows_incomplete_directory_candidate(
                super::WindowsCrossVolumeSourceKind::File,
                true,
                true,
                true,
                true,
                true,
                true,
            ),
            None,
        );
        assert_eq!(
            classify_windows_incomplete_directory_candidate(
                super::WindowsCrossVolumeSourceKind::Directory,
                false,
                true,
                true,
                true,
                true,
                true,
            ),
            None,
        );
    }

    #[test]
    fn windows_incomplete_staging_cleanup_is_bound_to_its_transaction_directory() {
        let transaction = std::path::Path::new(r"D:\recovery\.ottrin-move-abc");

        assert!(windows_staging_candidate_matches_transaction(
            transaction,
            &transaction.join(super::WINDOWS_CROSS_VOLUME_CANDIDATE_NAME),
        ));
        assert!(!windows_staging_candidate_matches_transaction(
            transaction,
            std::path::Path::new(r"D:\unrelated\candidate"),
        ));
        assert_eq!(WINDOWS_INCOMPLETE_STAGING_DISPOSITION_FLAGS, 0x13);
        assert_eq!(windows_incomplete_staging_entry_kind(false, false), None,);
        assert_eq!(
            windows_incomplete_staging_entry_kind(false, true),
            Some(WindowsIncompleteStagingEntryKind::Directory),
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_native_same_volume_overwrite_rejects_replaced_selection() {
        let fixture = crate::tests::TestDir::new("windows-overwrite-source-handoff");
        let source = fixture.path().join("source.txt");
        let parked_original = fixture.path().join("original.txt");
        let destination = fixture.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        let target = destination.join("source.txt");
        std::fs::write(&source, b"selected original").unwrap();
        std::fs::write(&target, b"displaced target").unwrap();
        let selected = super::move_recovery::open_reparse_safe_entry_for_rename(&source).unwrap();
        let selected_identity =
            super::move_recovery::stable_identity_from_handle(&selected).unwrap();
        std::fs::rename(&source, &parked_original).unwrap();
        std::fs::write(&source, b"external replacement").unwrap();
        let _lock = super::acquire_windows_recovery_lock().unwrap();
        let result =
            super::start_windows_same_volume_overwrite(&source, &destination, &target, selected);
        assert!(
            matches!(
                &result,
                Err(super::PlatformError::CommittedCleanupPending { .. })
            ),
            "unexpected overwrite result: {:?}",
            result.as_ref().err()
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"external replacement");
        assert_eq!(
            std::fs::read(&parked_original).unwrap(),
            b"selected original"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"displaced target");
        assert_eq!(
            super::move_recovery::stable_identity_of_entry(&parked_original).unwrap(),
            selected_identity
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_native_same_volume_directory_move_preserves_the_complete_tree() {
        struct Fixture(std::path::PathBuf);

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut fixture_id = [0_u8; 16];
        getrandom::fill(&mut fixture_id).expect("same-volume fixture id");
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "ottrin-native-same-volume-{}",
            super::encode_transaction_id(fixture_id)
        )));
        std::fs::create_dir(&fixture.0).expect("create same-volume directory fixture");
        let source = fixture.0.join("source-tree");
        let destination_root = fixture.0.join("destination");
        let nested = source.join("nested");
        std::fs::create_dir_all(&nested).expect("create nested source tree");
        std::fs::create_dir(&destination_root).expect("create destination directory");
        std::fs::write(nested.join("payload.bin"), b"same-volume directory payload")
            .expect("write nested payload");

        let publication =
            super::move_entry_transactional(&source, &destination_root, ConflictAction::Rename)
                .expect("same-volume directory Move should complete");
        let destination = destination_root.join("source-tree");

        assert_eq!(publication.disposition, crate::MoveDisposition::Moved);
        assert_eq!(
            publication.published_path.as_deref(),
            Some(destination.as_path())
        );
        assert!(
            !source.exists(),
            "the original directory name must be absent"
        );
        assert_eq!(
            std::fs::read(destination.join("nested").join("payload.bin"))
                .expect("read moved nested payload"),
            b"same-volume directory payload"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_native_completed_overwrite_does_not_own_reused_source_or_target_names() {
        let fixture = crate::tests::TestDir::new("completed-overwrite-name-reuse");
        let source = fixture.path().join("note.txt");
        let destination = fixture.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        let target = destination.join("note.txt");
        std::fs::write(&source, b"first source").unwrap();
        std::fs::write(&target, b"displaced destination").unwrap();
        let first =
            super::move_entry_transactional(&source, &destination, ConflictAction::Overwrite)
                .unwrap();
        let retained = first.retained_displaced_path.unwrap();
        let root = super::windows_recovery_root().unwrap();
        let journal = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "journal")
            })
            .unwrap();
        let record = super::load_windows_overwrite_record(&journal).unwrap();
        let local = record
            .transaction_path()
            .unwrap()
            .join(super::WINDOWS_LOCAL_JOURNAL_NAME);
        let central_bytes = std::fs::read(&journal).unwrap();
        let local_bytes = std::fs::read(&local).unwrap();

        std::fs::write(&source, b"later source").unwrap();
        let second = super::move_entry_transactional(&source, &destination, ConflictAction::Rename)
            .expect("a completed Replace must not reserve the old source name");
        assert_eq!(
            std::fs::read(second.published_path.unwrap()).unwrap(),
            b"later source"
        );
        assert!(!source.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"first source");

        std::fs::remove_file(&target).unwrap();
        std::fs::write(&target, b"reused destination").unwrap();
        let unrelated = fixture.path().join("unrelated.txt");
        std::fs::write(&unrelated, b"unrelated source").unwrap();
        let third =
            super::move_entry_transactional(&unrelated, &destination, ConflictAction::Rename)
                .expect("a completed Replace must not constrain later destination changes");
        assert_eq!(
            std::fs::read(third.published_path.unwrap()).unwrap(),
            b"unrelated source"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"reused destination");
        assert_eq!(std::fs::read(&retained).unwrap(), b"displaced destination");
        assert_eq!(std::fs::read(&journal).unwrap(), central_bytes);
        assert_eq!(std::fs::read(&local).unwrap(), local_bytes);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_native_completed_overwrite_keeps_ambiguous_private_evidence_blocked() {
        for inject_replacement in [false, true] {
            let fixture = crate::tests::TestDir::new("completed-overwrite-ambiguous");
            let source = fixture.path().join("note.txt");
            let destination = fixture.path().join("destination");
            std::fs::create_dir(&destination).unwrap();
            let target = destination.join("note.txt");
            std::fs::write(&source, b"first source").unwrap();
            std::fs::write(&target, b"displaced destination").unwrap();
            let first =
                super::move_entry_transactional(&source, &destination, ConflictAction::Overwrite)
                    .unwrap();
            let retained = first.retained_displaced_path.unwrap();
            let journal = std::fs::read_dir(super::windows_recovery_root().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "journal")
                })
                .unwrap();
            let mut record = super::load_windows_overwrite_record(&journal).unwrap();
            if inject_replacement {
                std::fs::write(record.replacement_path().unwrap(), b"unowned private entry")
                    .unwrap();
            } else {
                record.phase = super::WindowsOverwritePhase::SourceClaimed;
                // Leave the local Published journal unchanged.
                super::write_windows_record_file(&journal, &record, false).unwrap();
            }
            std::fs::write(&source, b"later source").unwrap();
            assert!(
                super::move_entry_transactional(&source, &destination, ConflictAction::Rename)
                    .is_err()
            );
            assert_eq!(std::fs::read(&source).unwrap(), b"later source");
            assert_eq!(std::fs::read(&target).unwrap(), b"first source");
            assert_eq!(std::fs::read(&retained).unwrap(), b"displaced destination");
            if inject_replacement {
                assert_eq!(
                    std::fs::read(record.replacement_path().unwrap()).unwrap(),
                    b"unowned private entry"
                );
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_native_named_move_preserves_original_name_and_occupied_entries() {
        for directory in [false, true] {
            let fixture = crate::tests::TestDir::new("named-move");
            let destination = fixture.path().join("destination");
            std::fs::create_dir(&destination).unwrap();
            let source = fixture.path().join("incoming (1)");
            let content = if directory {
                std::fs::create_dir(&source).unwrap();
                source.join("child.txt")
            } else {
                source.clone()
            };
            std::fs::write(&content, b"selected original").unwrap();
            let target = destination.join("original");
            std::fs::write(&target, b"later occupant").unwrap();
            let identity = move_recovery::stable_identity_of_entry(&source).unwrap();
            let skipped = super::move_windows_entry_to_named_destination(
                &source,
                &destination,
                std::ffi::OsStr::new("original"),
                ConflictAction::Skip,
            )
            .unwrap();
            assert_eq!(skipped.disposition, crate::MoveDisposition::Skipped);
            assert_eq!(std::fs::read(&content).unwrap(), b"selected original");
            assert_eq!(std::fs::read(&target).unwrap(), b"later occupant");
            std::fs::rename(&target, destination.join("parked-occupant")).unwrap();
            let moved = super::move_windows_entry_to_named_destination(
                &source,
                &destination,
                std::ffi::OsStr::new("original"),
                ConflictAction::Skip,
            )
            .unwrap();
            assert_eq!(moved.published_path.as_deref(), Some(target.as_path()));
            assert_eq!(
                move_recovery::stable_identity_of_entry(&target).unwrap(),
                identity
            );
            assert_eq!(
                std::fs::read(if directory {
                    target.join("child.txt")
                } else {
                    target
                })
                .unwrap(),
                b"selected original"
            );
            assert_eq!(
                std::fs::read(destination.join("parked-occupant")).unwrap(),
                b"later occupant"
            );
            assert!(!source.exists());
            assert!(!destination.join("incoming (1)").exists());
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_native_named_move_rejects_non_leaf_targets_before_mutation() {
        let fixture = crate::tests::TestDir::new("named-move-invalid");
        let source = fixture.path().join("source.txt");
        let destination = fixture.path().join("destination");
        std::fs::write(&source, b"untouched source").unwrap();
        std::fs::create_dir(&destination).unwrap();
        for name in [
            "",
            ".",
            "..",
            r"..\escape",
            r"D:\escape",
            "note:stream",
            "bad\0",
        ] {
            assert!(matches!(
                super::move_windows_entry_to_named_destination(
                    &source,
                    &destination,
                    std::ffi::OsStr::new(name),
                    ConflictAction::Skip
                ),
                Err(PlatformError::InvalidDestination(_))
            ));
            assert_eq!(std::fs::read(&source).unwrap(), b"untouched source");
            assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 0);
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an owned second NTFS volume in OTTRIN_WINDOWS_CROSS_VOLUME_ROOT"]
    fn windows_native_cross_volume_named_move_preserves_exact_original_name() {
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for directory in [false, true] {
            let sources = crate::tests::TestDir::new("named-cross-source");
            let destination = crate::tests::TestDir::new_at(&volume, "named-cross-destination");
            let _ = super::replace_test_recovery_root(Some(sources.path().join("move-state")));
            let source = sources.path().join("incoming (1)");
            let content = if directory {
                std::fs::create_dir(&source).unwrap();
                source.join("child.txt")
            } else {
                source.clone()
            };
            std::fs::write(&content, b"selected original").unwrap();
            let target = destination.path().join("original");
            std::fs::write(&target, b"later occupant").unwrap();
            assert_ne!(
                move_recovery::stable_identity_of_entry(&source)
                    .unwrap()
                    .volume_serial_number(),
                move_recovery::stable_identity_of_entry(destination.path())
                    .unwrap()
                    .volume_serial_number()
            );
            let skipped = super::move_windows_entry_to_named_destination(
                &source,
                destination.path(),
                std::ffi::OsStr::new("original"),
                ConflictAction::Skip,
            )
            .unwrap();
            assert_eq!(skipped.disposition, crate::MoveDisposition::Skipped);
            assert_eq!(std::fs::read(&content).unwrap(), b"selected original");
            assert_eq!(std::fs::read(&target).unwrap(), b"later occupant");
            std::fs::rename(&target, destination.path().join("parked-occupant")).unwrap();
            let moved = super::move_windows_entry_to_named_destination(
                &source,
                destination.path(),
                std::ffi::OsStr::new("original"),
                ConflictAction::Skip,
            )
            .unwrap();
            assert_eq!(moved.published_path.as_deref(), Some(target.as_path()));
            assert_eq!(
                std::fs::read(if directory {
                    target.join("child.txt")
                } else {
                    target
                })
                .unwrap(),
                b"selected original"
            );
            assert!(!source.exists());
            assert!(!destination.path().join("incoming (1)").exists());
            assert_eq!(
                std::fs::read(destination.path().join("parked-occupant")).unwrap(),
                b"later occupant"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an owned second NTFS volume in OTTRIN_WINDOWS_CROSS_VOLUME_ROOT"]
    fn windows_native_cross_volume_named_move_recovery_keeps_requested_target() {
        if std::env::var_os("OTTRIN_NAMED_MOVE_EXIT_CHILD").is_some() {
            let source =
                std::path::PathBuf::from(std::env::var_os("OTTRIN_NAMED_MOVE_SOURCE").unwrap());
            let destination = std::path::PathBuf::from(
                std::env::var_os("OTTRIN_NAMED_MOVE_DESTINATION").unwrap(),
            );
            super::move_windows_entry_to_named_destination(
                &source,
                &destination,
                std::ffi::OsStr::new("original.txt"),
                ConflictAction::Skip,
            )
            .unwrap();
            panic!("child did not reach process-exit boundary");
        }
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for boundary in ["CandidateUnlocked", "CandidatePublished"] {
            let sources = crate::tests::TestDir::new("named-cross-recovery-source");
            let destination =
                crate::tests::TestDir::new_at(&volume, "named-cross-recovery-destination");
            let state = sources.path().join("move-state");
            let _ = super::replace_test_recovery_root(Some(state.clone()));
            let source = sources.path().join("incoming (1).txt");
            std::fs::create_dir(&source).unwrap();
            std::fs::write(source.join("child.txt"), b"selected original").unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "move_transaction::windows_source_tests::windows_native_cross_volume_named_move_recovery_keeps_requested_target", "--nocapture"])
                .env("OTTRIN_NAMED_MOVE_EXIT_CHILD", "1")
                .env("OTTRIN_NAMED_MOVE_SOURCE", &source)
                .env("OTTRIN_NAMED_MOVE_DESTINATION", destination.path())
                .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT", &state)
                .env("OTTRIN_TEST_WINDOWS_MOVE_EXIT_BOUNDARY", boundary)
                .status().unwrap();
            assert_eq!(status.code(), Some(87));
            let journal = std::fs::read_dir(&state)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "cross-journal")
                })
                .unwrap();
            let record = super::load_windows_cross_volume_record(&journal).unwrap();
            let local = record
                .transaction_path()
                .unwrap()
                .join(super::WINDOWS_LOCAL_JOURNAL_NAME);
            let central_before = std::fs::read(&journal).unwrap();
            let local_before = std::fs::read(&local).unwrap();
            assert!(matches!(
                super::move_windows_entry_to_named_destination(
                    &source,
                    destination.path(),
                    std::ffi::OsStr::new("different.txt"),
                    ConflictAction::Skip
                ),
                Err(PlatformError::ClaimedRecoveryPending { .. })
            ));
            assert_eq!(
                std::fs::read(source.join("child.txt")).unwrap(),
                b"selected original"
            );
            assert_eq!(std::fs::read(&journal).unwrap(), central_before);
            assert_eq!(std::fs::read(&local).unwrap(), local_before);
            assert!(!destination.path().join("different.txt").exists());
            let completed = super::move_windows_entry_to_named_destination(
                &source,
                destination.path(),
                std::ffi::OsStr::new("original.txt"),
                ConflictAction::Skip,
            )
            .unwrap();
            assert_eq!(
                completed.published_path.as_deref(),
                Some(destination.path().join("original.txt").as_path())
            );
            assert_eq!(
                std::fs::read(destination.path().join("original.txt").join("child.txt")).unwrap(),
                b"selected original"
            );
            assert!(!source.exists());
            assert!(!destination.path().join("incoming (1).txt").exists());
        }
    }

    #[cfg(target_os = "windows")]
    struct CompletedCrossMoveFixture {
        // TestDir restores the thread-local root: drop destination before source.
        destination: crate::tests::TestDir,
        sources: crate::tests::TestDir,
        source: std::path::PathBuf,
        target: std::path::PathBuf,
        retained: std::path::PathBuf,
        journal: std::path::PathBuf,
        local: std::path::PathBuf,
        record: super::WindowsCrossVolumeRecord,
    }

    #[cfg(target_os = "windows")]
    fn completed_cross_move_fixture() -> CompletedCrossMoveFixture {
        let volume = std::path::PathBuf::from(
            std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").expect("owned second volume"),
        );
        let sources = crate::tests::TestDir::new("cross-completed-source");
        let destination = crate::tests::TestDir::new_at(&volume, "cross-completed-destination");
        let recovery_root = sources.path().join("move-state");
        let _ = super::replace_test_recovery_root(Some(recovery_root.clone()));
        let source = sources.path().join("note.txt");
        let target = destination.path().join("note.txt");
        std::fs::write(&source, b"first source").unwrap();
        std::fs::write(&target, b"displaced destination").unwrap();
        let first =
            super::move_entry_transactional(&source, destination.path(), ConflictAction::Overwrite)
                .unwrap();
        let retained = first.retained_displaced_path.unwrap();
        let journal = std::fs::read_dir(&recovery_root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "cross-journal")
            })
            .unwrap();
        let record = super::load_windows_cross_volume_record(&journal).unwrap();
        assert_ne!(
            record.source_identity.volume_serial_number(),
            record.destination_parent_identity.volume_serial_number()
        );
        let local = record
            .transaction_path()
            .unwrap()
            .join(super::WINDOWS_LOCAL_JOURNAL_NAME);
        assert_eq!(record.phase, super::WindowsCrossVolumePhase::SourceRemoved);
        assert_eq!(
            super::load_windows_cross_volume_record(&local).unwrap(),
            record
        );
        CompletedCrossMoveFixture {
            destination,
            sources,
            source,
            target,
            retained,
            journal,
            local,
            record,
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an owned second native NTFS volume in OTTRIN_WINDOWS_CROSS_VOLUME_ROOT"]
    fn windows_native_cross_volume_completed_overwrite_allows_later_name_reuse() {
        let fixture = completed_cross_move_fixture();
        let source = &fixture.source;
        let target = &fixture.target;
        let destination = &fixture.destination;
        let sources = &fixture.sources;
        let retained = &fixture.retained;
        let journal = &fixture.journal;
        let local = &fixture.local;
        let central_bytes = std::fs::read(journal).unwrap();
        let local_bytes = std::fs::read(local).unwrap();

        std::fs::write(source, b"later source").unwrap();
        let second =
            super::move_entry_transactional(source, destination.path(), ConflictAction::Rename)
                .expect("completed cross-volume Replace must not reserve its old source name");
        assert_eq!(
            std::fs::read(second.published_path.unwrap()).unwrap(),
            b"later source"
        );
        assert!(!source.exists());
        assert_eq!(std::fs::read(target).unwrap(), b"first source");

        std::fs::remove_file(target).unwrap();
        std::fs::write(target, b"reused destination").unwrap();
        let unrelated = sources.path().join("unrelated.txt");
        std::fs::write(&unrelated, b"unrelated source").unwrap();
        let third =
            super::move_entry_transactional(&unrelated, destination.path(), ConflictAction::Rename)
                .expect("completed cross-volume Replace must not constrain later target changes");
        assert_eq!(
            std::fs::read(third.published_path.unwrap()).unwrap(),
            b"unrelated source"
        );
        assert_eq!(std::fs::read(target).unwrap(), b"reused destination");
        assert_eq!(std::fs::read(retained).unwrap(), b"displaced destination");
        assert_eq!(std::fs::read(journal).unwrap(), central_bytes);
        assert_eq!(std::fs::read(local).unwrap(), local_bytes);
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an owned second native NTFS volume in OTTRIN_WINDOWS_CROSS_VOLUME_ROOT"]
    fn windows_native_cross_volume_completed_overwrite_converges_each_terminal_journal_gap() {
        for central_is_ahead in [false, true] {
            let fixture = completed_cross_move_fixture();
            let mut older = fixture.record.clone();
            older.phase = super::WindowsCrossVolumePhase::Published;
            let path = if central_is_ahead {
                &fixture.local
            } else {
                &fixture.journal
            };
            super::write_windows_record_file(path, &older, false).unwrap();
            std::fs::write(&fixture.source, b"later source").unwrap();
            let publication = super::move_entry_transactional(
                &fixture.source,
                fixture.destination.path(),
                ConflictAction::Rename,
            )
            .expect("a source-removal completion marker must survive either paired journal gap");
            assert_eq!(
                std::fs::read(publication.published_path.unwrap()).unwrap(),
                b"later source"
            );
            assert!(!fixture.source.exists());
            assert_eq!(std::fs::read(&fixture.target).unwrap(), b"first source");
            assert_eq!(
                std::fs::read(&fixture.retained).unwrap(),
                b"displaced destination"
            );
            assert_eq!(
                super::load_windows_cross_volume_record(&fixture.journal).unwrap(),
                fixture.record
            );
            assert_eq!(
                super::load_windows_cross_volume_record(&fixture.local).unwrap(),
                fixture.record
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an owned second native NTFS volume in OTTRIN_WINDOWS_CROSS_VOLUME_ROOT"]
    fn windows_native_cross_volume_completed_overwrite_refuses_ambiguous_private_evidence() {
        for mutation in 0..4 {
            let fixture = completed_cross_move_fixture();
            let transaction = fixture.record.transaction_path().unwrap();
            let claim =
                super::windows_directory_source_claim_path(fixture.sources.path(), &fixture.record)
                    .unwrap();
            match mutation {
                0 => {
                    let mut changed = fixture.record.clone();
                    changed.source_size += 1;
                    super::write_windows_record_file(&fixture.local, &changed, false).unwrap();
                }
                1 => std::fs::write(
                    fixture.record.candidate_path().unwrap(),
                    b"unknown candidate",
                )
                .unwrap(),
                2 => {
                    std::fs::rename(&fixture.retained, transaction.join("original-retained"))
                        .unwrap();
                    std::fs::write(&fixture.retained, b"unknown retained entry").unwrap();
                }
                3 => std::fs::write(&claim, b"unknown source claim").unwrap(),
                _ => unreachable!(),
            }
            let journal_before = std::fs::read(&fixture.journal).unwrap();
            let local_before = std::fs::read(&fixture.local).unwrap();
            std::fs::write(&fixture.source, b"later source").unwrap();
            assert!(
                super::move_entry_transactional(
                    &fixture.source,
                    fixture.destination.path(),
                    ConflictAction::Rename
                )
                .is_err()
            );
            assert_eq!(std::fs::read(&fixture.source).unwrap(), b"later source");
            assert_eq!(std::fs::read(&fixture.target).unwrap(), b"first source");
            assert_eq!(std::fs::read(&fixture.journal).unwrap(), journal_before);
            assert_eq!(std::fs::read(&fixture.local).unwrap(), local_before);
            match mutation {
                1 => assert_eq!(
                    std::fs::read(fixture.record.candidate_path().unwrap()).unwrap(),
                    b"unknown candidate"
                ),
                2 => {
                    assert_eq!(
                        std::fs::read(&fixture.retained).unwrap(),
                        b"unknown retained entry"
                    );
                    assert_eq!(
                        std::fs::read(transaction.join("original-retained")).unwrap(),
                        b"displaced destination"
                    );
                }
                3 => assert_eq!(std::fs::read(claim).unwrap(), b"unknown source claim"),
                _ => assert_eq!(
                    std::fs::read(&fixture.retained).unwrap(),
                    b"displaced destination"
                ),
            }
        }
    }

    /// Two invocations separated by an orchestrator-owned guest hard power cut.
    /// The paused prepare invocation must never be allowed to return normally.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an owned guest power cut and prepare/recover fixture orchestration"]
    fn windows_native_guest_power_cut_recovers_cross_volume_retained_overwrite() {
        use std::io::Write;
        let mode = std::env::var("OTTRIN_NATIVE_MOVE_POWER_CUT_MODE").unwrap();
        assert!(matches!(mode.as_str(), "prepare" | "recover"));
        let boundary = std::env::var("OTTRIN_NATIVE_MOVE_POWER_CUT_BOUNDARY").unwrap();
        let (pause_at, directory) = match boundary.as_str() {
            "BeforeSourceCleanup" => (super::WindowsMoveBoundary::BeforeSourceCleanup, false),
            "SourceClaimed" => (super::WindowsMoveBoundary::SourceClaimed, true),
            _ => panic!("unsupported power-cut boundary"),
        };
        let read = |entry: &std::path::Path| {
            if directory {
                assert!(entry.join("empty").is_dir());
                std::fs::read(entry.join("nested/payload")).unwrap()
            } else {
                std::fs::read(entry).unwrap()
            }
        };
        let root = std::path::PathBuf::from(
            std::env::var_os("OTTRIN_NATIVE_MOVE_POWER_CUT_ROOT").unwrap(),
        );
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        assert!(root.is_absolute() && volume.is_absolute());
        let destination = volume.join("ottrin-power-cut-overwrite");
        let source = root.join("entry");
        let target = destination.join("entry");
        let recovery = root.join("move-state");
        assert_eq!(super::windows_recovery_root().unwrap(), recovery);
        // Only roots created by prepare are removed by the external orchestrator.
        // They intentionally survive the killed process and failed recovery for
        // byte/journal diagnosis; orchestration must clean them on every outcome.
        if mode == "prepare" {
            std::fs::create_dir(&root).expect("fresh owned power-cut root");
            std::fs::create_dir(&destination).expect("fresh owned destination root");
            let create = |entry: &std::path::Path, bytes: &[u8]| {
                let payload = if directory {
                    std::fs::create_dir(entry).unwrap();
                    std::fs::create_dir(entry.join("nested")).unwrap();
                    std::fs::create_dir(entry.join("empty")).unwrap();
                    entry.join("nested/payload")
                } else {
                    entry.to_path_buf()
                };
                let mut file = std::fs::File::create(&payload).unwrap();
                file.write_all(bytes).unwrap();
                file.sync_all().unwrap();
                if directory {
                    for path in [entry.join("nested"), entry.to_path_buf()] {
                        move_recovery::open_reparse_safe_directory_for_rename(&path)
                            .unwrap()
                            .sync_all()
                            .unwrap();
                    }
                }
            };
            create(&source, b"power-cut source bytes");
            create(&target, b"power-cut displaced bytes");
            let identity = move_recovery::stable_identity_of_entry(&target).unwrap();
            let mut manifest = std::fs::File::create(root.join("displaced-identity.json")).unwrap();
            manifest
                .write_all(&serde_json::to_vec(&identity).unwrap())
                .unwrap();
            manifest.sync_all().unwrap();
            move_recovery::open_reparse_safe_directory_for_rename(&root)
                .unwrap()
                .sync_all()
                .unwrap();
            let ready = root.join("ready");
            let reset = WindowsMoveHookReset;
            super::WINDOWS_MOVE_HOOK.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move |boundary, _| {
                    if boundary == pause_at {
                        let mut file = std::fs::File::create(&ready).unwrap();
                        file.write_all(format!("{pause_at:?}").as_bytes()).unwrap();
                        file.sync_all().unwrap();
                        move_recovery::open_reparse_safe_directory_for_rename(
                            ready.parent().unwrap(),
                        )
                        .unwrap()
                        .sync_all()
                        .unwrap();
                        // Bounded even if orchestration fails to cut the guest.
                        std::thread::sleep(std::time::Duration::from_secs(90));
                        panic!("orchestrator did not cut the guest at the ready boundary");
                    }
                }));
            });
            let result =
                super::move_entry_transactional(&source, &destination, ConflictAction::Overwrite);
            drop(reset);
            panic!("prepare unexpectedly returned: {:?}", result.as_ref().err());
        }
        assert_eq!(
            std::fs::read(root.join("ready")).unwrap(),
            boundary.as_bytes()
        );
        let expected: move_recovery::WindowsStableIdentity =
            serde_json::from_slice(&std::fs::read(root.join("displaced-identity.json")).unwrap())
                .unwrap();
        let transactions = super::discover_retained_windows_move_transactions().unwrap();
        assert_eq!(transactions.len(), 1);
        let id = transactions[0].transaction_id;
        let journal = super::windows_cross_volume_journal_path(&recovery, id);
        let record = super::load_windows_cross_volume_record(&journal).unwrap();
        let retained = record.retained_path().unwrap().unwrap();
        let claimed = super::windows_directory_source_claim_path(&root, &record).unwrap();
        if directory {
            assert!(!source.exists());
            assert_eq!(read(&claimed), b"power-cut source bytes");
            assert_eq!(
                move_recovery::stable_identity_of_entry(&claimed).unwrap(),
                record.source_identity
            );
        } else {
            assert_eq!(read(&source), b"power-cut source bytes");
        }
        assert_eq!(read(&target), b"power-cut source bytes");
        assert_eq!(read(&retained), b"power-cut displaced bytes");
        assert_eq!(
            move_recovery::stable_identity_of_entry(&retained).unwrap(),
            expected
        );
        super::recover_retained_windows_move_transaction(id).unwrap();
        assert!(!source.exists());
        if directory {
            assert!(!claimed.exists());
        }
        assert_eq!(read(&target), b"power-cut source bytes");
        assert_eq!(read(&retained), b"power-cut displaced bytes");
        assert_eq!(
            move_recovery::stable_identity_of_entry(&retained).unwrap(),
            expected
        );
        super::recover_retained_windows_move_transaction(id).unwrap();
        assert!(!source.exists());
        if directory {
            assert!(!claimed.exists());
        }
        assert_eq!(read(&target), b"power-cut source bytes");
        assert_eq!(read(&retained), b"power-cut displaced bytes");
        assert_eq!(
            move_recovery::stable_identity_of_entry(&retained).unwrap(),
            expected
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_regular_file_move_preserves_verified_bytes() {
        use crate::PlatformOps;
        assert!(
            std::env::var_os("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT").is_some(),
            "set OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT to an isolated empty fixture root"
        );
        let destination_volume = std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT")
            .map(std::path::PathBuf::from)
            .expect("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT must name a second Windows volume");
        let source_fixture = crate::tests::TestDir::new("cross-volume-public-source");
        let destination_fixture =
            crate::tests::TestDir::new_at(&destination_volume, "cross-volume-public-destination");
        let source_root = source_fixture.path();
        let destination_root = destination_fixture.path();
        let source = source_root.join("payload.bin");
        let destination = destination_root.join("payload.bin");
        let payload = b"Ottrin native cross-volume verified payload\0\xff";
        std::fs::write(&source, payload).expect("write source payload");

        let source_handle = move_recovery::open_reparse_safe_entry_for_rename(&source)
            .expect("open source identity handle");
        let destination_handle =
            move_recovery::open_reparse_safe_directory_for_rename(destination_root)
                .expect("open destination directory handle");
        let source_identity = move_recovery::stable_identity_from_handle(&source_handle)
            .expect("read source identity");
        let destination_identity = move_recovery::stable_identity_from_handle(&destination_handle)
            .expect("read destination identity");
        assert_ne!(
            source_identity.volume_serial_number(),
            destination_identity.volume_serial_number(),
            "native test paths must reside on different Windows volumes"
        );
        drop(source_handle);
        drop(destination_handle);

        let (disposition, receipt) = crate::DefaultPlatform
            .execute_move_with_undo_receipt(&source, destination_root, ConflictAction::Rename)
            .expect("public cross-volume Move should complete");
        assert_eq!(disposition, crate::MoveDisposition::Moved);
        assert!(
            receipt.is_none(),
            "Windows must not advertise unsupported Move undo"
        );
        assert!(
            !source.exists(),
            "source must be removed only after publication"
        );
        assert_eq!(
            std::fs::read(&destination).expect("read destination"),
            payload
        );

        let batch_source = source_root.join("batch.bin");
        std::fs::write(&batch_source, payload).unwrap();
        let (_, dispositions) = crate::DefaultPlatform
            .execute_command_with_move_dispositions(&ottrin_core::FileCommand::Move {
                sources: vec![batch_source.clone()],
                destination: destination_root.to_path_buf(),
                conflict: ConflictAction::Rename,
            })
            .expect("public batch cross-volume Move should complete");
        assert_eq!(dispositions, vec![crate::MoveDisposition::Moved]);
        assert!(!batch_source.exists());
        assert_eq!(
            std::fs::read(destination_root.join("batch.bin")).unwrap(),
            payload
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_replace_batch_moves_unoccupied_files_and_trees() {
        use crate::PlatformOps;
        let volume = std::path::PathBuf::from(
            std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT")
                .expect("set a second native Windows volume"),
        );
        let sources = crate::tests::TestDir::new("replace-batch-source");
        let destination = crate::tests::TestDir::new_at(&volume, "replace-batch-destination");
        let _recovery = super::replace_test_recovery_root(Some(sources.path().join("move-state")));
        let file = sources.path().join("file.txt");
        let tree = sources.path().join("tree");
        std::fs::write(&file, b"new file").unwrap();
        std::fs::create_dir_all(tree.join("nested")).unwrap();
        std::fs::write(tree.join("nested/payload.txt"), b"new tree").unwrap();
        let existing = sources.path().join("existing.txt");
        std::fs::write(&existing, b"replacement").unwrap();
        std::fs::write(destination.path().join("existing.txt"), b"displaced").unwrap();

        let (_, dispositions) = crate::DefaultPlatform
            .execute_command_with_move_dispositions(&ottrin_core::FileCommand::Move {
                sources: vec![existing.clone(), file.clone(), tree.clone()],
                destination: destination.path().to_path_buf(),
                conflict: ConflictAction::Overwrite,
            })
            .expect("Replace batch must also move its nonconflicting entries");
        assert_eq!(dispositions, vec![crate::MoveDisposition::Moved; 3]);
        assert!(!existing.exists() && !file.exists() && !tree.exists());
        for (path, bytes) in [
            ("existing.txt", b"replacement".as_slice()),
            ("file.txt", b"new file".as_slice()),
            ("tree/nested/payload.txt", b"new tree".as_slice()),
        ] {
            assert_eq!(std::fs::read(destination.path().join(path)).unwrap(), bytes);
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_move_into_modify_only_destination() {
        use crate::PlatformOps;
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        let source_fixture = crate::tests::TestDir::new("modify-only-source");
        let destination_fixture = crate::tests::TestDir::new_at(&volume, "modify-only-destination");
        // Application recovery state lives in the user's private profile, not the shared destination.
        let _ = super::replace_test_recovery_root(Some(source_fixture.path().join("move-state")));
        let source = source_fixture.path().join("payload.txt");
        std::fs::write(&source, b"move with Modify destination access").unwrap();
        let user = std::env::var("USERNAME").unwrap();
        let acl = std::process::Command::new("icacls.exe")
            .arg(destination_fixture.path())
            .args(["/inheritance:r", "/grant:r", &format!("{user}:(OI)(CI)M")])
            .output()
            .unwrap();
        assert!(acl.status.success(), "Modify ACL setup failed: {acl:?}");
        let (disposition, receipt) = crate::DefaultPlatform
            .execute_move_with_undo_receipt(
                &source,
                destination_fixture.path(),
                ConflictAction::Rename,
            )
            .expect("Move into a Modify-only destination");
        assert_eq!(disposition, crate::MoveDisposition::Moved);
        assert!(receipt.is_none());
        assert!(!source.exists());
        assert_eq!(
            std::fs::read(destination_fixture.path().join("payload.txt")).unwrap(),
            b"move with Modify destination access"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_symbolic_link_preserves_raw_target_without_dereferencing() {
        use std::os::windows::fs::symlink_file;

        assert!(
            std::env::var_os("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT").is_some(),
            "set OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT to an isolated empty fixture root"
        );
        let destination_volume = std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT")
            .map(std::path::PathBuf::from)
            .expect("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT must name a second Windows volume");
        let mut transaction_id = [0_u8; 16];
        getrandom::fill(&mut transaction_id).expect("native test transaction id");
        let test_name = format!(
            "ottrin-native-cross-volume-link-{}",
            super::encode_transaction_id(transaction_id)
        );
        let source_root = std::env::temp_dir().join(&test_name);
        let destination_root = destination_volume.join(&test_name);
        std::fs::create_dir(&source_root).expect("create source test directory");
        std::fs::create_dir(&destination_root).expect("create destination test directory");
        let raw_target = std::path::PathBuf::from("missing-target.txt");
        let source = source_root.join("payload-link");
        let destination = destination_root.join("payload-link");
        symlink_file(&raw_target, &source).expect("create dangling source symbolic link");

        let publication =
            super::move_entry_transactional(&source, &destination_root, ConflictAction::Rename)
                .expect("cross-volume symbolic-link transaction should complete");

        assert_eq!(publication.disposition, crate::MoveDisposition::Moved);
        assert!(std::fs::symlink_metadata(&source).is_err());
        assert!(
            std::fs::symlink_metadata(&destination)
                .expect("published symbolic link")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_link(&destination).expect("published raw target"),
            raw_target
        );
        assert!(
            !destination.exists(),
            "the dangling link was not dereferenced"
        );

        std::fs::remove_file(&destination).expect("remove native test symbolic link");
        std::fs::remove_dir(&source_root).expect("remove native test source directory");
        std::fs::remove_dir(&destination_root).expect("remove native test destination directory");
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_directory_move_publishes_verified_tree() {
        struct Fixture {
            source_root: std::path::PathBuf,
            destination_root: std::path::PathBuf,
            recovery_root: std::path::PathBuf,
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.source_root);
                let _ = std::fs::remove_dir_all(&self.destination_root);
                let _ = std::fs::remove_dir_all(&self.recovery_root);
            }
        }

        let recovery_root = std::env::var_os("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT")
            .map(std::path::PathBuf::from)
            .expect("set OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT to an isolated empty fixture root");
        let destination_volume = std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT")
            .map(std::path::PathBuf::from)
            .expect("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT must name a second Windows volume");
        let mut transaction_id = [0_u8; 16];
        getrandom::fill(&mut transaction_id).expect("native directory test transaction id");
        let test_name = format!(
            "ottrin-native-cross-volume-directory-{}",
            super::encode_transaction_id(transaction_id)
        );
        let fixture = Fixture {
            source_root: std::env::temp_dir().join(&test_name),
            destination_root: destination_volume.join(&test_name),
            recovery_root,
        };
        let source = fixture.source_root.join("source-tree");
        let nested = source.join("nested");
        std::fs::create_dir_all(&nested).expect("create cross-volume source tree");
        std::fs::create_dir(&fixture.destination_root)
            .expect("create cross-volume destination root");
        let payload = b"cross-volume directory verified payload";
        std::fs::write(nested.join("payload.bin"), payload).expect("write nested payload");
        std::fs::create_dir(source.join("empty")).unwrap();
        std::fs::write(fixture.source_root.join("outside.txt"), b"outside sentinel").unwrap();
        std::os::windows::fs::symlink_file("missing-target", nested.join("dangling")).unwrap();
        std::os::windows::fs::symlink_file("../../outside.txt", nested.join("external")).unwrap();
        std::os::windows::fs::symlink_dir("..", nested.join("cycle")).unwrap();
        let original_links = ["dangling", "external", "cycle"].map(|name| {
            (
                name,
                super::read_windows_symbolic_link(&nested.join(name)).unwrap(),
            )
        });

        let publication = super::move_entry_transactional(
            &source,
            &fixture.destination_root,
            ConflictAction::Rename,
        )
        .expect("cross-volume directory transaction should complete");

        assert_eq!(publication.disposition, crate::MoveDisposition::Moved);
        assert_eq!(
            publication.published_path,
            Some(fixture.destination_root.join("source-tree"))
        );
        assert!(
            !source.exists(),
            "source must be removed only after publication"
        );
        assert_eq!(
            std::fs::read(
                fixture
                    .destination_root
                    .join("source-tree")
                    .join("nested")
                    .join("payload.bin")
            )
            .expect("read published nested payload"),
            payload
        );
        let published = fixture.destination_root.join("source-tree");
        assert!(published.join("empty").is_dir());
        for (name, target, directory) in [
            ("dangling", "missing-target", false),
            ("external", "../../outside.txt", false),
            ("cycle", "..", true),
        ] {
            let path = published.join("nested").join(name);
            assert!(
                std::fs::symlink_metadata(&path)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(
                std::fs::read_link(&path).unwrap(),
                std::path::Path::new(target)
            );
            let observed = super::read_windows_symbolic_link(&path).unwrap();
            let original = &original_links
                .iter()
                .find(|(original_name, _)| *original_name == name)
                .unwrap()
                .1;
            assert_eq!(
                &observed, original,
                "raw UTF-16 target and link kind must survive the move"
            );
            assert_eq!(observed.1, directory);
        }
        assert!(!published.join("nested/dangling").exists());
        assert!(!published.join("nested/external").exists());
        assert_eq!(
            std::fs::read(fixture.source_root.join("outside.txt")).unwrap(),
            b"outside sentinel"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_conflicts_preserve_published_and_displaced_entries() {
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for kind in ["file", "directory", "symlink"] {
            for conflict in [
                ConflictAction::Skip,
                ConflictAction::Rename,
                ConflictAction::Overwrite,
            ] {
                let source_fixture = crate::tests::TestDir::new("cross-conflict-source");
                let destination_fixture =
                    crate::tests::TestDir::new_at(&volume, "cross-conflict-destination");
                let source = source_fixture.path().join("entry");
                let target = destination_fixture.path().join("entry");
                let create = |path: &std::path::Path, contents: &str| match kind {
                    "directory" => {
                        std::fs::create_dir_all(path.join("nested")).unwrap();
                        std::fs::write(path.join("nested/payload"), contents).unwrap();
                    }
                    "symlink" => std::os::windows::fs::symlink_file(contents, path).unwrap(),
                    _ => std::fs::write(path, contents).unwrap(),
                };
                let read = |path: &std::path::Path| match kind {
                    "directory" => std::fs::read(path.join("nested/payload")).unwrap(),
                    "symlink" => std::fs::read_link(path)
                        .unwrap()
                        .to_string_lossy()
                        .as_bytes()
                        .to_vec(),
                    _ => std::fs::read(path).unwrap(),
                };
                create(&source, "source-data");
                create(&target, "displaced-data");
                let displaced_identity = move_recovery::stable_identity_of_entry(&target).unwrap();
                let result =
                    super::move_entry_transactional(&source, destination_fixture.path(), conflict)
                        .unwrap_or_else(|error| {
                            panic!("kind={kind}, conflict={conflict:?}: {error:?}")
                        });
                if conflict == ConflictAction::Skip {
                    assert_eq!(result.disposition, crate::MoveDisposition::Skipped);
                    assert_eq!(read(&source), b"source-data");
                    assert_eq!(read(&target), b"displaced-data");
                } else {
                    assert_eq!(result.disposition, crate::MoveDisposition::Moved);
                    assert!(std::fs::symlink_metadata(&source).is_err());
                    let published = result.published_path.unwrap();
                    assert_eq!(read(&published), b"source-data");
                    if conflict == ConflictAction::Rename {
                        assert_ne!(published, target);
                        assert_eq!(read(&target), b"displaced-data");
                    } else {
                        assert_eq!(published, target);
                        let retained = result
                            .retained_displaced_path
                            .expect("Overwrite must retain displaced entry");
                        assert_eq!(
                            move_recovery::stable_identity_of_entry(&retained).unwrap(),
                            displaced_identity
                        );
                        assert_eq!(read(&retained), b"displaced-data");
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_recovery_excludes_another_process_until_owner_death() {
        if let Some(ready) = std::env::var_os("OTTRIN_TEST_WINDOWS_RECOVERY_LOCK_READY") {
            let _held = super::acquire_windows_recovery_lock().unwrap();
            std::fs::write(ready, b"locked").unwrap();
            loop {
                std::thread::park();
            }
        }
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        let source_fixture = crate::tests::TestDir::new("recovery-owner-source");
        let destination_fixture =
            crate::tests::TestDir::new_at(&volume, "recovery-owner-destination");
        let source = source_fixture.path().join("entry");
        let target = destination_fixture.path().join("entry");
        std::fs::write(&source, b"source-data").unwrap();
        std::fs::write(&target, b"displaced-data").unwrap();
        let displaced_identity = move_recovery::stable_identity_of_entry(&target).unwrap();
        let mut permissions = std::fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&target, permissions).unwrap();
        assert!(
            super::move_entry_transactional(
                &source,
                destination_fixture.path(),
                ConflictAction::Overwrite
            )
            .is_err()
        );
        let transactions = super::discover_retained_windows_move_transactions().unwrap();
        assert_eq!(transactions.len(), 1);
        let id = transactions[0].transaction_id;
        assert!(
            std::process::Command::new("attrib.exe")
                .arg("-R")
                .arg(&target)
                .status()
                .unwrap()
                .success()
        );
        let root = super::windows_recovery_root().unwrap();
        let journal = super::windows_cross_volume_journal_path(&root, id);
        let before = std::fs::read(&journal).unwrap();
        let ready = source_fixture.path().join("lock-ready");
        struct Owner(std::process::Child);
        impl Drop for Owner {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut owner = Owner(std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "move_transaction::windows_source_tests::windows_native_cross_volume_recovery_excludes_another_process_until_owner_death", "--nocapture"])
            .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT", &root)
            .env("OTTRIN_TEST_WINDOWS_RECOVERY_LOCK_READY", &ready)
            .spawn().unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ready.exists() {
            assert!(
                owner.0.try_wait().unwrap().is_none(),
                "lock owner exited before readiness"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "lock owner did not become ready"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(std::fs::read(&ready).unwrap(), b"locked");
        assert!(matches!(
            super::recover_retained_windows_move_transaction(id),
            Err(super::PlatformError::ClaimedRecoveryPending { .. })
        ));
        assert!(matches!(
            super::move_entry_transactional(
                &source,
                destination_fixture.path(),
                ConflictAction::Overwrite
            ),
            Err(super::PlatformError::ClaimedRecoveryPending { .. })
        ));
        assert_eq!(std::fs::read(&journal).unwrap(), before);
        assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
        assert_eq!(std::fs::read(&target).unwrap(), b"displaced-data");
        assert_eq!(
            move_recovery::stable_identity_of_entry(&target).unwrap(),
            displaced_identity
        );
        owner.0.kill().unwrap();
        assert!(!owner.0.wait().unwrap().success());
        super::recover_retained_windows_move_transaction(id).expect("recovery after owner death");
        assert!(!source.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
        let record = super::load_windows_cross_volume_record(&journal).unwrap();
        let retained = record.retained_path().unwrap().unwrap();
        assert_eq!(std::fs::read(&retained).unwrap(), b"displaced-data");
        assert_eq!(
            move_recovery::stable_identity_of_entry(&retained).unwrap(),
            displaced_identity
        );
        super::recover_retained_windows_move_transaction(id)
            .expect("completed recovery is idempotent");
        assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
        assert_eq!(std::fs::read(&retained).unwrap(), b"displaced-data");
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_symbolic_link_streams_are_preserved_by_refusal() {
        use std::io::{Read, Write};
        use std::os::windows::fs::{OpenOptionsExt, symlink_dir, symlink_file};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };

        if let Some(source) = std::env::var_os("OTTRIN_TEST_LINK_STREAM_SOURCE") {
            let destination = std::env::var_os("OTTRIN_TEST_LINK_STREAM_DESTINATION").unwrap();
            super::WINDOWS_MOVE_HOOK.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(|boundary, _| {
                    if boundary == super::WindowsMoveBoundary::BeforeSourceCleanup {
                        std::process::exit(87);
                    }
                }));
            });
            super::move_entry_transactional(
                std::path::Path::new(&source),
                std::path::Path::new(&destination),
                ConflictAction::Rename,
            )
            .unwrap();
            panic!("child must stop after publication");
        }

        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for (is_directory, after_publication) in [false, true]
            .into_iter()
            .flat_map(|directory| [false, true].map(|after| (directory, after)))
        {
            let source_fixture = crate::tests::TestDir::new("link-stream-source");
            let destination = crate::tests::TestDir::new_at(&volume, "link-stream-destination");
            let source = source_fixture.path().join("entry");
            let target = source_fixture.path().join("target");
            let raw_target = std::path::Path::new("target");
            if is_directory {
                std::fs::create_dir(&target).unwrap();
                symlink_dir(raw_target, &source).unwrap();
            } else {
                std::fs::write(&target, b"target payload").unwrap();
                symlink_file(raw_target, &source).unwrap();
            }
            let source_identity = move_recovery::stable_identity_of_entry(&source).unwrap();
            if after_publication {
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--ignored", "--exact", "move_transaction::windows_source_tests::windows_native_cross_volume_symbolic_link_streams_are_preserved_by_refusal", "--nocapture"])
                    .env("OTTRIN_TEST_LINK_STREAM_SOURCE", &source)
                    .env("OTTRIN_TEST_LINK_STREAM_DESTINATION", destination.path())
                    .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT", destination.path().join("move-state"))
                    .status().unwrap();
                assert_eq!(status.code(), Some(87));
            }
            let stream = source.with_file_name("entry:retained-metadata");
            let flags = FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS;
            let mut stream_handle = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .custom_flags(flags)
                .open(&stream)
                .unwrap();
            stream_handle.write_all(b"link-owned stream").unwrap();
            drop(stream_handle);
            let link_handle =
                move_recovery::open_reparse_safe_cross_volume_source(&source).unwrap();
            assert!(
                super::windows_file_streams(&link_handle)
                    .unwrap()
                    .contains(&std::ffi::OsString::from(":retained-metadata:$DATA"))
            );
            let target_handle =
                move_recovery::open_reparse_safe_cross_volume_source(&target).unwrap();
            assert!(windows_stream_set_is_copy_safe(
                super::windows_file_streams(&target_handle).unwrap(),
                is_directory
            ));
            drop((link_handle, target_handle));

            let result = super::move_entry_transactional(
                &source,
                destination.path(),
                ConflictAction::Rename,
            );
            if after_publication {
                assert!(
                    matches!(result, Err(PlatformError::CommittedCleanupPending { .. })),
                    "published Move must retain the source link with cleanup pending"
                );
            } else {
                assert!(
                    matches!(
                        result,
                        Err(PlatformError::UnsupportedSafeCrossDeviceMove(_))
                    ),
                    "Move must refuse the link before publication"
                );
            }
            assert_eq!(
                move_recovery::stable_identity_of_entry(&source).unwrap(),
                source_identity
            );
            assert_eq!(std::fs::read_link(&source).unwrap(), raw_target);
            let mut bytes = Vec::new();
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(flags)
                .open(&stream)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            assert_eq!(bytes, b"link-owned stream");
            if !is_directory {
                assert_eq!(std::fs::read(&target).unwrap(), b"target payload");
            }
            let published = destination.path().join("entry");
            if after_publication {
                assert_eq!(std::fs::read_link(&published).unwrap(), raw_target);
            } else {
                assert!(std::fs::symlink_metadata(&published).is_err());
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_extended_attributes_are_preserved_by_refusal() {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
        use windows_sys::Wdk::Storage::FileSystem::{
            FILE_FULL_EA_INFORMATION, NtQueryEaFile, NtSetEaFile,
        };
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
        use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

        if let Some(source) = std::env::var_os("OTTRIN_TEST_MOVE_EA_SOURCE") {
            let destination = std::env::var_os("OTTRIN_TEST_MOVE_EA_DESTINATION").unwrap();
            super::WINDOWS_MOVE_HOOK.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(|boundary, _| {
                    if boundary == super::WindowsMoveBoundary::BeforeSourceCleanup {
                        std::process::exit(87);
                    }
                }));
            });
            super::move_entry_transactional(
                std::path::Path::new(&source),
                std::path::Path::new(&destination),
                ConflictAction::Rename,
            )
            .unwrap();
            panic!("child must stop after publication");
        }

        fn read_ea(path: &std::path::Path) -> Vec<u8> {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(path)
                .unwrap();
            let mut bytes = [0_u64; 32];
            let mut io = IO_STATUS_BLOCK::default();
            assert!(
                unsafe {
                    NtQueryEaFile(
                        file.as_raw_handle(),
                        &mut io,
                        bytes.as_mut_ptr().cast(),
                        std::mem::size_of_val(&bytes) as u32,
                        false,
                        std::ptr::null(),
                        0,
                        std::ptr::null(),
                        true,
                    )
                } >= 0
            );
            assert!(io.Information <= std::mem::size_of_val(&bytes));
            unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<u8>(), io.Information) }
                .to_vec()
        }
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for (kind, after_publication) in ["file", "directory", "child"]
            .into_iter()
            .flat_map(|kind| [false, true].map(|after| (kind, after)))
        {
            let source_fixture = crate::tests::TestDir::new("move-ea-source");
            let destination = crate::tests::TestDir::new_at(&volume, "move-ea-destination");
            let source = source_fixture.path().join("entry");
            let payload = if kind == "file" {
                source.clone()
            } else {
                std::fs::create_dir(&source).unwrap();
                source.join("payload")
            };
            std::fs::write(&payload, b"keep payload and metadata").unwrap();
            if after_publication {
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--ignored", "--exact", "move_transaction::windows_source_tests::windows_native_cross_volume_extended_attributes_are_preserved_by_refusal", "--nocapture"])
                    .env("OTTRIN_TEST_MOVE_EA_SOURCE", &source)
                    .env("OTTRIN_TEST_MOVE_EA_DESTINATION", destination.path())
                    .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT", destination.path().join("move-state"))
                    .status().unwrap();
                assert_eq!(status.code(), Some(87));
            }
            let decorated = if kind == "directory" {
                &source
            } else {
                &payload
            };
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(decorated)
                .unwrap();
            let mut bytes = [0_u64; 3];
            let ea = bytes.as_mut_ptr().cast::<FILE_FULL_EA_INFORMATION>();
            unsafe {
                (*ea).EaNameLength = 8;
                (*ea).EaValueLength = 4;
                std::ptr::copy_nonoverlapping(
                    b"OTTRINQA\0keep".as_ptr(),
                    std::ptr::addr_of_mut!((*ea).EaName).cast::<u8>(),
                    13,
                );
            }
            let mut io = IO_STATUS_BLOCK::default();
            assert!(
                unsafe { NtSetEaFile(file.as_raw_handle(), &mut io, bytes.as_ptr().cast(), 21) }
                    >= 0
            );
            drop(file);
            let before = read_ea(decorated);
            assert_eq!(&before[8..], b"OTTRINQA\0keep");
            let result = super::move_entry_transactional(
                &source,
                destination.path(),
                ConflictAction::Rename,
            );
            assert!(
                result.is_err(),
                "Move must preserve unsupported {kind} metadata by refusing"
            );
            assert_eq!(
                std::fs::read(&payload).unwrap(),
                b"keep payload and metadata"
            );
            assert_eq!(read_ea(decorated), before);
            let published = destination.path().join("entry");
            if after_publication {
                let published_payload = if kind == "file" {
                    published
                } else {
                    published.join("payload")
                };
                assert_eq!(
                    std::fs::read(published_payload).unwrap(),
                    b"keep payload and metadata"
                );
            } else {
                assert!(!published.exists());
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_teardown_retries_after_central_journal_sharing_failure() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
        if let Some(source) = std::env::var_os("OTTRIN_TEST_TEARDOWN_SOURCE") {
            let destination = std::env::var_os("OTTRIN_TEST_TEARDOWN_DESTINATION").unwrap();
            super::move_entry_transactional(
                std::path::Path::new(&source),
                std::path::Path::new(&destination),
                ConflictAction::Rename,
            )
            .unwrap();
            panic!("child must exit before source cleanup");
        }
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for boundary in [
            "BeforeSourceCleanup",
            "TeardownPrepared",
            "LocalJournalRemoved",
            "TransactionDirectoryRemoved",
        ] {
            let source_fixture = crate::tests::TestDir::new("teardown-source");
            let destination = crate::tests::TestDir::new_at(&volume, "teardown-destination");
            let source = source_fixture.path().join("entry");
            let target = destination.path().join("entry");
            std::fs::write(&source, b"published data must survive").unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "move_transaction::windows_source_tests::windows_native_cross_volume_teardown_retries_after_central_journal_sharing_failure", "--nocapture"])
            .env("OTTRIN_TEST_TEARDOWN_SOURCE", &source)
            .env("OTTRIN_TEST_TEARDOWN_DESTINATION", destination.path())
            .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT", destination.path().join("move-state"))
            .env("OTTRIN_TEST_WINDOWS_MOVE_EXIT_BOUNDARY", boundary)
            .status().unwrap();
            assert_eq!(status.code(), Some(87));
            let transactions = super::discover_retained_windows_move_transactions().unwrap();
            assert_eq!(transactions.len(), 1);
            let id = transactions[0].transaction_id;
            let journal = super::windows_cross_volume_journal_path(
                &super::windows_recovery_root().unwrap(),
                id,
            );
            let record = super::load_windows_cross_volume_record(&journal).unwrap();
            let transaction = record.transaction_path().unwrap();
            let checked = std::rc::Rc::new(std::cell::Cell::new(0));
            let callback_checked = checked.clone();
            let reset = WindowsMoveHookReset;
            super::WINDOWS_MOVE_HOOK.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
                    if boundary == super::WindowsMoveBoundary::JournalCleanupValidated {
                        let replacement = path.with_extension("replacement");
                        std::fs::write(&replacement, b"unknown replacement").unwrap();
                        assert!(std::fs::rename(&replacement, path).is_err());
                        assert!(
                            std::fs::OpenOptions::new()
                                .write(true)
                                .truncate(true)
                                .open(path)
                                .is_err()
                        );
                        assert_eq!(std::fs::read(&replacement).unwrap(), b"unknown replacement");
                        std::fs::remove_file(replacement).unwrap();
                        callback_checked.set(callback_checked.get() + 1);
                    }
                }));
            });
            if boundary == "BeforeSourceCleanup" || boundary == "LocalJournalRemoved" {
                let guard = std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                    .open(&journal)
                    .unwrap();
                let refused = super::move_entry_transactional(
                    &source,
                    destination.path(),
                    ConflictAction::Rename,
                );
                assert!(
                    refused.is_err(),
                    "central journal sharing must prevent complete teardown"
                );
                assert!(!source.exists(), "exercise teardown after source removal");
                assert_eq!(
                    std::fs::read(&target).unwrap(),
                    b"published data must survive"
                );
                assert!(journal.exists(), "keep discoverable completion evidence");
                if boundary == "LocalJournalRemoved" {
                    assert!(
                        !transaction.exists(),
                        "central journal must be removed last"
                    );
                }
                drop(guard);
            } else if boundary == "TeardownPrepared" {
                let local = transaction.join(super::WINDOWS_LOCAL_JOURNAL_NAME);
                let saved = std::fs::read(&local).unwrap();
                std::fs::write(&local, b"unexpected journal content").unwrap();
                assert!(super::recover_retained_windows_move_transaction(id).is_err());
                assert_eq!(
                    std::fs::read(&local).unwrap(),
                    b"unexpected journal content"
                );
                assert!(journal.exists());
                let predecessor: super::WindowsCrossVolumeRecord =
                    serde_json::from_slice(&saved).unwrap();
                assert_eq!(predecessor.phase, super::WindowsCrossVolumePhase::Published);
                std::fs::write(&local, &saved).unwrap();
                let unknown = transaction.join("unknown");
                std::fs::write(&unknown, b"retain unknown contents").unwrap();
                assert!(super::recover_retained_windows_move_transaction(id).is_err());
                assert_eq!(std::fs::read(&unknown).unwrap(), b"retain unknown contents");
                assert!(journal.exists());
                std::fs::remove_file(unknown).unwrap();
            } else {
                std::fs::create_dir(&transaction).unwrap();
                let unknown = transaction.join("replacement");
                std::fs::write(&unknown, b"preserve replacement directory").unwrap();
                assert!(super::recover_retained_windows_move_transaction(id).is_err());
                assert_eq!(
                    std::fs::read(&unknown).unwrap(),
                    b"preserve replacement directory"
                );
                assert!(journal.exists());
                std::fs::remove_file(unknown).unwrap();
                std::fs::remove_dir(&transaction).unwrap();
            }
            super::recover_retained_windows_move_transaction(id)
                .expect("retry teardown after journal sharing is restored");
            assert_eq!(
                std::fs::read(&target).unwrap(),
                b"published data must survive"
            );
            assert!(!source.exists());
            assert!(!transaction.exists());
            assert!(!journal.exists());
            assert!(checked.get() > 0, "exercise pinned journal deletion");
            drop(reset);
            assert!(
                super::discover_retained_windows_move_transactions()
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_FAT32_ROOT on an owned native FAT32 volume"]
    fn windows_native_move_fat32_boundary_preserves_both_entries() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationByHandleW;

        let volume = std::path::PathBuf::from(
            std::env::var_os("OTTRIN_WINDOWS_FAT32_ROOT").expect("owned FAT32 fixture root"),
        );
        let fat = crate::tests::TestDir::new_at(&volume, "fat-boundary");
        // Keep recovery state on the ordinary NTFS fixture, independent of direction.
        let ntfs = crate::tests::TestDir::new("ntfs-boundary");
        for (root, expected) in [(fat.path(), "FAT32"), (ntfs.path(), "NTFS")] {
            let directory = move_recovery::open_reparse_safe_directory_for_rename(root).unwrap();
            let mut name = [0_u16; 32];
            assert_ne!(
                unsafe {
                    GetVolumeInformationByHandleW(
                        directory.as_raw_handle(),
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        name.as_mut_ptr(),
                        name.len() as u32,
                    )
                },
                0
            );
            let end = name.iter().position(|&unit| unit == 0).unwrap();
            assert_eq!(String::from_utf16(&name[..end]).unwrap(), expected);
        }
        for (source_root, destination_root) in
            [(ntfs.path(), fat.path()), (fat.path(), ntfs.path())]
        {
            for conflict in [ConflictAction::Rename, ConflictAction::Overwrite] {
                let name = format!("entry-{conflict:?}");
                let source = source_root.join(&name);
                let target = destination_root.join(&name);
                std::fs::write(&source, b"source must survive").unwrap();
                std::fs::write(&target, b"destination must survive").unwrap();
                let result = super::move_entry_transactional(&source, destination_root, conflict);
                assert!(result.is_err(), "unsupported filesystem Move must refuse");
                eprintln!("{source:?} {conflict:?}: {:?}", result.as_ref().err());
                assert_eq!(std::fs::read(&source).unwrap(), b"source must survive");
                assert_eq!(std::fs::read(&target).unwrap(), b"destination must survive");
                assert!(
                    !destination_root
                        .join(format!("entry-{conflict:?} (1)"))
                        .exists()
                );
                std::fs::remove_file(source).unwrap();
                std::fs::remove_file(target).unwrap();
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_explicit_recovery_retains_displaced_file() {
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        let source_fixture = crate::tests::TestDir::new("explicit-recovery-source");
        let destination_fixture =
            crate::tests::TestDir::new_at(&volume, "explicit-recovery-destination");
        let source = source_fixture.path().join("entry");
        let target = destination_fixture.path().join("entry");
        std::fs::write(&source, b"source-data").unwrap();
        std::fs::write(&target, b"protected-data").unwrap();
        let identity = move_recovery::stable_identity_of_entry(&target).unwrap();
        let mut permissions = std::fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&target, permissions).unwrap();
        assert!(
            super::move_entry_transactional(
                &source,
                destination_fixture.path(),
                ConflictAction::Overwrite
            )
            .is_err()
        );
        let transactions = super::discover_retained_windows_move_transactions().unwrap();
        assert_eq!(transactions.len(), 1);
        let id = transactions[0].transaction_id;
        assert!(super::recover_retained_windows_move_transaction(id).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
        assert_eq!(std::fs::read(&target).unwrap(), b"protected-data");
        assert!(
            std::process::Command::new("attrib.exe")
                .arg("-R")
                .arg(&target)
                .status()
                .unwrap()
                .success()
        );
        super::recover_retained_windows_move_transaction(id)
            .expect("explicit recovery after removing fixture protection");
        assert!(!source.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
        let record = super::load_windows_cross_volume_record(
            &super::windows_cross_volume_journal_path(&super::windows_recovery_root().unwrap(), id),
        )
        .unwrap();
        let retained = record.retained_path().unwrap().unwrap();
        assert_eq!(
            move_recovery::stable_identity_of_entry(&retained).unwrap(),
            identity
        );
        assert_eq!(std::fs::read(&retained).unwrap(), b"protected-data");
        for _ in 0..2 {
            super::recover_retained_windows_move_transaction(id)
                .expect("completed retained recovery is idempotent");
            assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
            assert_eq!(std::fs::read(&retained).unwrap(), b"protected-data");
            assert_eq!(
                super::discover_retained_windows_move_transactions()
                    .unwrap()
                    .len(),
                1
            );
        }
        std::fs::write(&target, b"later-user-edit").unwrap();
        // A completed Move has no source left to delete. Repeated recovery must
        // preserve subsequent edits rather than replay publication.
        super::recover_retained_windows_move_transaction(id)
            .expect("completed recovery preserves later user edits");
        assert_eq!(std::fs::read(&target).unwrap(), b"later-user-edit");
        assert_eq!(std::fs::read(&retained).unwrap(), b"protected-data");
        let root = super::windows_recovery_root().unwrap();
        let journal = super::windows_cross_volume_journal_path(&root, id);
        let mut mismatched_id = id;
        mismatched_id[0] ^= 1;
        let mismatched_journal = super::windows_cross_volume_journal_path(&root, mismatched_id);
        std::fs::copy(&journal, &mismatched_journal).unwrap();
        assert!(super::discover_retained_windows_move_transactions().is_err());
        assert!(super::recover_retained_windows_move_transaction(mismatched_id).is_err());
        std::fs::remove_file(mismatched_journal).unwrap();
        std::fs::write(&journal, b"malformed journal").unwrap();
        assert!(super::discover_retained_windows_move_transactions().is_err());
        assert!(super::recover_retained_windows_move_transaction(id).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"later-user-edit");
        assert_eq!(
            move_recovery::stable_identity_of_entry(&retained).unwrap(),
            identity
        );
        assert_eq!(std::fs::read(&retained).unwrap(), b"protected-data");
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_read_only_destination_preserves_entries_and_allows_retry() {
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for conflict in [
            ConflictAction::Skip,
            ConflictAction::Rename,
            ConflictAction::Overwrite,
        ] {
            let source_fixture = crate::tests::TestDir::new("readonly-target-source");
            let destination_fixture =
                crate::tests::TestDir::new_at(&volume, "readonly-target-destination");
            let source = source_fixture.path().join("entry");
            let target = destination_fixture.path().join("entry");
            std::fs::write(&source, b"source-data").unwrap();
            std::fs::write(&target, b"protected-data").unwrap();
            let identity = move_recovery::stable_identity_of_entry(&target).unwrap();
            let mut permissions = std::fs::metadata(&target).unwrap().permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(&target, permissions).unwrap();
            let result =
                super::move_entry_transactional(&source, destination_fixture.path(), conflict);
            if conflict == ConflictAction::Overwrite {
                assert!(
                    result.is_err(),
                    "Overwrite must respect the destination read-only attribute"
                );
                assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
            } else {
                let result = result.expect("Skip and Rename leave the protected target intact");
                if conflict == ConflictAction::Skip {
                    assert_eq!(result.disposition, crate::MoveDisposition::Skipped);
                    assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
                } else {
                    assert_eq!(result.disposition, crate::MoveDisposition::Moved);
                    let published = result.published_path.unwrap();
                    assert_ne!(published, target);
                    assert_eq!(std::fs::read(published).unwrap(), b"source-data");
                    assert!(!source.exists());
                }
            }
            assert_eq!(
                move_recovery::stable_identity_of_entry(&target).unwrap(),
                identity
            );
            assert_eq!(std::fs::read(&target).unwrap(), b"protected-data");
            assert!(std::fs::metadata(&target).unwrap().permissions().readonly());
            if conflict == ConflictAction::Overwrite {
                assert!(
                    std::process::Command::new("attrib.exe")
                        .arg("-R")
                        .arg(&target)
                        .status()
                        .unwrap()
                        .success()
                );
                let result =
                    super::move_entry_transactional(&source, destination_fixture.path(), conflict)
                        .expect("retry after the owned destination protection is removed");
                assert_eq!(result.disposition, crate::MoveDisposition::Moved);
                assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
                assert!(!source.exists());
                let retained = result.retained_displaced_path.unwrap();
                assert_eq!(
                    move_recovery::stable_identity_of_entry(&retained).unwrap(),
                    identity
                );
                assert_eq!(std::fs::read(retained).unwrap(), b"protected-data");
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_read_only_overwrite_publication_exit_recovers() {
        if std::env::var_os("OTTRIN_TEST_WINDOWS_OVERWRITE_PUBLICATION_EXIT_CHILD").is_some() {
            let source = std::path::PathBuf::from(
                std::env::var_os("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_SOURCE").unwrap(),
            );
            let destination = std::path::PathBuf::from(
                std::env::var_os("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_DESTINATION").unwrap(),
            );
            let result =
                super::move_entry_transactional(&source, &destination, ConflictAction::Overwrite);
            panic!(
                "publication child returned instead of exiting: {:?}",
                result.as_ref().err()
            );
        }
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for boundary in ["FileReplaced", "FilePublicationFlushed"] {
            let source_fixture = crate::tests::TestDir::new("readonly-publication-source");
            let destination_fixture =
                crate::tests::TestDir::new_at(&volume, "readonly-publication-destination");
            let source = source_fixture.path().join("entry");
            let target = destination_fixture.path().join("entry");
            std::fs::write(&source, b"source-data").unwrap();
            std::fs::write(&target, b"displaced-data").unwrap();
            let displaced_identity = move_recovery::stable_identity_of_entry(&target).unwrap();
            let mut permissions = std::fs::metadata(&source).unwrap().permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(&source, permissions).unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "move_transaction::windows_source_tests::windows_native_cross_volume_read_only_overwrite_publication_exit_recovers", "--nocapture"])
            .env("OTTRIN_TEST_WINDOWS_OVERWRITE_PUBLICATION_EXIT_CHILD", "1")
            .env("OTTRIN_TEST_WINDOWS_MOVE_EXIT_BOUNDARY", boundary)
            .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_SOURCE", &source)
            .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_DESTINATION", destination_fixture.path())
            .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT", destination_fixture.path().join("move-state"))
            .status().unwrap();
            assert_eq!(status.code(), Some(87));
            assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
            assert!(std::fs::metadata(&source).unwrap().permissions().readonly());
            assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
            let transaction = std::fs::read_dir(destination_fixture.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".ottrin-move-")
                })
                .unwrap();
            let retained = transaction.join(super::WINDOWS_RETAINED_NAME);
            assert_eq!(
                move_recovery::stable_identity_of_entry(&retained).unwrap(),
                displaced_identity
            );
            assert_eq!(std::fs::read(&retained).unwrap(), b"displaced-data");
            assert_eq!(
                std::fs::metadata(&target).unwrap().permissions().readonly(),
                boundary == "FilePublicationFlushed"
            );
            if boundary == "FileReplaced" {
                assert!(
                    std::process::Command::new("attrib.exe")
                        .arg("-R")
                        .arg(&source)
                        .status()
                        .unwrap()
                        .success()
                );
                let stream = std::path::PathBuf::from(format!("{}:late-data", source.display()));
                std::fs::write(&stream, b"added-after-interruption").unwrap();
                let mut permissions = std::fs::metadata(&source).unwrap().permissions();
                permissions.set_readonly(true);
                std::fs::set_permissions(&source, permissions).unwrap();
                let result = super::move_entry_transactional(
                    &source,
                    destination_fixture.path(),
                    ConflictAction::Overwrite,
                );
                assert!(
                    result.is_err(),
                    "source with a new named stream must survive"
                );
                assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
                assert_eq!(std::fs::read(&stream).unwrap(), b"added-after-interruption");
                assert!(std::fs::metadata(&source).unwrap().permissions().readonly());
                assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
                // Remove only the added stream from this owned fixture for retry.
                assert!(
                    std::process::Command::new("attrib.exe")
                        .arg("-R")
                        .arg(&source)
                        .status()
                        .unwrap()
                        .success()
                );
                std::fs::remove_file(&stream).unwrap();
                let mut permissions = std::fs::metadata(&source).unwrap().permissions();
                permissions.set_readonly(true);
                std::fs::set_permissions(&source, permissions).unwrap();
            }
            if boundary == "FilePublicationFlushed" {
                let published_modified = std::fs::metadata(&target).unwrap().modified().unwrap();
                assert!(
                    std::process::Command::new("attrib.exe")
                        .arg("-R")
                        .arg(&source)
                        .status()
                        .unwrap()
                        .success()
                );
                std::fs::write(&source, b"changed-source").unwrap();
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&source)
                    .unwrap()
                    .set_modified(
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000),
                    )
                    .unwrap();
                let mut permissions = std::fs::metadata(&source).unwrap().permissions();
                permissions.set_readonly(true);
                std::fs::set_permissions(&source, permissions).unwrap();
                assert!(
                    super::move_entry_transactional(
                        &source,
                        destination_fixture.path(),
                        ConflictAction::Overwrite
                    )
                    .is_err()
                );
                assert_eq!(std::fs::read(&source).unwrap(), b"changed-source");
                assert_eq!(
                    std::fs::metadata(&target).unwrap().modified().unwrap(),
                    published_modified
                );
                assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
                // Restore only this owned fixture before exercising restart.
                assert!(
                    std::process::Command::new("attrib.exe")
                        .arg("-R")
                        .arg(&source)
                        .status()
                        .unwrap()
                        .success()
                );
                std::fs::write(&source, b"source-data").unwrap();
                let mut permissions = std::fs::metadata(&source).unwrap().permissions();
                permissions.set_readonly(true);
                std::fs::set_permissions(&source, permissions).unwrap();
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--ignored", "--exact", "move_transaction::windows_source_tests::windows_native_cross_volume_read_only_overwrite_publication_exit_recovers", "--nocapture"])
                    .env("OTTRIN_TEST_WINDOWS_OVERWRITE_PUBLICATION_EXIT_CHILD", "1")
                    .env("OTTRIN_TEST_WINDOWS_MOVE_EXIT_BOUNDARY", "PublicationReclaimed")
                    .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_SOURCE", &source)
                    .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_DESTINATION", destination_fixture.path())
                    .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT", destination_fixture.path().join("move-state"))
                    .status().unwrap();
                assert_eq!(status.code(), Some(87));
                assert!(!target.exists());
                let candidate = transaction.join(super::WINDOWS_CROSS_VOLUME_CANDIDATE_NAME);
                assert_eq!(std::fs::read(&candidate).unwrap(), b"source-data");
                assert!(
                    std::fs::metadata(&candidate)
                        .unwrap()
                        .permissions()
                        .readonly()
                );
                assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
                assert!(std::fs::metadata(&source).unwrap().permissions().readonly());
            }
            let result = super::move_entry_transactional(
                &source,
                destination_fixture.path(),
                ConflictAction::Overwrite,
            );
            if result.is_err() {
                assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
                assert!(std::fs::metadata(&source).unwrap().permissions().readonly());
            }
            let moved = result.expect("resume atomic replacement with source metadata");
            assert_eq!(moved.disposition, crate::MoveDisposition::Moved);
            assert_eq!(moved.published_path.as_deref(), Some(target.as_path()));
            assert_eq!(
                moved.retained_displaced_path.as_deref(),
                Some(retained.as_path())
            );
            assert!(!source.exists());
            assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
            assert!(std::fs::metadata(&target).unwrap().permissions().readonly());
            assert_eq!(
                move_recovery::stable_identity_of_entry(&retained).unwrap(),
                displaced_identity
            );
            assert_eq!(std::fs::read(&retained).unwrap(), b"displaced-data");
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_read_only_overwrite_retains_displaced_file() {
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        let source_fixture = crate::tests::TestDir::new("readonly-overwrite-source");
        let destination_fixture =
            crate::tests::TestDir::new_at(&volume, "readonly-overwrite-destination");
        let source = source_fixture.path().join("entry");
        let target = destination_fixture.path().join("entry");
        std::fs::write(&source, b"source-data").unwrap();
        std::fs::write(&target, b"displaced-data").unwrap();
        let displaced_identity = move_recovery::stable_identity_of_entry(&target).unwrap();
        let mut permissions = std::fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&source, permissions).unwrap();
        let result = super::move_entry_transactional(
            &source,
            destination_fixture.path(),
            ConflictAction::Overwrite,
        );
        if result.is_err() {
            assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
            assert!(std::fs::metadata(&source).unwrap().permissions().readonly());
        }
        let moved = result.expect("Overwrite with a read-only source");
        assert_eq!(moved.disposition, crate::MoveDisposition::Moved);
        assert_eq!(moved.published_path.as_deref(), Some(target.as_path()));
        assert!(!source.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
        assert!(std::fs::metadata(&target).unwrap().permissions().readonly());
        let retained = moved
            .retained_displaced_path
            .expect("retain exact displaced file");
        assert_eq!(
            move_recovery::stable_identity_of_entry(&retained).unwrap(),
            displaced_identity
        );
        assert_eq!(std::fs::read(&retained).unwrap(), b"displaced-data");
        assert!(
            !std::fs::metadata(&retained)
                .unwrap()
                .permissions()
                .readonly()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_move_preserves_read_only_file() {
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        let source_fixture = crate::tests::TestDir::new("readonly-source");
        let destination_fixture = crate::tests::TestDir::new_at(&volume, "readonly-destination");
        let source = source_fixture.path().join("entry");
        let target = destination_fixture.path().join("entry");
        std::fs::write(&source, b"source-data").unwrap();
        let mut permissions = std::fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&source, permissions).unwrap();
        let result = super::move_entry_transactional(
            &source,
            destination_fixture.path(),
            ConflictAction::Rename,
        );
        if result.is_err() {
            assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
            assert!(std::fs::metadata(&source).unwrap().permissions().readonly());
        }
        let moved = result.expect("move a read-only file while preserving its attribute");
        assert_eq!(moved.disposition, crate::MoveDisposition::Moved);
        assert!(!source.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
        assert!(std::fs::metadata(&target).unwrap().permissions().readonly());
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_move_preserves_basic_metadata() {
        use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN,
            FILE_BASIC_INFO, FileBasicInfo, SetFileInformationByHandle,
        };
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for directory in [false, true] {
            let source_fixture = crate::tests::TestDir::new("metadata-source");
            let destination_fixture =
                crate::tests::TestDir::new_at(&volume, "metadata-destination");
            let source = source_fixture.path().join("entry");
            let target = destination_fixture.path().join("entry");
            if directory {
                std::fs::create_dir(&source).unwrap();
                std::fs::write(source.join("payload"), b"source-data").unwrap();
            } else {
                std::fs::write(&source, b"source-data").unwrap();
            }
            let attributes = FILE_ATTRIBUTE_HIDDEN
                | if directory {
                    FILE_ATTRIBUTE_DIRECTORY
                } else {
                    FILE_ATTRIBUTE_ARCHIVE
                };
            let expected = FILE_BASIC_INFO {
                CreationTime: 132_000_000_000_000_000,
                LastAccessTime: 132_000_000_000_000_000,
                LastWriteTime: 133_000_000_000_000_000,
                ChangeTime: 0,
                FileAttributes: attributes,
            };
            let handle = move_recovery::open_reparse_safe_cross_volume_candidate(&source).unwrap();
            assert_ne!(
                unsafe {
                    SetFileInformationByHandle(
                        handle.as_raw_handle(),
                        FileBasicInfo,
                        std::ptr::addr_of!(expected).cast(),
                        std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                    )
                },
                0,
                "set fixture metadata: {}",
                std::io::Error::last_os_error()
            );
            drop(handle);
            let before = std::fs::metadata(&source).unwrap();
            assert_eq!(before.creation_time(), expected.CreationTime as u64);
            assert_eq!(before.last_write_time(), expected.LastWriteTime as u64);
            assert_eq!(before.file_attributes(), attributes);
            let moved = super::move_entry_transactional(
                &source,
                destination_fixture.path(),
                ConflictAction::Rename,
            )
            .expect("move entry with basic metadata");
            assert_eq!(moved.disposition, crate::MoveDisposition::Moved);
            assert!(!source.exists());
            let after = std::fs::metadata(&target).unwrap();
            assert_eq!(after.creation_time(), before.creation_time());
            assert_eq!(after.last_write_time(), before.last_write_time());
            assert_eq!(after.file_attributes(), before.file_attributes());
            assert_eq!(
                std::fs::read(if directory {
                    target.join("payload")
                } else {
                    target
                })
                .unwrap(),
                b"source-data"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_destination_acl_denial_preserves_source_and_allows_retry() {
        fn acl(path: &std::path::Path, arguments: &[&str]) {
            let output = std::process::Command::new("icacls.exe")
                .arg(path)
                .args(arguments)
                .output()
                .unwrap();
            assert!(output.status.success(), "icacls failed: {output:?}");
        }
        struct RestoreAcl(std::path::PathBuf);
        impl Drop for RestoreAcl {
            fn drop(&mut self) {
                acl(&self.0, &["/remove:d", "*S-1-1-0"]);
            }
        }
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        let source_fixture = crate::tests::TestDir::new("acl-source");
        let destination_fixture = crate::tests::TestDir::new_at(&volume, "acl-destination");
        let source = source_fixture.path().join("entry");
        let target = destination_fixture.path().join("entry");
        std::fs::write(&source, b"source-data").unwrap();
        // Deny file/subdirectory creation only in this owned fixture. Keep
        // WRITE_DAC available so the ordinary test user can restore its ACL.
        acl(destination_fixture.path(), &["/deny", "*S-1-1-0:(WD,AD)"]);
        let restore = RestoreAcl(destination_fixture.path().to_owned());
        assert!(std::fs::write(destination_fixture.path().join("probe"), b"denied").is_err());
        let refused = super::move_entry_transactional(
            &source,
            destination_fixture.path(),
            ConflictAction::Rename,
        );
        assert!(
            refused.is_err(),
            "Move must refuse denied destination access"
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
        assert!(!target.exists());
        drop(restore);
        let retried = super::move_entry_transactional(
            &source,
            destination_fixture.path(),
            ConflictAction::Rename,
        )
        .expect("retry must proceed after destination access is restored");
        assert_eq!(retried.disposition, crate::MoveDisposition::Moved);
        assert_eq!(retried.published_path.as_deref(), Some(target.as_path()));
        assert!(!source.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_sharing_conflicts_preserve_source_and_allow_retry() {
        use std::{cell::RefCell, os::windows::fs::OpenOptionsExt, rc::Rc};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for lock_source in [true, false] {
            let source_fixture = crate::tests::TestDir::new("sharing-source");
            let destination_fixture = crate::tests::TestDir::new_at(&volume, "sharing-destination");
            let source = source_fixture.path().join("entry");
            let target = destination_fixture.path().join("entry");
            std::fs::write(&source, b"source-data").unwrap();
            let writer = Rc::new(RefCell::new(None));
            let _reset = WindowsMoveHookReset;
            if lock_source {
                *writer.borrow_mut() = Some(
                    std::fs::OpenOptions::new()
                        .write(true)
                        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                        .open(&source)
                        .unwrap(),
                );
            } else {
                let callback_writer = writer.clone();
                super::WINDOWS_MOVE_HOOK.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
                        if boundary == super::WindowsMoveBoundary::BeforeSourceCleanup {
                            *callback_writer.borrow_mut() = Some(
                                std::fs::OpenOptions::new()
                                    .write(true)
                                    .share_mode(
                                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                                    )
                                    .open(path)
                                    .unwrap(),
                            );
                        }
                    }));
                });
            }
            let result = super::move_entry_transactional(
                &source,
                destination_fixture.path(),
                ConflictAction::Rename,
            );
            assert!(
                writer.borrow().is_some(),
                "writer must overlap the tested boundary"
            );
            assert!(result.is_err(), "Move must refuse an incompatible writer");
            assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
            if lock_source {
                assert!(!target.exists());
            } else {
                assert!(matches!(
                    result,
                    Err(PlatformError::CommittedCleanupPending { .. })
                ));
                assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
            }
            drop(_reset);
            drop(writer.borrow_mut().take());
            let retried = super::move_entry_transactional(
                &source,
                destination_fixture.path(),
                ConflictAction::Rename,
            )
            .expect("retry must proceed after the external writer closes");
            assert_eq!(retried.disposition, crate::MoveDisposition::Moved);
            assert_eq!(retried.published_path.as_deref(), Some(target.as_path()));
            assert!(!source.exists());
            assert_eq!(std::fs::read(&target).unwrap(), b"source-data");
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_published_file_or_link_change_preserves_source() {
        use std::{cell::Cell, rc::Rc};
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for link in [false, true] {
            let source_fixture = crate::tests::TestDir::new("published-change-source");
            let destination_fixture =
                crate::tests::TestDir::new_at(&volume, "published-change-destination");
            let source = source_fixture.path().join("entry");
            let target = destination_fixture.path().join("entry");
            if link {
                std::os::windows::fs::symlink_file("source-data", &source).unwrap();
            } else {
                std::fs::write(&source, b"source-data").unwrap();
            }
            let reached = Rc::new(Cell::new(false));
            let callback_reached = reached.clone();
            let _reset = WindowsMoveHookReset;
            super::WINDOWS_MOVE_HOOK.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
                    if boundary == super::WindowsMoveBoundary::BeforeSourceCleanup {
                        if link {
                            std::fs::remove_file(path).unwrap();
                            std::os::windows::fs::symlink_file("external-change", path).unwrap();
                        } else {
                            std::fs::write(path, b"external-change").unwrap();
                        }
                        callback_reached.set(true);
                    }
                }));
            });
            let result = super::move_entry_transactional(
                &source,
                destination_fixture.path(),
                ConflictAction::Rename,
            );
            assert!(reached.get(), "publication handoff must be exercised");
            assert!(
                std::fs::symlink_metadata(&source).is_ok(),
                "source was lost after published entry changed, link={link}: {:?}",
                result.as_ref().err()
            );
            assert!(
                matches!(result, Err(PlatformError::CommittedCleanupPending { .. })),
                "must retain recovery conflict: {:?}",
                result.as_ref().err()
            );
            if link {
                assert_eq!(
                    std::fs::read_link(&source).unwrap(),
                    std::path::Path::new("source-data")
                );
                assert_eq!(
                    std::fs::read_link(&target).unwrap(),
                    std::path::Path::new("external-change")
                );
            } else {
                assert_eq!(std::fs::read(&source).unwrap(), b"source-data");
                assert_eq!(std::fs::read(&target).unwrap(), b"external-change");
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_overwrite_retention_process_exit_recovers() {
        if std::env::var_os("OTTRIN_TEST_WINDOWS_RETAINED_EXIT_CHILD").is_some() {
            let source = std::path::PathBuf::from(
                std::env::var_os("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_SOURCE").unwrap(),
            );
            let destination = std::path::PathBuf::from(
                std::env::var_os("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_DESTINATION").unwrap(),
            );
            let result =
                super::move_entry_transactional(&source, &destination, ConflictAction::Overwrite);
            panic!(
                "retention child returned instead of exiting: {:?}",
                result.as_ref().err()
            );
        }
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for directory in [true, false] {
            let source_fixture = crate::tests::TestDir::new("retention-exit-source");
            let destination_fixture =
                crate::tests::TestDir::new_at(&volume, "retention-exit-destination");
            let source = source_fixture.path().join("entry");
            let target = destination_fixture.path().join("entry");
            let create = |path: &std::path::Path, contents: &str| {
                if directory {
                    std::fs::create_dir(path).unwrap();
                    std::fs::write(path.join("payload"), contents).unwrap();
                } else {
                    std::os::windows::fs::symlink_file(contents, path).unwrap();
                }
            };
            let read = |path: &std::path::Path| {
                if directory {
                    std::fs::read(path.join("payload")).unwrap()
                } else {
                    std::fs::read_link(path)
                        .unwrap()
                        .to_string_lossy()
                        .as_bytes()
                        .to_vec()
                }
            };
            create(&source, "source-data");
            create(&target, "displaced-data");
            let displaced_identity = move_recovery::stable_identity_of_entry(&target).unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "move_transaction::windows_source_tests::windows_native_cross_volume_overwrite_retention_process_exit_recovers", "--nocapture"])
                .env("OTTRIN_TEST_WINDOWS_RETAINED_EXIT_CHILD", "1")
                .env("OTTRIN_TEST_WINDOWS_MOVE_EXIT_BOUNDARY", "DisplacedRetained")
                .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_SOURCE", &source)
                .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_DESTINATION", destination_fixture.path())
                .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT", destination_fixture.path().join("move-state"))
                .status().unwrap();
            assert_eq!(
                status.code(),
                Some(87),
                "must exit after displaced retention, directory={directory}"
            );
            assert!(std::fs::symlink_metadata(&target).is_err());
            assert_eq!(read(&source), b"source-data");
            let transaction = std::fs::read_dir(destination_fixture.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".ottrin-move-")
                })
                .unwrap();
            let retained = transaction.join(super::WINDOWS_RETAINED_NAME);
            assert_eq!(
                move_recovery::stable_identity_of_entry(&retained).unwrap(),
                displaced_identity
            );
            assert_eq!(read(&retained), b"displaced-data");
            let publication = super::move_entry_transactional(
                &source,
                destination_fixture.path(),
                ConflictAction::Overwrite,
            )
            .expect("ordinary Move must finish interrupted retained publication");
            assert_eq!(publication.disposition, crate::MoveDisposition::Moved);
            assert_eq!(
                publication.published_path.as_deref(),
                Some(target.as_path())
            );
            assert_eq!(
                publication.retained_displaced_path.as_deref(),
                Some(retained.as_path())
            );
            assert!(std::fs::symlink_metadata(&source).is_err());
            assert_eq!(read(&target), b"source-data");
            assert_eq!(
                move_recovery::stable_identity_of_entry(&retained).unwrap(),
                displaced_identity
            );
            assert_eq!(read(&retained), b"displaced-data");
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_directory_handoff_mutations_preserve_source() {
        use super::WindowsMoveBoundary;
        use std::{
            cell::{Cell, RefCell},
            rc::Rc,
        };
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for fault in [
            WindowsMoveBoundary::CandidateUnlocked,
            WindowsMoveBoundary::SourceUnlocked,
            WindowsMoveBoundary::SourceClaimed,
        ] {
            let source_fixture = crate::tests::TestDir::new("directory-handoff-source");
            let destination_fixture =
                crate::tests::TestDir::new_at(&volume, "directory-handoff-destination");
            let source = source_fixture.path().join("source-tree");
            let target = destination_fixture.path().join("source-tree");
            std::fs::create_dir_all(source.join("nested")).unwrap();
            std::fs::write(source.join("nested/payload"), b"original data").unwrap();
            let reached = Rc::new(Cell::new(false));
            let claimed = Rc::new(RefCell::new(None));
            let callback_reached = reached.clone();
            let callback_claimed = claimed.clone();
            let callback_target = target.clone();
            let _reset = WindowsMoveHookReset;
            super::WINDOWS_MOVE_HOOK.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
                    if boundary == WindowsMoveBoundary::SourceClaimed {
                        *callback_claimed.borrow_mut() = Some(path.to_path_buf());
                    }
                    if boundary == fault {
                        if boundary != WindowsMoveBoundary::CandidateUnlocked {
                            assert!(
                                std::fs::write(
                                    callback_target.join("nested/payload"),
                                    b"destination attack"
                                )
                                .is_err(),
                                "verified destination must stay locked through source mutation"
                            );
                        }
                        std::fs::write(path.join("nested/payload"), b"external change").unwrap();
                        callback_reached.set(true);
                    }
                }));
            });
            let result = super::move_entry_transactional(
                &source,
                destination_fixture.path(),
                ConflictAction::Rename,
            );
            assert!(
                reached.get(),
                "fault boundary {fault:?} was not reached: {:?}",
                result.as_ref().err()
            );
            assert!(
                matches!(result, Err(PlatformError::CommittedCleanupPending { .. })),
                "changed content must prevent source cleanup: {:?}",
                result.as_ref().err()
            );
            let retained_source = claimed.borrow().clone().unwrap_or_else(|| source.clone());
            let expected_source: &[u8] = if fault == WindowsMoveBoundary::CandidateUnlocked {
                b"original data"
            } else {
                b"external change"
            };
            assert_eq!(
                std::fs::read(retained_source.join("nested/payload")).unwrap(),
                expected_source
            );
            assert_eq!(
                std::fs::read(target.join("nested/payload")).unwrap(),
                if fault == WindowsMoveBoundary::CandidateUnlocked {
                    b"external change" as &[u8]
                } else {
                    b"original data" as &[u8]
                }
            );
            drop(_reset);
            assert!(
                super::move_entry_transactional(
                    &source,
                    destination_fixture.path(),
                    ConflictAction::Rename
                )
                .is_err(),
                "ordinary recovery must retain the conflict"
            );
            assert_eq!(
                std::fs::read(retained_source.join("nested/payload")).unwrap(),
                expected_source
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_directory_process_exit_recovers_without_data_loss() {
        if std::env::var_os("OTTRIN_TEST_WINDOWS_DIRECTORY_EXIT_CHILD").is_some() {
            let source = std::path::PathBuf::from(
                std::env::var_os("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_SOURCE").unwrap(),
            );
            let destination = std::path::PathBuf::from(
                std::env::var_os("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_DESTINATION").unwrap(),
            );
            let result =
                super::move_entry_transactional(&source, &destination, ConflictAction::Rename);
            panic!(
                "directory child returned instead of exiting: {:?}",
                result.as_ref().err()
            );
        }
        let volume =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT").unwrap());
        for boundary in [
            "JournalPublished",
            "CandidateUnlocked",
            "CandidatePublished",
            "BeforeSourceCleanup",
            "SourceUnlocked",
            "SourceClaimed",
        ] {
            let source_fixture = crate::tests::TestDir::new("directory-exit-source");
            let destination_fixture =
                crate::tests::TestDir::new_at(&volume, "directory-exit-destination");
            let source = source_fixture.path().join("source-tree");
            let target = destination_fixture.path().join("source-tree");
            std::fs::create_dir_all(source.join("nested")).unwrap();
            std::fs::write(source.join("nested/payload"), b"interrupted directory data").unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "move_transaction::windows_source_tests::windows_native_cross_volume_directory_process_exit_recovers_without_data_loss", "--nocapture"])
                .env("OTTRIN_TEST_WINDOWS_DIRECTORY_EXIT_CHILD", "1")
                .env("OTTRIN_TEST_WINDOWS_MOVE_EXIT_BOUNDARY", boundary)
                .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_SOURCE", &source)
                .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_DESTINATION", destination_fixture.path())
                .env("OTTRIN_TEST_WINDOWS_MOVE_RECOVERY_ROOT", destination_fixture.path().join("move-state"))
                .status().unwrap();
            assert_eq!(status.code(), Some(87), "child must exit at {boundary}");
            if matches!(boundary, "JournalPublished" | "CandidateUnlocked") {
                assert!(!target.exists(), "{boundary} must precede publication");
            } else {
                assert_eq!(
                    std::fs::read(target.join("nested/payload")).unwrap(),
                    b"interrupted directory data"
                );
            }
            let source_copies = std::fs::read_dir(source_fixture.path())
                .unwrap()
                .map(|entry| entry.unwrap().path().join("nested/payload"))
                .filter(|path| path.exists())
                .collect::<Vec<_>>();
            assert_eq!(
                source_copies.len(),
                1,
                "interruption must preserve the original or claimed source"
            );
            assert_eq!(
                std::fs::read(&source_copies[0]).unwrap(),
                b"interrupted directory data"
            );
            if matches!(boundary, "JournalPublished" | "CandidatePublished") {
                let transaction = std::fs::read_dir(destination_fixture.path())
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(".ottrin-move-")
                    })
                    .unwrap();
                let local = transaction.join(super::WINDOWS_LOCAL_JOURNAL_NAME);
                if boundary == "JournalPublished" {
                    assert!(!local.exists());
                    let unexpected = transaction.join("unexpected");
                    std::fs::write(&unexpected, b"keep unknown data").unwrap();
                    let refused = super::move_entry_transactional(
                        &source,
                        destination_fixture.path(),
                        ConflictAction::Rename,
                    );
                    assert!(
                        matches!(
                            &refused,
                            Err(super::PlatformError::ClaimedRecoveryPending { .. })
                        ),
                        "unexpected refusal at {boundary}: {:?}",
                        refused.as_ref().err()
                    );
                    assert!(
                        !local.exists(),
                        "occupied initial transaction must not be repaired"
                    );
                    assert_eq!(std::fs::read(&unexpected).unwrap(), b"keep unknown data");
                    std::fs::remove_file(unexpected).unwrap();
                } else {
                    let local_bytes = std::fs::read(&local).unwrap();
                    std::fs::remove_file(&local).unwrap();
                    let refused = super::move_entry_transactional(
                        &source,
                        destination_fixture.path(),
                        ConflictAction::Rename,
                    );
                    assert!(
                        matches!(
                            &refused,
                            Err(super::PlatformError::ClaimedRecoveryPending { .. })
                        ),
                        "unexpected refusal at {boundary}: {:?}",
                        refused.as_ref().err()
                    );
                    assert!(
                        !local.exists(),
                        "a later missing journal must not be recreated"
                    );
                    assert_eq!(
                        std::fs::read(target.join("nested/payload")).unwrap(),
                        b"interrupted directory data"
                    );
                    std::fs::write(local, local_bytes).unwrap();
                }
                assert_eq!(
                    std::fs::read(&source_copies[0]).unwrap(),
                    b"interrupted directory data"
                );
            }
            let publication = super::move_entry_transactional(
                &source,
                destination_fixture.path(),
                ConflictAction::Rename,
            )
            .expect("ordinary Move entry must recover the interrupted directory");
            assert_eq!(publication.disposition, crate::MoveDisposition::Moved);
            assert_eq!(
                publication.published_path.as_deref(),
                Some(target.as_path())
            );
            assert!(!source_copies[0].exists());
            assert_eq!(
                std::fs::read(target.join("nested/payload")).unwrap(),
                b"interrupted directory data"
            );
            assert!(
                !std::fs::read_dir(destination_fixture.path())
                    .unwrap()
                    .any(|entry| entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".ottrin-move-"))
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires OTTRIN_WINDOWS_CROSS_VOLUME_ROOT on a second native Windows volume"]
    fn windows_native_cross_volume_staged_process_exit_recovers_through_move_entry() {
        use std::process::Command;

        let destination_volume = std::env::var_os("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT")
            .map(std::path::PathBuf::from)
            .expect("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT must name a second Windows volume");
        if std::env::var_os("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_EXIT_CHILD").is_some() {
            let source = std::path::PathBuf::from(
                std::env::var_os("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_SOURCE")
                    .expect("child source path"),
            );
            let destination_root = std::path::PathBuf::from(
                std::env::var_os("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_DESTINATION")
                    .expect("child destination path"),
            );
            if std::env::var_os("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_RECOVER_CHILD").is_some() {
                super::move_entry_transactional(&source, &destination_root, ConflictAction::Rename)
                    .expect("child should exit while recovering private metadata");
                panic!("child returned without exercising metadata recovery exit");
            }
            let _recovery_lock = super::acquire_windows_recovery_lock()
                .expect("acquire child Windows Move recovery lock");
            let source_handle = move_recovery::open_reparse_safe_entry_for_rename(&source)
                .expect("open child source identity handle");
            let destination_handle =
                move_recovery::open_reparse_safe_directory_for_rename(&destination_root)
                    .expect("open child destination directory handle");
            super::move_windows_cross_volume_preflight(
                &source,
                &destination_root,
                source.file_name().expect("child source filename"),
                ConflictAction::Rename,
                source_handle,
                destination_handle,
            )
            .expect("child should exit after durable staged state");
            panic!("child returned without exercising the process-exit hook");
        }

        for read_only in [false, true] {
            let mut transaction_id = [0_u8; 16];
            getrandom::fill(&mut transaction_id).expect("native interruption test transaction id");
            let test_name = format!(
                "ottrin-native-cross-volume-interrupt-{}",
                super::encode_transaction_id(transaction_id)
            );
            let source_root = std::env::temp_dir().join(&test_name);
            let destination_root = destination_volume.join(&test_name);
            let state_root = source_root.join("state");
            std::fs::create_dir_all(&source_root).expect("create interruption source root");
            std::fs::create_dir_all(&destination_root)
                .expect("create interruption destination root");
            std::fs::create_dir_all(&state_root).expect("create interruption state root");
            let source = source_root.join("payload.bin");
            let destination = destination_root.join("payload.bin");
            let payload = b"Ottrin native process-exit recovery payload\0\xff";
            std::fs::write(&source, payload).expect("write interruption source payload");
            let mut permissions = std::fs::metadata(&source).unwrap().permissions();
            permissions.set_readonly(read_only);
            std::fs::set_permissions(&source, permissions).unwrap();

            let status = Command::new(std::env::current_exe().expect("native test executable"))
            .args([
                "--ignored",
                "--exact",
                "move_transaction::windows_source_tests::windows_native_cross_volume_staged_process_exit_recovers_through_move_entry",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("OTTRIN_WINDOWS_CROSS_VOLUME_ROOT", &destination_volume)
            .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_EXIT_CHILD", "1")
            .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_EXIT_AFTER_STAGED", "1")
            .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_SOURCE", &source)
            .env(
                "OTTRIN_TEST_WINDOWS_CROSS_VOLUME_DESTINATION",
                &destination_root,
            )
            .env("LOCALAPPDATA", &state_root)
            .status()
            .expect("run interruption child");
            assert_eq!(
                status.code(),
                Some(86),
                "child must exit at staged boundary"
            );
            assert!(
                source.exists(),
                "interruption before publication must preserve source"
            );
            assert!(
                !destination.exists(),
                "staged candidate must not yet be published"
            );
            let transaction = std::fs::read_dir(&destination_root)
                .expect("inspect interrupted destination root")
                .filter_map(Result::ok)
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".ottrin-move-")
                })
                .expect("durable transaction directory must remain")
                .path();
            assert_eq!(
                std::fs::read(transaction.join(super::WINDOWS_CROSS_VOLUME_CANDIDATE_NAME))
                    .expect("read interrupted staged candidate"),
                payload
            );

            if read_only {
                let status = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--ignored", "--exact",
                        "move_transaction::windows_source_tests::windows_native_cross_volume_staged_process_exit_recovers_through_move_entry",
                        "--test-threads=1", "--nocapture",
                    ])
                    .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_EXIT_CHILD", "1")
                    .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_RECOVER_CHILD", "1")
                    .env("OTTRIN_TEST_WINDOWS_MOVE_EXIT_BOUNDARY", "CandidateWritable")
                    .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_SOURCE", &source)
                    .env("OTTRIN_TEST_WINDOWS_CROSS_VOLUME_DESTINATION", &destination_root)
                    .env("LOCALAPPDATA", &state_root)
                    .status().expect("run private metadata interruption child");
                assert_eq!(status.code(), Some(87));
                assert_eq!(std::fs::read(&source).unwrap(), payload);
                assert!(std::fs::metadata(&source).unwrap().permissions().readonly());
                let candidate = transaction.join(super::WINDOWS_CROSS_VOLUME_CANDIDATE_NAME);
                assert_eq!(std::fs::read(&candidate).unwrap(), payload);
                assert!(
                    !std::fs::metadata(&candidate)
                        .unwrap()
                        .permissions()
                        .readonly()
                );
            }

            let previous_local_app_data = std::env::var_os("LOCALAPPDATA");
            unsafe {
                std::env::set_var("LOCALAPPDATA", &state_root);
            }
            let recovery =
                super::move_entry_transactional(&source, &destination_root, ConflictAction::Rename);
            unsafe {
                if let Some(previous) = previous_local_app_data {
                    std::env::set_var("LOCALAPPDATA", previous);
                } else {
                    std::env::remove_var("LOCALAPPDATA");
                }
            }
            if recovery.is_err() {
                assert_eq!(std::fs::read(&source).unwrap(), payload);
                assert_eq!(
                    std::fs::metadata(&source).unwrap().permissions().readonly(),
                    read_only
                );
            }
            let publication =
                recovery.expect("ordinary Move entry should resume staged transaction");
            assert_eq!(publication.disposition, crate::MoveDisposition::Moved);
            assert_eq!(
                publication.published_path.as_deref(),
                Some(destination.as_path())
            );
            assert!(
                !source.exists(),
                "recovery removes source only after publication"
            );
            assert_eq!(
                std::fs::read(&destination).expect("read recovered destination"),
                payload
            );
            assert!(
                !transaction.exists(),
                "successful recovery cleans transaction data"
            );

            assert_eq!(
                std::fs::metadata(&destination)
                    .unwrap()
                    .permissions()
                    .readonly(),
                read_only
            );
            std::fs::remove_dir_all(&source_root).expect("remove interruption source root");
            std::fs::remove_dir_all(&destination_root)
                .expect("remove interruption destination root");
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_native_move_journal_requires_namespace_flush() {
        use std::{cell::RefCell, os::windows::fs::OpenOptionsExt, rc::Rc};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
        };
        let mut missing_failures = Vec::new();
        for replace in [false, true] {
            let fixture = crate::tests::TestDir::new("journal-namespace");
            let journal = fixture.path().join("journal");
            if replace {
                super::write_windows_record_file(&journal, &1_u32, false).unwrap();
            }
            let guard = Rc::new(RefCell::new(None));
            let callback_guard = guard.clone();
            let expected_journal = journal.clone();
            let reset = WindowsMoveHookReset;
            super::WINDOWS_MOVE_HOOK.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move |boundary, path| {
                    if boundary == super::WindowsMoveBoundary::JournalPublished
                        && path == expected_journal
                    {
                        let parent = path.parent().unwrap();
                        // Acquire after publication: an earlier lock can prevent
                        // ReplaceFileW itself, missing the durability boundary.
                        *callback_guard.borrow_mut() = Some(
                            std::fs::OpenOptions::new()
                                .read(true)
                                .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
                                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                                .open(parent)
                                .unwrap(),
                        );
                        assert!(
                            move_recovery::open_reparse_safe_directory_for_rename(parent).is_err(),
                            "fixture must deny the namespace flush handle"
                        );
                    }
                }));
            });
            let result = super::write_windows_record_file(&journal, &2_u32, false);
            assert!(
                guard.borrow().is_some(),
                "exercise journal publication boundary"
            );
            assert_eq!(std::fs::read(&journal).unwrap(), b"2");
            if result.is_ok() {
                missing_failures.push(replace);
            }
            drop(reset);
            drop(guard.borrow_mut().take());
            super::write_windows_record_file(&journal, &3_u32, false)
                .expect("retry after directory sharing is restored");
            assert_eq!(std::fs::read(&journal).unwrap(), b"3");
        }
        assert!(
            missing_failures.is_empty(),
            "journal persistence reported success without namespace durability: replace={missing_failures:?}"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_native_locked_directory_snapshot_preserves_exclusion() {
        let fixture = crate::tests::TestDir::new("windows-locked-directory-snapshot");
        let directory = fixture.path().join("candidate");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("payload"), b"verified content").unwrap();
        // More than one native enumeration buffer, including a counted UTF-16 name.
        for index in 0..600 {
            std::fs::write(
                directory.join(format!("child-{index:04}-acceptance.txt")),
                b"",
            )
            .unwrap();
        }
        std::fs::write(directory.join("ångström-文件.txt"), b"unicode").unwrap();
        let locked = move_recovery::open_reparse_safe_cross_volume_candidate(&directory).unwrap();
        assert!(std::fs::File::open(&directory).is_err());
        let snapshot = super::capture_windows_directory_snapshot(&directory, &locked)
            .expect("verify through the owned publication handle");
        assert_eq!(snapshot.entries.len(), 602);
        assert!(
            snapshot
                .entries
                .iter()
                .any(|entry| entry.relative_path == std::path::Path::new("ångström-文件.txt"))
        );
        assert!(std::fs::write(directory.join("payload"), b"changed").is_err());
        assert!(std::fs::remove_file(directory.join("payload")).is_err());
        assert!(std::fs::File::open(&directory).is_err());
        drop((snapshot, locked));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_native_directory_streams_are_empty_or_rejected() {
        let fixture = crate::tests::TestDir::new("windows-directory-streams");
        let directory = fixture.path().join("source-tree");
        std::fs::create_dir(&directory).unwrap();
        let handle = move_recovery::open_reparse_safe_entry_for_rename(&directory).unwrap();
        assert!(super::windows_file_streams(&handle).unwrap().is_empty());
        assert!(super::capture_windows_directory_snapshot(&directory, &handle).is_ok());

        let stream = directory.with_file_name("source-tree:retained-metadata");
        std::fs::write(&stream, b"named directory data").unwrap();
        let streams = super::windows_file_streams(&handle).unwrap();
        assert_eq!(
            streams,
            vec![std::ffi::OsString::from(":retained-metadata:$DATA")]
        );
        assert!(matches!(
            super::capture_windows_directory_snapshot(&directory, &handle),
            Err(PlatformError::UnsupportedSafeCrossDeviceMove(_))
        ));
        assert_eq!(std::fs::read(stream).unwrap(), b"named directory data");
    }

    #[test]
    fn windows_cross_volume_regular_file_rejects_named_data_streams() {
        assert!(windows_stream_set_is_copy_safe(["::$DATA"], false));
        assert!(!windows_stream_set_is_copy_safe(
            ["::$DATA", ":retained-metadata:$DATA",],
            false
        ));
        assert!(!windows_stream_set_is_copy_safe(
            std::iter::empty::<&str>(),
            false
        ));
    }

    #[test]
    fn windows_cross_volume_directories_require_no_data_streams() {
        assert!(windows_stream_set_is_copy_safe(
            std::iter::empty::<&str>(),
            true
        ));
        assert!(!windows_stream_set_is_copy_safe(["::$DATA"], true));
        assert!(!windows_stream_set_is_copy_safe([":metadata:$DATA"], true));
    }

    #[test]
    fn windows_cross_volume_publication_flushes_file_and_namespace_after_rename() {
        let events = std::cell::RefCell::new(Vec::new());
        publish_windows_cross_volume_candidate_with(
            || {
                events.borrow_mut().push("rename");
                Ok(())
            },
            || {
                events.borrow_mut().push("file");
                Ok(())
            },
            || {
                events.borrow_mut().push("namespace");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(events.into_inner(), vec!["rename", "file", "namespace"]);
    }

    #[test]
    fn windows_cross_volume_directory_overwrite_retains_before_publishing() {
        let events = std::cell::RefCell::new(Vec::new());
        publish_windows_cross_volume_directory_overwrite_with(
            || {
                events.borrow_mut().push("retain");
                Ok(())
            },
            || {
                events.borrow_mut().push("flush-retained-namespace");
                Ok(())
            },
            || {
                events.borrow_mut().push("publish");
                Ok(())
            },
            || {
                events.borrow_mut().push("flush-published");
                Ok(())
            },
            || {
                events.borrow_mut().push("flush-published-namespace");
                Ok(())
            },
        )
        .expect("directory publication");

        assert_eq!(
            events.into_inner(),
            [
                "retain",
                "flush-retained-namespace",
                "publish",
                "flush-published",
                "flush-published-namespace",
            ]
        );
    }

    #[test]
    fn windows_cross_volume_completion_flushes_source_namespace_before_cleanup() {
        let events = std::cell::RefCell::new(Vec::new());
        complete_windows_cross_volume_source_removal_with(
            || {
                events.borrow_mut().push("source-namespace");
                Ok(())
            },
            || {
                events.borrow_mut().push("cleanup");
                Ok(())
            },
        )
        .expect("source removal should become durable before cleanup");

        assert_eq!(events.into_inner(), vec!["source-namespace", "cleanup"]);
    }

    #[test]
    fn windows_cross_volume_recovery_flushes_retained_and_namespace_before_recording() {
        let events = std::cell::RefCell::new(Vec::new());
        flush_windows_cross_volume_recovered_publication_with(
            || {
                events.borrow_mut().push("published");
                Ok(())
            },
            || {
                events.borrow_mut().push("retained");
                Ok(())
            },
            || {
                events.borrow_mut().push("namespace");
                Ok(())
            },
            || {
                events.borrow_mut().push("recorded");
                Ok(())
            },
        )
        .expect("recovered publication should become durable");

        assert_eq!(
            events.into_inner(),
            vec!["published", "retained", "namespace", "recorded"]
        );
    }

    #[test]
    fn windows_cross_volume_publication_does_not_flush_namespace_after_file_flush_failure() {
        let events = std::cell::RefCell::new(Vec::new());
        let result = publish_windows_cross_volume_candidate_with(
            || {
                events.borrow_mut().push("rename");
                Ok(())
            },
            || {
                events.borrow_mut().push("file");
                Err(PlatformError::Io("injected file flush failure".into()))
            },
            || {
                events.borrow_mut().push("namespace");
                Ok(())
            },
        );
        assert!(matches!(result, Err(PlatformError::Io(_))));
        assert_eq!(events.into_inner(), vec!["rename", "file"]);
    }

    #[test]
    fn windows_cross_volume_restart_flushes_unrecorded_publication_before_source_removal() {
        assert_eq!(
            require_windows_publication_durability(
                WindowsCrossVolumeRestartAction::RemoveSource,
                false,
            ),
            WindowsCrossVolumeRestartAction::FlushPublished,
        );
        assert_eq!(
            require_windows_publication_durability(
                WindowsCrossVolumeRestartAction::RemoveSource,
                true,
            ),
            WindowsCrossVolumeRestartAction::RemoveSource,
        );
    }

    #[test]
    fn windows_cross_volume_skip_finishes_without_mutating_an_existing_destination() {
        assert_eq!(
            classify_windows_cross_volume_preflight(ConflictAction::Skip, true),
            WindowsCrossVolumePreflight::Skip,
        );
        for conflict in [
            ConflictAction::Skip,
            ConflictAction::Rename,
            ConflictAction::Overwrite,
        ] {
            assert_eq!(
                classify_windows_cross_volume_preflight(conflict, false),
                WindowsCrossVolumePreflight::TransactionRequired,
            );
        }
        for conflict in [ConflictAction::Rename, ConflictAction::Overwrite] {
            assert_eq!(
                classify_windows_cross_volume_preflight(conflict, true),
                WindowsCrossVolumePreflight::TransactionRequired,
            );
        }
    }

    #[test]
    fn windows_cross_volume_source_handle_denies_write_and_delete_sharing() {
        assert_eq!(
            move_recovery::WINDOWS_CROSS_VOLUME_SOURCE_SHARE_MODE,
            0x0000_0001
        );
    }

    #[test]
    fn windows_cross_volume_candidate_handle_denies_all_concurrent_mutation() {
        assert_eq!(move_recovery::WINDOWS_CROSS_VOLUME_CANDIDATE_SHARE_MODE, 0);
        assert_ne!(
            move_recovery::WINDOWS_CROSS_VOLUME_CANDIDATE_ACCESS_MODE & 0x4000_0000,
            0,
            "publication handle must be write-capable for FlushFileBuffers"
        );
    }

    #[test]
    fn windows_cross_volume_destination_parent_handle_can_flush_publication_namespace() {
        assert_ne!(
            move_recovery::WINDOWS_DESTINATION_PARENT_ACCESS_MODE & 0x4000_0000,
            0,
            "FlushFileBuffers requires a write-capable destination directory handle"
        );
    }

    #[test]
    fn windows_recovery_lock_denies_concurrent_recovery_owners() {
        assert_eq!(WINDOWS_RECOVERY_LOCK_SHARE_MODE, 0);
        assert_eq!(
            windows_recovery_lock_path(std::path::Path::new("Ottrin/move-recovery")),
            std::path::PathBuf::from("Ottrin/move-recovery.lock")
        );
    }

    #[test]
    fn windows_cross_volume_publication_requires_the_recorded_destination_parent() {
        assert!(windows_destination_parent_is_pinned(41_u64, 41));
        assert!(!windows_destination_parent_is_pinned(42_u64, 41));
    }

    #[test]
    fn windows_source_removal_requires_identity_and_content_evidence() {
        assert_eq!(
            classify_windows_source_removal(true, true, true),
            WindowsSourceRemovalDecision::RemoveOpenedSource,
        );
        for evidence in [
            (false, true, true),
            (true, false, true),
            (true, true, false),
            (false, false, false),
        ] {
            assert_eq!(
                classify_windows_source_removal(evidence.0, evidence.1, evidence.2),
                WindowsSourceRemovalDecision::PreserveForRecovery,
            );
        }
    }

    #[test]
    fn windows_cross_volume_rename_advances_from_latest_durable_candidate() {
        let candidates = [
            std::path::PathBuf::from("report.txt"),
            std::path::PathBuf::from("report (1).txt"),
        ];
        assert_eq!(
            next_windows_rename_candidate(&candidates),
            Some(std::path::PathBuf::from("report (2).txt")),
        );
    }

    #[test]
    fn windows_cross_volume_rename_candidate_journals_reconcile_only_by_prefix() {
        assert_eq!(
            reconcile_windows_rename_candidates(
                ConflictAction::Rename,
                &["report.txt", "report (2).txt"],
                &["report.txt"],
            ),
            Some(vec!["report.txt", "report (2).txt"]),
        );
        assert_eq!(
            reconcile_windows_rename_candidates(
                ConflictAction::Rename,
                &["report.txt", "report (3).txt"],
                &["report.txt", "report (2).txt"],
            ),
            None,
        );
        assert_eq!(
            reconcile_windows_rename_candidates(
                ConflictAction::Skip,
                &["report.txt", "report (2).txt"],
                &["report.txt"],
            ),
            None,
        );
    }

    #[test]
    fn windows_cross_volume_link_overwrite_resumes_after_displaced_retention() {
        assert_eq!(
            classify_windows_cross_volume_overwrite_restart(
                true,
                Some(1),
                None,
                Some(2),
                Some(3),
                1,
                2,
                3,
            ),
            WindowsCrossVolumeRestartAction::PublishStaged,
        );
        assert_eq!(
            classify_windows_cross_volume_overwrite_restart(
                true,
                Some(1),
                None,
                Some(2),
                Some(4),
                1,
                2,
                3,
            ),
            WindowsCrossVolumeRestartAction::RetainAmbiguous,
        );
    }

    #[test]
    fn windows_cross_volume_overwrite_restart_requires_retained_displaced_identity() {
        let source = 11_u64;
        let staged = 22_u64;
        let displaced = 33_u64;
        assert_eq!(
            classify_windows_cross_volume_overwrite_restart(
                true,
                Some(source),
                Some(displaced),
                None,
                None,
                source,
                staged,
                displaced,
            ),
            WindowsCrossVolumeRestartAction::CopyAndVerify,
        );
        assert_eq!(
            classify_windows_cross_volume_overwrite_restart(
                true,
                Some(source),
                Some(displaced),
                Some(staged),
                None,
                source,
                staged,
                displaced,
            ),
            WindowsCrossVolumeRestartAction::PublishStaged,
        );
        assert_eq!(
            classify_windows_cross_volume_overwrite_restart(
                true,
                Some(source),
                Some(staged),
                None,
                Some(displaced),
                source,
                staged,
                displaced,
            ),
            WindowsCrossVolumeRestartAction::RemoveSource,
        );
        assert_eq!(
            classify_windows_cross_volume_overwrite_restart(
                true,
                Some(source),
                None,
                Some(staged),
                Some(displaced),
                source,
                staged,
                displaced,
            ),
            WindowsCrossVolumeRestartAction::PublishStaged,
            "a non-atomic directory replacement must resume after retaining the displaced target",
        );
        assert_eq!(
            classify_windows_cross_volume_overwrite_restart(
                false,
                Some(source),
                None,
                Some(staged),
                Some(displaced),
                source,
                staged,
                displaced,
            ),
            WindowsCrossVolumeRestartAction::RetainAmbiguous,
            "file replacement without a journaled reclaim must retain ambiguous gaps",
        );
        assert_eq!(
            classify_windows_cross_volume_overwrite_restart(
                true,
                None,
                Some(staged),
                None,
                Some(displaced),
                source,
                staged,
                displaced,
            ),
            WindowsCrossVolumeRestartAction::Complete,
        );
        for ambiguous in [
            classify_windows_cross_volume_overwrite_restart(
                true,
                Some(source),
                Some(staged),
                None,
                None,
                source,
                staged,
                displaced,
            ),
            classify_windows_cross_volume_overwrite_restart(
                true,
                Some(source),
                Some(displaced),
                Some(displaced),
                None,
                source,
                staged,
                displaced,
            ),
            classify_windows_cross_volume_overwrite_restart(
                true,
                Some(source),
                Some(staged),
                Some(staged),
                Some(displaced),
                source,
                staged,
                displaced,
            ),
        ] {
            assert_eq!(ambiguous, WindowsCrossVolumeRestartAction::RetainAmbiguous);
        }
    }

    #[test]
    fn windows_cross_volume_restart_uses_durable_identity_topology() {
        let source = 11_u64;
        let staged = 22_u64;
        assert_eq!(
            classify_windows_cross_volume_restart(Some(source), None, None, source, None),
            WindowsCrossVolumeRestartAction::CopyAndVerify,
        );
        assert_eq!(
            classify_windows_cross_volume_restart(
                Some(source),
                None,
                Some(staged),
                source,
                Some(staged),
            ),
            WindowsCrossVolumeRestartAction::PublishStaged,
        );
        assert_eq!(
            classify_windows_cross_volume_restart(
                Some(source),
                Some(staged),
                None,
                source,
                Some(staged),
            ),
            WindowsCrossVolumeRestartAction::RemoveSource,
        );
        assert_eq!(
            classify_windows_cross_volume_restart(None, Some(staged), None, source, Some(staged),),
            WindowsCrossVolumeRestartAction::Complete,
        );
        assert_eq!(
            classify_windows_cross_volume_restart(
                Some(99),
                Some(staged),
                None,
                source,
                Some(staged),
            ),
            WindowsCrossVolumeRestartAction::RetainAmbiguous,
        );
    }

    #[test]
    fn windows_overwrite_requires_published_source_and_retained_displaced_identity() {
        let source = 11;
        let displaced = 22;

        assert_eq!(
            classify_windows_replace_topology(
                Some(source),
                None,
                Some(displaced),
                source,
                displaced,
            ),
            WindowsReplaceTopology::PublishedAndRetained,
        );
        assert_eq!(
            classify_windows_replace_topology(Some(source), None, Some(99), source, displaced,),
            WindowsReplaceTopology::PublishedAmbiguous,
        );
        assert_eq!(
            classify_windows_replace_topology(
                Some(displaced),
                Some(source),
                None,
                source,
                displaced,
            ),
            WindowsReplaceTopology::NotPublished,
        );
        assert_eq!(
            classify_windows_replace_topology(None, None, None, source, displaced),
            WindowsReplaceTopology::Ambiguous,
        );
    }

    #[test]
    fn windows_overwrite_restart_uses_durable_identities_not_advisory_phase() {
        let source = 11_u64;
        let displaced = 22_u64;

        assert_eq!(
            classify_windows_overwrite_restart(
                None,
                Some(displaced),
                Some(source),
                None,
                source,
                displaced,
            ),
            WindowsOverwriteRestartAction::PublishClaimed,
        );
        assert_eq!(
            classify_windows_overwrite_restart(
                None,
                Some(source),
                None,
                Some(displaced),
                source,
                displaced,
            ),
            WindowsOverwriteRestartAction::FinishPublished,
        );
        assert_eq!(
            classify_windows_overwrite_restart(
                None,
                None,
                Some(source),
                Some(displaced),
                source,
                displaced,
            ),
            WindowsOverwriteRestartAction::PublishAfterRetain,
        );
        assert_eq!(
            classify_windows_overwrite_restart(
                Some(source),
                Some(displaced),
                None,
                None,
                source,
                displaced,
            ),
            WindowsOverwriteRestartAction::ClaimSource,
        );
        assert_eq!(
            classify_windows_overwrite_restart(
                None,
                Some(source),
                Some(displaced),
                None,
                source,
                displaced,
            ),
            WindowsOverwriteRestartAction::RetainAmbiguous,
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn with_move_env_serializer<T>(work: impl FnOnce() -> T) -> T {
        let _guard = TEST_MOVE_ENV_SERIALIZER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        work()
    }

    struct TestDir(std::path::PathBuf, Option<std::path::PathBuf>);
    impl TestDir {
        fn new(label: &str) -> Self {
            loop {
                let mut random = [0_u8; 8];
                getrandom::fill(&mut random).unwrap();
                let suffix = random
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                let path = std::env::temp_dir()
                    .join(format!("ottrin-move-crossfs-metadata-{label}-{suffix}"));
                match std::fs::create_dir(&path) {
                    Ok(()) => {
                        let previous_recovery_root = replace_test_recovery_root(Some(
                            path.join("state").join("ottrin").join("moves"),
                        ));
                        return Self(path, previous_recovery_root);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create test directory: {error}"),
                }
            }
        }
    }

    fn is_move_transaction_directory(name: &std::ffi::OsStr) -> bool {
        let name = name.to_string_lossy();
        const PREFIX: &str = ".ottrin-move-";
        name.len() == PREFIX.len() + 32
            && name.starts_with(PREFIX)
            && name[PREFIX.len()..]
                .chars()
                .all(|char| char.is_ascii_hexdigit())
    }

    fn has_local_transaction_dir(root: &Path) -> bool {
        std::fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.path().is_dir() && is_move_transaction_directory(&entry.file_name()))
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
            let _ = replace_test_recovery_root(self.1.take());
        }
    }

    #[test]
    fn test_dir_restores_outer_recovery_root_after_nested_scope() {
        let outer = TestDir::new("outer-recovery-root-isolation");
        let outer_root = outer.0.join("state").join("ottrin").join("moves");
        assert_eq!(default_recovery_root().unwrap(), outer_root);

        {
            let inner = TestDir::new("inner-recovery-root-isolation");
            assert_eq!(
                default_recovery_root().unwrap(),
                inner.0.join("state").join("ottrin").join("moves")
            );
        }

        assert_eq!(default_recovery_root().unwrap(), outer_root);
    }

    #[test]
    fn cross_filesystem_retry_through_public_api_recovers_claimed_source() {
        let destination_root = TestDir::new("public-retry");
        let source_root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-state")
            .join(format!(
                "ottrin-move-public-retry-{}",
                getrandom::u64().unwrap()
            ));
        std::fs::create_dir_all(&source_root).unwrap();
        let source = source_root.join("source");
        let destination = destination_root.0.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let source_device = std::fs::symlink_metadata(&source_root).unwrap().dev();
        let destination_device = std::fs::symlink_metadata(&destination).unwrap().dev();
        assert_ne!(
            source_device, destination_device,
            "test requires repository storage and the system temporary directory on distinct filesystems"
        );

        let interrupted = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| {
                if point == TestPoint::PendingDestination {
                    Err("simulate interruption after source claim")
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(
            interrupted,
            Err(PlatformError::ClaimedRecoveryPending { .. })
        ));
        assert!(!source.exists());

        let recovered = move_entry_transactional(&source, &destination, ConflictAction::Skip);

        let recovered = recovered.unwrap_or_else(|error| panic!("public retry failed: {error}"));
        assert_eq!(recovered.disposition, MoveDisposition::Moved);
        assert_eq!(recovered.published_path, Some(destination.join("source")));
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"payload"
        );
        assert!(!source.exists());
        assert!(!has_local_transaction_dir(&source_root));
        std::fs::remove_dir_all(source_root).unwrap();
    }

    #[test]
    fn cross_filesystem_move_publishes_destination_metadata_with_snapshot() {
        let dir = TestDir::new("metadata");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                TestPoint::Publication => Err("publication hook failure"),
                _ => Ok(()),
            },
        );
        assert!(matches!(
            result,
            Err(PlatformError::Io(message))
                if message == "move test hook failed: publication hook failure"
        ));

        let local_transaction = std::fs::read_dir(dir.0.clone())
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.path().is_dir()
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".ottrin-move-")
            })
            .expect("local transaction must exist");
        let local_transaction_name = local_transaction.file_name();
        let local_transaction_name = local_transaction_name.to_string_lossy();
        let transaction_id = local_transaction_name.trim_start_matches(".ottrin-move-");
        let local_path = local_transaction.path();

        let local_journal = serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(local_path.join("journal")).unwrap(),
        )
        .unwrap();
        let recovery_root = default_recovery_root().unwrap();
        let central_journal = serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(recovery_root.join(format!("{transaction_id}.journal"))).unwrap(),
        )
        .unwrap();

        assert_eq!(local_journal["phase"], "pending-destination");
        assert_eq!(central_journal["phase"], "pending-destination");
        assert!(!local_journal["destination_transaction_name"].is_null());
        assert!(!local_journal["source_tree_snapshot"].is_null());
        assert!(!local_journal["staged_destination_identity"].is_null());
        assert!(!central_journal["destination_transaction_name"].is_null());
        assert!(!central_journal["source_tree_snapshot"].is_null());
        assert!(!central_journal["staged_destination_identity"].is_null());
    }

    #[test]
    fn cross_filesystem_directory_move_preserves_nested_files_and_symlinks() {
        with_move_env_serializer(|| {
            use std::os::unix::fs::symlink;

            let dir = TestDir::new("directory-tree");
            let source = dir.0.join("source-tree");
            let nested = source.join("nested");
            let destination = dir.0.join("destination");
            std::fs::create_dir_all(&nested).unwrap();
            std::fs::create_dir(&destination).unwrap();
            std::fs::write(nested.join("payload.txt"), b"nested payload").unwrap();
            symlink("nested/payload.txt", source.join("payload-link")).unwrap();

            let publication = move_entry_transactional_with_test_hook(
                &source,
                &destination,
                ConflictAction::Skip,
                |point| {
                    if point == TestPoint::ClassifyCrossDevice {
                        Err("force cross-device move")
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap();

            let published = destination.join("source-tree");
            assert_eq!(publication, MoveDisposition::Moved);
            assert!(!source.exists());
            assert_eq!(
                std::fs::read(published.join("nested/payload.txt")).unwrap(),
                b"nested payload"
            );
            assert_eq!(
                std::fs::read_link(published.join("payload-link")).unwrap(),
                PathBuf::from("nested/payload.txt")
            );
        });
    }

    #[test]
    fn cross_filesystem_symlink_move_preserves_link_without_touching_target() {
        with_move_env_serializer(|| {
            use std::os::unix::fs::symlink;

            let dir = TestDir::new("symlink-entry");
            let target = dir.0.join("target.txt");
            let source = dir.0.join("source-link");
            let destination = dir.0.join("destination");
            std::fs::write(&target, b"target payload").unwrap();
            std::fs::create_dir(&destination).unwrap();
            symlink("target.txt", &source).unwrap();

            let publication = move_entry_transactional_with_test_hook(
                &source,
                &destination,
                ConflictAction::Skip,
                |point| {
                    if point == TestPoint::ClassifyCrossDevice {
                        Err("force cross-device move")
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap();

            let published = destination.join("source-link");
            assert_eq!(publication, MoveDisposition::Moved);
            assert!(std::fs::symlink_metadata(&source).is_err());
            assert_eq!(
                std::fs::read_link(&published).unwrap(),
                PathBuf::from("target.txt")
            );
            assert_eq!(std::fs::read(&target).unwrap(), b"target payload");
        });
    }

    #[test]
    fn cross_filesystem_source_mutation_after_publication_preserves_both_entries() {
        let dir = TestDir::new("source-mutation-after-publication");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "original payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| {
                if point == TestPoint::ClassifyCrossDevice {
                    Err("force cross-device move")
                } else if point == TestPoint::Publication {
                    let transaction = std::fs::read_dir(&dir.0)
                        .unwrap()
                        .filter_map(Result::ok)
                        .find(|entry| is_move_transaction_directory(&entry.file_name()))
                        .unwrap();
                    std::fs::write(transaction.path().join("claimed"), "mutated payload").unwrap();
                    Ok(())
                } else {
                    Ok(())
                }
            },
        );

        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { message, .. })
                if message == "cross-device source snapshot changed before cleanup"
        ));
        assert_eq!(
            std::fs::read_to_string(destination.join("source")).unwrap(),
            "original payload"
        );
        let transaction = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| is_move_transaction_directory(&entry.file_name()))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(transaction.path().join("claimed")).unwrap(),
            "mutated payload"
        );
    }

    #[test]
    fn cross_filesystem_staged_content_mutation_before_publication_preserves_source() {
        let dir = TestDir::new("staged-content-mutation-before-publication");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "original payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| {
                if point == TestPoint::ClassifyCrossDevice {
                    Err("force cross-device move")
                } else if point == TestPoint::PendingDestination {
                    let destination_transaction = std::fs::read_dir(&destination)
                        .unwrap()
                        .filter_map(Result::ok)
                        .find(|entry| is_move_transaction_directory(&entry.file_name()))
                        .expect("destination transaction exists after staging");
                    std::fs::write(
                        destination_transaction
                            .path()
                            .join(CROSS_DEVICE_STAGED_NAME),
                        "mutated staged payload",
                    )
                    .unwrap();
                    Ok(())
                } else {
                    Ok(())
                }
            },
        );

        assert!(matches!(
            result,
            Err(PlatformError::ClaimedRecoveryPending { message, .. })
                if message == "cross-device staged destination does not match source snapshot"
        ));
        assert!(
            !destination.join("source").exists(),
            "unverified staged content must not be published"
        );
        let source_transaction = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| is_move_transaction_directory(&entry.file_name()))
            .expect("claimed source remains recoverable");
        assert_eq!(
            std::fs::read(source_transaction.path().join("claimed")).unwrap(),
            b"original payload"
        );
    }

    #[test]
    fn cross_filesystem_source_mutation_at_cleanup_boundary_preserves_both_entries() {
        let dir = TestDir::new("source-mutation-at-cleanup-boundary");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "original payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| {
                if point == TestPoint::ClassifyCrossDevice {
                    Err("force cross-device move")
                } else if point == TestPoint::BeforeSourceCleanup {
                    let transaction = std::fs::read_dir(&dir.0)
                        .unwrap()
                        .filter_map(Result::ok)
                        .find(|entry| is_move_transaction_directory(&entry.file_name()))
                        .unwrap();
                    std::fs::write(transaction.path().join("claimed"), "mutated payload").unwrap();
                    Ok(())
                } else {
                    Ok(())
                }
            },
        );

        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { message, .. })
                if message == "cross-device source snapshot changed at cleanup boundary"
        ));
        assert_eq!(
            std::fs::read_to_string(destination.join("source")).unwrap(),
            "original payload"
        );
        let transaction = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| is_move_transaction_directory(&entry.file_name()))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(transaction.path().join("claimed")).unwrap(),
            "mutated payload"
        );
    }

    #[test]
    fn cross_filesystem_move_durably_records_publication_before_source_cleanup() {
        let dir = TestDir::new("published-before-cleanup");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                TestPoint::BeforeSourceCleanup => Err("stop before source cleanup"),
                _ => Ok(()),
            },
        );
        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { message, .. })
                if message == "move test hook failed: stop before source cleanup"
        ));

        let local_transaction = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.path().is_dir()
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".ottrin-move-")
            })
            .expect("local transaction must remain for recovery");
        let local_path = local_transaction.path();
        let transaction_id = local_transaction
            .file_name()
            .to_string_lossy()
            .trim_start_matches(".ottrin-move-")
            .to_owned();
        let local_journal: serde_json::Value =
            serde_json::from_slice(&std::fs::read(local_path.join("journal")).unwrap()).unwrap();
        let central_journal: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                default_recovery_root()
                    .unwrap()
                    .join(format!("{transaction_id}.journal")),
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(local_journal["phase"], "published");
        assert_eq!(central_journal["phase"], "published");
        assert_eq!(
            std::fs::read(local_path.join("claimed")).unwrap(),
            b"payload"
        );
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"payload"
        );
    }

    #[test]
    fn cross_filesystem_recovery_finishes_published_move_source_cleanup() {
        let dir = TestDir::new("recover-published-before-cleanup");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                TestPoint::BeforeSourceCleanup => Err("stop before source cleanup"),
                _ => Ok(()),
            },
        );
        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { message, .. })
                if message == "move test hook failed: stop before source cleanup"
        ));

        let recovery_root = default_recovery_root().unwrap();
        let retained = discover_retained_move_transactions_at(&recovery_root).unwrap();
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].phase, move_recovery::JournalPhase::Published);

        recover_retained_move_pending_destination_at(&recovery_root, retained[0].transaction_id)
            .unwrap();

        assert!(!source.exists());
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"payload"
        );
        assert!(
            discover_retained_move_transactions_at(&recovery_root)
                .unwrap()
                .is_empty()
        );
        assert!(!has_local_transaction_dir(&dir.0));
        assert!(
            !std::fs::read_dir(&destination)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| is_move_transaction_directory(&entry.file_name()))
        );
    }

    #[test]
    fn cross_filesystem_recovery_finishes_cleanup_after_source_was_unlinked() {
        let dir = TestDir::new("recover-published-after-unlink");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                TestPoint::BeforeSourceCleanup => Err("stop before source cleanup"),
                _ => Ok(()),
            },
        );
        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { .. })
        ));

        let recovery_root = default_recovery_root().unwrap();
        let retained = discover_retained_move_transactions_at(&recovery_root).unwrap();
        let local = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| is_move_transaction_directory(&entry.file_name()))
            .unwrap();
        std::fs::remove_file(local.path().join("claimed")).unwrap();

        recover_retained_move_pending_destination_at(&recovery_root, retained[0].transaction_id)
            .unwrap();

        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"payload"
        );
        assert!(
            discover_retained_move_transactions_at(&recovery_root)
                .unwrap()
                .is_empty()
        );
        assert!(!has_local_transaction_dir(&dir.0));
    }

    #[test]
    fn same_filesystem_overwrite_publication_interruption_is_committed_and_retained() {
        let dir = TestDir::new("overwrite-publication-interruption");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "moved payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("source"), "displaced payload").unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Overwrite,
            |point| {
                if point == TestPoint::Publication {
                    Err("simulate termination after publication")
                } else {
                    Ok(())
                }
            },
        );

        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { message, .. })
                if message == "move test hook failed after publication: simulate termination after publication"
        ));
        assert!(!source.exists());
        assert_eq!(
            std::fs::read_to_string(destination.join("source")).unwrap(),
            "moved payload"
        );
        let local = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| is_move_transaction_directory(&entry.file_name()))
            .expect("durable local transaction");
        assert_eq!(
            std::fs::read_to_string(local.path().join("claimed")).unwrap(),
            "displaced payload"
        );
        let recovery_root = default_recovery_root().unwrap();
        let retained = discover_retained_move_transactions_at(&recovery_root).unwrap();
        assert_eq!(retained.len(), 1);

        recover_retained_move_pending_destination_at(&recovery_root, retained[0].transaction_id)
            .unwrap();

        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved payload");
        assert_eq!(
            std::fs::read_to_string(destination.join("source")).unwrap(),
            "displaced payload"
        );
        assert!(
            discover_retained_move_transactions_at(&recovery_root)
                .unwrap()
                .is_empty()
        );
        assert!(!has_local_transaction_dir(&dir.0));
    }

    #[test]
    fn cross_filesystem_overwrite_interruption_after_exchange_preserves_all_three_entries() {
        let dir = TestDir::new("cross-overwrite-post-exchange");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "source payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("source"), "displaced destination payload").unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Overwrite,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                TestPoint::Publication => Err("simulate interruption after exchange"),
                _ => Ok(()),
            },
        );

        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { message, .. })
                if message == "move test hook failed after cross-device overwrite publication: simulate interruption after exchange"
        ));
        assert!(!source.exists());
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"source payload"
        );

        let source_transaction = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.path().is_dir() && is_move_transaction_directory(&entry.file_name())
            })
            .expect("source transaction remains for recovery");
        assert_eq!(
            std::fs::read(source_transaction.path().join("claimed")).unwrap(),
            b"source payload"
        );

        let destination_transaction = std::fs::read_dir(&destination)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.path().is_dir() && is_move_transaction_directory(&entry.file_name())
            })
            .expect("destination transaction retains displaced destination");
        assert_eq!(
            std::fs::read(
                destination_transaction
                    .path()
                    .join(CROSS_DEVICE_STAGED_NAME)
            )
            .unwrap(),
            b"displaced destination payload"
        );

        let recovery_root = default_recovery_root().unwrap();
        let retained = discover_retained_move_transactions_at(&recovery_root).unwrap();
        assert_eq!(retained.len(), 1);
        recover_retained_move_pending_destination_at(&recovery_root, retained[0].transaction_id)
            .unwrap();

        assert_eq!(std::fs::read(&source).unwrap(), b"source payload");
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"displaced destination payload"
        );
        assert!(!has_local_transaction_dir(&dir.0));
        assert!(!has_local_transaction_dir(&destination));
        assert!(
            discover_retained_move_transactions_at(&recovery_root)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn cross_filesystem_overwrite_recovery_resumes_after_source_restore() {
        let dir = TestDir::new("cross-overwrite-resume-after-source-restore");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        let destination_target = destination.join("source");
        std::fs::write(&source, "source payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&destination_target, "displaced destination payload").unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Overwrite,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                TestPoint::Publication => Err("simulate interruption after exchange"),
                _ => Ok(()),
            },
        );
        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { .. })
        ));

        let source_transaction = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.path().is_dir() && is_move_transaction_directory(&entry.file_name())
            })
            .expect("source transaction remains for recovery");
        let destination_transaction = std::fs::read_dir(&destination)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.path().is_dir() && is_move_transaction_directory(&entry.file_name())
            })
            .expect("destination transaction remains for recovery");

        super::super::exchange_paths(
            &destination_transaction
                .path()
                .join(CROSS_DEVICE_STAGED_NAME),
            &destination_target,
        )
        .unwrap();
        std::fs::rename(source_transaction.path().join("claimed"), &source).unwrap();

        let recovery_root = default_recovery_root().unwrap();
        let retained = discover_retained_move_transactions_at(&recovery_root).unwrap();
        assert_eq!(retained.len(), 1);
        recover_retained_move_pending_destination_at(&recovery_root, retained[0].transaction_id)
            .unwrap();

        assert_eq!(std::fs::read(&source).unwrap(), b"source payload");
        assert_eq!(
            std::fs::read(&destination_target).unwrap(),
            b"displaced destination payload"
        );
        assert!(!has_local_transaction_dir(&dir.0));
        assert!(!has_local_transaction_dir(&destination));
        assert!(
            discover_retained_move_transactions_at(&recovery_root)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn cross_filesystem_overwrite_success_retains_displaced_destination_and_recovery_evidence() {
        let dir = TestDir::new("cross-overwrite-retained");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "source payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("source"), "displaced destination payload").unwrap();

        let publication = move_with_hooks(
            &source,
            &destination,
            ConflictAction::Overwrite,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                _ => Ok(()),
            },
        )
        .unwrap();

        assert_eq!(publication.disposition, MoveDisposition::Moved);
        assert_eq!(publication.published_path, Some(destination.join("source")));
        assert!(!source.exists());
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"source payload"
        );
        let retained_displaced_path = publication
            .retained_displaced_path
            .expect("overwrite must retain the displaced destination");
        assert_eq!(
            std::fs::read(&retained_displaced_path).unwrap(),
            b"displaced destination payload"
        );

        let retained =
            discover_retained_move_transactions_at(&default_recovery_root().unwrap()).unwrap();
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].phase, move_recovery::JournalPhase::Published);
    }

    #[test]
    fn cross_filesystem_published_overwrite_restart_finishes_source_cleanup_and_retains_undo_data()
    {
        let dir = TestDir::new("cross-overwrite-published-restart");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "source payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("source"), "displaced destination payload").unwrap();

        let publication = move_with_hooks(
            &source,
            &destination,
            ConflictAction::Overwrite,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                _ => Ok(()),
            },
        )
        .unwrap();
        let retained_displaced_path = publication
            .retained_displaced_path
            .expect("overwrite must retain displaced data");
        let recovery_root = default_recovery_root().unwrap();
        let retained = discover_retained_move_transactions_at(&recovery_root).unwrap();

        recover_retained_move_pending_destination_at(&recovery_root, retained[0].transaction_id)
            .unwrap();

        assert!(!source.exists());
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"source payload"
        );
        assert_eq!(
            std::fs::read(&retained_displaced_path).unwrap(),
            b"displaced destination payload"
        );
        let retained_after_restart =
            discover_retained_move_transactions_at(&recovery_root).unwrap();
        assert_eq!(retained_after_restart.len(), 1);
        assert_eq!(
            retained_after_restart[0].phase,
            move_recovery::JournalPhase::Published
        );
    }

    #[test]
    fn cross_filesystem_destination_parent_is_synced_after_publication_before_source_cleanup() {
        let dir = TestDir::new("cross-parent-sync-order");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "source payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        let mut observed = Vec::new();

        move_with_hooks(&source, &destination, ConflictAction::Skip, |point| {
            observed.push(point);
            match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                _ => Ok(()),
            }
        })
        .unwrap();

        let publication = observed
            .iter()
            .position(|point| *point == TestPoint::DestinationPublished)
            .expect("publication must be observable");
        let parent_sync = observed
            .iter()
            .position(|point| *point == TestPoint::DestinationParentSynced)
            .expect("post-publication destination-parent sync must be observable");
        let source_cleanup = observed
            .iter()
            .position(|point| *point == TestPoint::BeforeSourceCleanup)
            .expect("source cleanup boundary must be observable");
        assert!(publication < parent_sync);
        assert!(parent_sync < source_cleanup);
    }

    #[test]
    fn cross_filesystem_source_replacement_after_revalidation_is_not_deleted() {
        let dir = TestDir::new("cross-source-post-revalidation-race");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "source payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                TestPoint::AfterSourceRevalidation => {
                    let transaction = std::fs::read_dir(&dir.0)
                        .unwrap()
                        .filter_map(Result::ok)
                        .find(|entry| {
                            entry.path().is_dir()
                                && is_move_transaction_directory(&entry.file_name())
                        })
                        .expect("source transaction remains during cleanup");
                    std::fs::rename(
                        transaction.path().join("claimed"),
                        transaction.path().join("original-away"),
                    )
                    .unwrap();
                    std::fs::write(transaction.path().join("claimed"), "replacement payload")
                        .unwrap();
                    Ok(())
                }
                _ => Ok(()),
            },
        );

        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { .. })
        ));
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"source payload"
        );
        let transaction = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.path().is_dir() && is_move_transaction_directory(&entry.file_name())
            })
            .expect("source transaction remains for recovery");
        assert_eq!(
            std::fs::read(transaction.path().join("claimed")).unwrap(),
            b"replacement payload"
        );
        assert_eq!(
            std::fs::read(transaction.path().join("original-away")).unwrap(),
            b"source payload"
        );
    }

    #[test]
    fn cross_filesystem_recovery_handles_source_cleanup_quarantine() {
        let dir = TestDir::new("cross-source-cleanup-quarantine-recovery");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "source payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                TestPoint::SourceCleanupQuarantined => {
                    Err("simulate interruption after source quarantine")
                }
                _ => Ok(()),
            },
        );

        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { .. })
        ));
        let transaction = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.path().is_dir() && is_move_transaction_directory(&entry.file_name())
            })
            .expect("source transaction remains for recovery");
        assert_eq!(
            std::fs::read(transaction.path().join(SOURCE_CLEANUP_NAME)).unwrap(),
            b"source payload"
        );

        std::fs::rename(
            transaction.path().join(SOURCE_CLEANUP_NAME),
            transaction.path().join("original-away"),
        )
        .unwrap();
        std::fs::write(
            transaction.path().join(SOURCE_CLEANUP_NAME),
            "replacement payload",
        )
        .unwrap();

        let recovery_root = default_recovery_root().unwrap();
        let retained = discover_retained_move_transactions_at(&recovery_root).unwrap();
        assert_eq!(retained.len(), 1);
        let recovery_result = recover_retained_move_pending_destination_at(
            &recovery_root,
            retained[0].transaction_id,
        );

        assert!(recovery_result.is_err());
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"source payload"
        );
        assert_eq!(
            std::fs::read(transaction.path().join(SOURCE_CLEANUP_NAME)).unwrap(),
            b"replacement payload"
        );
        assert_eq!(
            std::fs::read(transaction.path().join("original-away")).unwrap(),
            b"source payload"
        );

        std::fs::remove_file(transaction.path().join(SOURCE_CLEANUP_NAME)).unwrap();
        std::fs::rename(
            transaction.path().join("original-away"),
            transaction.path().join(SOURCE_CLEANUP_NAME),
        )
        .unwrap();
        recover_retained_move_pending_destination_at(&recovery_root, retained[0].transaction_id)
            .unwrap();

        assert!(!source.exists());
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"source payload"
        );
        assert!(!has_local_transaction_dir(&dir.0));
    }

    #[test]
    fn cross_filesystem_cleanup_preserves_quarantine_replacement() {
        let dir = TestDir::new("cross-source-cleanup-quarantine-replacement");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        std::fs::write(&source, "source payload").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| match point {
                TestPoint::ClassifyCrossDevice => Err("force cross-device move"),
                TestPoint::SourceCleanupQuarantined => {
                    let transaction = std::fs::read_dir(&dir.0)
                        .unwrap()
                        .filter_map(Result::ok)
                        .find(|entry| {
                            entry.path().is_dir()
                                && is_move_transaction_directory(&entry.file_name())
                        })
                        .expect("source transaction remains until cleanup completes");
                    std::fs::rename(
                        transaction.path().join(SOURCE_CLEANUP_NAME),
                        transaction.path().join("original-away"),
                    )
                    .unwrap();
                    std::fs::write(
                        transaction.path().join(SOURCE_CLEANUP_NAME),
                        "replacement payload",
                    )
                    .unwrap();
                    Ok(())
                }
                _ => Ok(()),
            },
        );

        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { .. })
        ));
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"source payload"
        );
        let transaction = std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.path().is_dir() && is_move_transaction_directory(&entry.file_name())
            })
            .expect("source transaction remains for recovery");
        assert_eq!(
            std::fs::read(transaction.path().join(SOURCE_CLEANUP_NAME)).unwrap(),
            b"replacement payload"
        );
        assert_eq!(
            std::fs::read(transaction.path().join("original-away")).unwrap(),
            b"source payload"
        );
    }

    #[test]
    fn same_filesystem_publication_uses_verified_destination_parent_after_destination_alias_retarget()
     {
        let dir = TestDir::new("destination-handle-rebind");
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        let retired = dir.0.join("destination-retired");
        let attacker = dir.0.join("attacker");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::create_dir(&retired).unwrap();
        std::fs::create_dir(&attacker).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| {
                if point == TestPoint::PendingDestination {
                    std::fs::rename(&destination, &retired).unwrap();
                    std::fs::rename(&attacker, &destination).unwrap();
                }
                Ok(())
            },
        );
        assert!(result.is_ok(), "move failed: {result:?}");
        assert_eq!(std::fs::read(retired.join("source")).unwrap(), b"payload");
        assert!(!destination.join("source").exists());
        assert!(!source.exists());
    }

    #[test]
    fn same_filesystem_restore_uses_verified_source_parent_after_source_alias_retarget() {
        let dir = TestDir::new("source-handle-rebind");
        let source_parent_real = dir.0.join("source-parent");
        let source_parent_alias = dir.0.join("source-parent-link");
        let source = source_parent_alias.join("source");
        let destination = dir.0.join("destination");
        let attacker = dir.0.join("attacker");
        std::fs::create_dir(&source_parent_real).unwrap();
        std::fs::create_dir(&attacker).unwrap();
        std::fs::create_dir(&destination).unwrap();
        use std::os::unix::fs::symlink;
        symlink(&source_parent_real, &source_parent_alias).unwrap();
        std::fs::write(&source, "payload").unwrap();
        std::fs::write(destination.join("source"), "occupied").unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| {
                if point == TestPoint::PendingDestination {
                    let _ = std::fs::remove_file(&source_parent_alias);
                    symlink(&attacker, &source_parent_alias).unwrap();
                }
                Ok(())
            },
        );
        assert!(result.is_ok());
        assert_eq!(
            std::fs::read(source_parent_real.join("source")).unwrap(),
            b"payload"
        );
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"occupied"
        );
        assert!(!attacker.join("source").exists());
    }

    #[cfg(target_os = "linux")]
    fn run_publication_sigkill_child(root: &Path) {
        use rustix::process;

        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| {
                if point == TestPoint::Publication {
                    let _ = process::kill_process(process::getpid(), process::Signal::Kill);
                    panic!("publication child was not terminated");
                } else {
                    Ok(())
                }
            },
        );
        panic!("publication move returned {result:?} without being terminated");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn same_filesystem_publication_kill_child_process_after_publication_preserves_volatile_state() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_PUBLICATION_SIGKILL_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root = std::env::var_os("OTTRIN_MOVE_PUBLICATION_SIGKILL_ROOT").unwrap();
                run_publication_sigkill_child(Path::new(&root));
                panic!("publication kill child returned without being terminated");
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-publication-kill-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_transaction::tests::same_filesystem_publication_kill_child_process_after_publication_preserves_volatile_state",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_PUBLICATION_SIGKILL_CHILD", "1")
            .env("OTTRIN_MOVE_PUBLICATION_SIGKILL_ROOT", &root)
            .env("XDG_STATE_HOME", &state_root)
            .status()
            .unwrap();
            assert_eq!(status.signal(), Some(9));
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"payload"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            let has_central_journal_after_kill =
                std::fs::read_dir(state_root.join("ottrin").join("moves"))
                    .unwrap()
                    .filter_map(Result::ok)
                    .any(|entry| {
                        entry
                            .path()
                            .extension()
                            .is_some_and(|ext| ext == std::ffi::OsStr::new("journal"))
                    });
            assert!(has_central_journal_after_kill);
            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }
            assert!(has_local_transaction_dir(&root));

            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Skip,
                |_| Ok(()),
            );
            assert!(recovery.is_ok(), "recovery retry failed: {recovery:?}");
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"payload"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            assert!(!has_local_transaction_dir(&root));

            let has_central_journal = std::fs::read_dir(state_root.join("ottrin").join("moves"))
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == std::ffi::OsStr::new("journal"))
                });
            assert!(!has_central_journal);
        });
    }

    #[cfg(target_os = "linux")]
    fn run_overwrite_publication_sigkill_child(root: &Path) {
        use rustix::process;

        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::write(&source, "moved payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("source"), "displaced payload").unwrap();
        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Overwrite,
            |point| {
                if point == TestPoint::Publication {
                    let _ = process::kill_process(process::getpid(), process::Signal::Kill);
                    panic!("overwrite publication child was not terminated");
                } else {
                    Ok(())
                }
            },
        );
        panic!("overwrite publication move returned {result:?} without being terminated");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn same_filesystem_overwrite_publication_sigkill_recovers_both_entries_after_restart() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_OVERWRITE_SIGKILL_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root = std::env::var_os("OTTRIN_MOVE_OVERWRITE_SIGKILL_ROOT").unwrap();
                run_overwrite_publication_sigkill_child(Path::new(&root));
                panic!("overwrite publication child returned without being terminated");
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-overwrite-publication-kill-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "move_transaction::tests::same_filesystem_overwrite_publication_sigkill_recovers_both_entries_after_restart",
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env("OTTRIN_MOVE_OVERWRITE_SIGKILL_CHILD", "1")
                .env("OTTRIN_MOVE_OVERWRITE_SIGKILL_ROOT", &root)
                .env("XDG_STATE_HOME", &state_root)
                .status()
                .unwrap();
            assert_eq!(status.signal(), Some(9));
            assert!(!root.join("source").exists());
            assert_eq!(
                std::fs::read_to_string(root.join("destination").join("source")).unwrap(),
                "moved payload"
            );

            let recovery_root = state_root.join("ottrin").join("moves");
            let retained = discover_retained_move_transactions_at(&recovery_root).unwrap();
            assert_eq!(retained.len(), 1);
            recover_retained_move_pending_destination_at(
                &recovery_root,
                retained[0].transaction_id,
            )
            .unwrap();

            assert_eq!(
                std::fs::read_to_string(root.join("source")).unwrap(),
                "moved payload"
            );
            assert_eq!(
                std::fs::read_to_string(root.join("destination").join("source")).unwrap(),
                "displaced payload"
            );
            assert!(
                discover_retained_move_transactions_at(&recovery_root)
                    .unwrap()
                    .is_empty()
            );
            assert!(!has_local_transaction_dir(&root));
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    #[cfg(target_os = "linux")]
    fn run_pending_destination_sigkill_child(root: &Path) {
        use rustix::process;

        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(destination.join("source"), b"occupied").unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Rename,
            |point| {
                if point == TestPoint::PendingDestination {
                    let _ = process::kill_process(process::getpid(), process::Signal::Kill);
                    panic!("pending destination child was not terminated");
                } else {
                    Ok(())
                }
            },
        );
        panic!("pending destination move returned {result:?} without being terminated");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn same_filesystem_rename_pending_destination_kill_child_process_replays_to_next_available_name()
     {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root =
                    std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_ROOT").unwrap();
                run_pending_destination_sigkill_child(Path::new(&root));
                panic!("pending destination kill child returned without being terminated");
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-pending-destination-kill-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_transaction::tests::same_filesystem_rename_pending_destination_kill_child_process_replays_to_next_available_name",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_CHILD", "1")
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_ROOT", &root)
            .env("XDG_STATE_HOME", &state_root)
            .status()
            .unwrap();
            assert_eq!(status.signal(), Some(9));
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }

            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Rename,
                |_| Ok(()),
            );
            assert!(recovery.is_ok(), "recovery retry failed: {recovery:?}");
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert_eq!(
                std::fs::read(root.join("destination").join("source (1)")).unwrap(),
                b"payload"
            );

            assert!(!has_local_transaction_dir(&root));

            let has_central_journal = std::fs::read_dir(state_root.join("ottrin").join("moves"))
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == std::ffi::OsStr::new("journal"))
                });
            assert!(!has_central_journal);
        });
    }

    #[cfg(target_os = "linux")]
    fn run_cross_filesystem_pending_destination_sigkill_child(
        root: &Path,
        conflict: ConflictAction,
    ) {
        use rustix::process;

        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(destination.join("source"), b"occupied").unwrap();

        let result =
            move_entry_transactional_with_test_hook(&source, &destination, conflict, |point| {
                if point == TestPoint::ClassifyCrossDevice {
                    Err("force cross-device move")
                } else if point == TestPoint::PendingDestination {
                    let _ = process::kill_process(process::getpid(), process::Signal::Kill);
                    panic!("cross-device pending destination child was not terminated");
                } else {
                    Ok(())
                }
            });
        panic!(
            "cross-device pending destination move returned {result:?} without being terminated"
        );
    }

    #[cfg(target_os = "linux")]
    fn run_cross_filesystem_unoccupied_pending_destination_sigkill_child(root: &Path) {
        use rustix::process;

        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::write(&source, "payload").unwrap();
        std::fs::create_dir_all(&destination).unwrap();

        let result = move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Skip,
            |point| {
                if point == TestPoint::ClassifyCrossDevice {
                    Err("force cross-device move")
                } else if point == TestPoint::PendingDestination {
                    let _ = process::kill_process(process::getpid(), process::Signal::Kill);
                    panic!("cross-device pending destination child was not terminated");
                } else {
                    Ok(())
                }
            },
        );
        panic!(
            "cross-device pending destination move returned {result:?} without being terminated"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_replay_rejects_staged_content_changed_without_identity_change() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_REPLAY_CONTENT_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root = std::env::var_os("OTTRIN_MOVE_REPLAY_CONTENT_ROOT").unwrap();
                run_cross_filesystem_unoccupied_pending_destination_sigkill_child(Path::new(&root));
                panic!("cross-device replay-content child returned without being terminated");
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-cross-replay-content-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "move_transaction::tests::cross_filesystem_replay_rejects_staged_content_changed_without_identity_change",
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env("OTTRIN_MOVE_REPLAY_CONTENT_CHILD", "1")
                .env("OTTRIN_MOVE_REPLAY_CONTENT_ROOT", &root)
                .env("XDG_STATE_HOME", &state_root)
                .status()
                .unwrap();
            assert_eq!(status.signal(), Some(9));

            let destination_transaction = std::fs::read_dir(root.join("destination"))
                .unwrap()
                .filter_map(Result::ok)
                .find(|entry| is_move_transaction_directory(&entry.file_name()))
                .expect("destination transaction exists after staging");
            let staged = destination_transaction
                .path()
                .join(CROSS_DEVICE_STAGED_NAME);
            let staged_identity =
                stable_identity_from_metadata(&std::fs::symlink_metadata(&staged).unwrap());
            std::fs::write(&staged, b"corrupt").unwrap();
            assert_eq!(
                stable_identity_from_metadata(&std::fs::symlink_metadata(&staged).unwrap()),
                staged_identity,
                "the corruption must preserve the staged inode identity"
            );

            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }
            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Skip,
                |point| {
                    if point == TestPoint::ClassifyCrossDevice {
                        Err("force cross-device move")
                    } else {
                        Ok(())
                    }
                },
            );
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }

            assert!(matches!(
                recovery,
                Err(PlatformError::CommittedCleanupPending { message, .. })
                    if message == "recovery staged destination content does not match source snapshot"
            ));
            assert!(!root.join("destination").join("source").exists());
            let source_transaction = std::fs::read_dir(&root)
                .unwrap()
                .filter_map(Result::ok)
                .find(|entry| is_move_transaction_directory(&entry.file_name()))
                .expect("claimed source remains recoverable");
            assert_eq!(
                std::fs::read(source_transaction.path().join("claimed")).unwrap(),
                b"payload"
            );
            assert_eq!(std::fs::read(staged).unwrap(), b"corrupt");
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_replay_syncs_destination_parent_before_completion() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_REPLAY_SYNC_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root = std::env::var_os("OTTRIN_MOVE_REPLAY_SYNC_ROOT").unwrap();
                run_cross_filesystem_unoccupied_pending_destination_sigkill_child(Path::new(&root));
                panic!("cross-device replay-sync child returned without being terminated");
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-cross-replay-sync-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "move_transaction::tests::cross_filesystem_replay_syncs_destination_parent_before_completion",
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env("OTTRIN_MOVE_REPLAY_SYNC_CHILD", "1")
                .env("OTTRIN_MOVE_REPLAY_SYNC_ROOT", &root)
                .env("XDG_STATE_HOME", &state_root)
                .status()
                .unwrap();
            assert_eq!(status.signal(), Some(9));

            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }
            let mut observed = Vec::new();
            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Skip,
                |point| {
                    observed.push(point);
                    if point == TestPoint::ClassifyCrossDevice {
                        Err("force cross-device move")
                    } else {
                        Ok(())
                    }
                },
            );
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }
            assert!(recovery.is_ok(), "recovery retry failed: {recovery:?}");

            let publication = observed
                .iter()
                .position(|point| *point == TestPoint::DestinationPublished)
                .expect("replay publication must be observable");
            let parent_sync = observed
                .iter()
                .position(|point| *point == TestPoint::DestinationParentSynced)
                .expect("replay destination-parent sync must be observable");
            assert!(publication < parent_sync);
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"payload"
            );
            assert!(!root.join("source").exists());
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_rename_pending_destination_kill_child_process_replays_to_next_available_name()
     {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root =
                    std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_ROOT").unwrap();
                run_cross_filesystem_pending_destination_sigkill_child(
                    Path::new(&root),
                    ConflictAction::Rename,
                );
                panic!(
                    "cross-device pending destination kill child returned without being terminated"
                );
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-cross-pending-destination-kill-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_transaction::tests::cross_filesystem_rename_pending_destination_kill_child_process_replays_to_next_available_name",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_CHILD", "1")
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SIGKILL_ROOT", &root)
            .env("XDG_STATE_HOME", &state_root)
            .status()
            .unwrap();
            assert_eq!(status.signal(), Some(9));
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }

            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Rename,
                |point| {
                    if point == TestPoint::ClassifyCrossDevice {
                        Err("force cross-device move")
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(recovery.is_ok(), "recovery retry failed: {recovery:?}");
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert_eq!(
                std::fs::read(root.join("destination").join("source (1)")).unwrap(),
                b"payload"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            assert!(!has_local_transaction_dir(&root));

            let has_central_journal = std::fs::read_dir(state_root.join("ottrin").join("moves"))
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == std::ffi::OsStr::new("journal"))
                });
            assert!(!has_central_journal);
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_skip_pending_destination_kill_child_process_restores_source_on_recovery() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SKIP_SIGKILL_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root =
                    std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_SKIP_SIGKILL_ROOT").unwrap();
                run_cross_filesystem_pending_destination_sigkill_child(
                    Path::new(&root),
                    ConflictAction::Skip,
                );
                panic!(
                    "cross-device skip pending destination kill child returned without being terminated"
                );
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-cross-pending-destination-skip-kill-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_transaction::tests::cross_filesystem_skip_pending_destination_kill_child_process_restores_source_on_recovery",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SKIP_SIGKILL_CHILD", "1")
            .env("OTTRIN_MOVE_PENDING_DESTINATION_SKIP_SIGKILL_ROOT", &root)
            .env("XDG_STATE_HOME", &state_root)
            .status()
            .unwrap();
            assert_eq!(status.signal(), Some(9));
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());

            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }
            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Skip,
                |point| {
                    if point == TestPoint::ClassifyCrossDevice {
                        Err("force cross-device move")
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(recovery.is_ok(), "recovery retry failed: {recovery:?}");
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }
            assert_eq!(std::fs::read(root.join("source")).unwrap(), b"payload");
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert!(!has_local_transaction_dir(&root));

            let has_central_journal = std::fs::read_dir(state_root.join("ottrin").join("moves"))
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == std::ffi::OsStr::new("journal"))
                });
            assert!(!has_central_journal);
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_recovery_requires_destination_transaction() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        with_move_env_serializer(|| {
            if std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_VANISHED_SIGKILL_CHILD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                let root =
                    std::env::var_os("OTTRIN_MOVE_PENDING_DESTINATION_VANISHED_SIGKILL_ROOT")
                        .unwrap();
                run_cross_filesystem_pending_destination_sigkill_child(
                    Path::new(&root),
                    ConflictAction::Rename,
                );
                panic!(
                    "cross-device vanished destination tx kill child returned without being terminated"
                );
            }

            let root = std::env::temp_dir().join(format!(
                "ottrin-move-cross-pending-destination-vanish-{}",
                getrandom::u64().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            let state_root = root.join("state");
            std::fs::create_dir_all(&state_root).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_transaction::tests::cross_filesystem_recovery_requires_destination_transaction",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_PENDING_DESTINATION_VANISHED_SIGKILL_CHILD", "1")
            .env("OTTRIN_MOVE_PENDING_DESTINATION_VANISHED_SIGKILL_ROOT", &root)
            .env("XDG_STATE_HOME", &state_root)
            .status()
            .unwrap();
            assert_eq!(status.signal(), Some(9));

            for entry in std::fs::read_dir(root.join("destination"))
                .unwrap()
                .filter_map(Result::ok)
            {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".ottrin-move-")
                {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }

            let previous_state_home = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &state_root);
            }
            let recovery = move_entry_transactional_with_test_hook(
                &root.join("source"),
                &root.join("destination"),
                ConflictAction::Rename,
                |point| {
                    if point == TestPoint::ClassifyCrossDevice {
                        Err("force cross-device move")
                    } else {
                        Ok(())
                    }
                },
            );
            if let Some(previous_state_home) = previous_state_home {
                unsafe {
                    std::env::set_var("XDG_STATE_HOME", previous_state_home);
                }
            } else {
                unsafe {
                    std::env::remove_var("XDG_STATE_HOME");
                }
            }
            assert!(matches!(
                recovery,
                Err(PlatformError::CommittedCleanupPending { .. })
            ));
            assert!(!std::fs::exists(root.join("source")).unwrap_or_default());
            assert_eq!(
                std::fs::read(root.join("destination").join("source")).unwrap(),
                b"occupied"
            );
            assert!(has_local_transaction_dir(&root));
        });
    }
}
