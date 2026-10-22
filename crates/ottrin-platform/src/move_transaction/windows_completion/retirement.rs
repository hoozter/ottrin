//! Explicit, queued retirement of old controls. The immutable completion receipt
//! remains authoritative. An intent precedes write exclusion, so a later process
//! can recognize and restore only this operation's known transitional policies.
use super::*;
use crate::windows_fs::{
    PerObjectFile,
    journal_access::{Policy, State},
};
use std::collections::BTreeSet;
use windows_sys::Win32::Storage::FileSystem::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsMoveCleanupReport {
    pub journals_removed: usize,
    pub directories_removed: usize,
    pub directories_kept: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    identity: Identity,
    policy: Policy,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
enum Phase {
    Intent(Vec<Node>),
    Complete(WindowsMoveCleanupReport),
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EventBody {
    version: u16,
    root: Identity,
    file: Identity,
    transaction: [u8; 16],
    completion_file: Identity,
    completion_checksum: [u8; 32],
    phase: Phase,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    body: EventBody,
    checksum: [u8; 32],
}
fn digest(body: &EventBody) -> Result<[u8; 32], PlatformError> {
    let mut hash = Sha256::new();
    hash.update(b"Ottrin Move control retirement v1\0");
    hash.update(serde_json::to_vec(body).map_err(io)?);
    Ok(hash.finalize().into())
}
fn event(root: &File, receipt: &Receipt, kind: RecordKind) -> Result<Option<Event>, PlatformError> {
    let Some((identity, bytes)) = read_native_record(root, &record_name(receipt.id()?, kind))?
    else {
        return Ok(None);
    };
    let value: Event = serde_json::from_slice(&bytes).map_err(io)?;
    if value.body.version != 1
        || value.body.root != receipt.body.root
        || value.body.file != identity
        || value.body.transaction != receipt.id()?
        || value.body.completion_file != receipt.body.file
        || value.body.completion_checksum != receipt.checksum
        || value.checksum != digest(&value.body)?
    {
        return Err(io("retirement event identity or checksum disagrees"));
    }
    match (&value.body.phase, kind) {
        (Phase::Intent(nodes), RecordKind::RetirementIntent) => {
            let allowed = receipt
                .body
                .controls
                .iter()
                .flat_map(|o| [o.identity, o.central.identity, o.local.identity])
                .collect::<Vec<_>>();
            let mut seen = BTreeSet::new();
            for node in nodes {
                if !node.policy.valid()
                    || !allowed.contains(&node.identity)
                    || !seen.insert(node.identity.object_key())
                {
                    return Err(io("invalid retirement access intent"));
                }
            }
        }
        (Phase::Complete(_), RecordKind::RetirementComplete) => {}
        _ => return Err(io("retirement event has the wrong phase")),
    }
    Ok(Some(value))
}
fn write_event(
    root: &Path,
    parent: &File,
    receipt: &Receipt,
    phase: Phase,
) -> Result<(), PlatformError> {
    let kind = if matches!(phase, Phase::Intent(_)) {
        RecordKind::RetirementIntent
    } else {
        RecordKind::RetirementComplete
    };
    write_native_record(root, parent, receipt.id()?, kind, |root, file| {
        let body = EventBody {
            version: 1,
            root,
            file,
            transaction: receipt.id()?,
            completion_file: receipt.body.file,
            completion_checksum: receipt.checksum,
            phase,
        };
        let value = Event {
            checksum: digest(&body)?,
            body,
        };
        serde_json::to_vec(&value).map_err(io)
    })
}
pub(in crate::move_transaction) fn complete(
    root: &Path,
    receipt: &Receipt,
) -> Result<bool, PlatformError> {
    let parent = windows_fs::open_directory(root, FILE_GENERIC_READ).map_err(io)?;
    Ok(event(&parent, receipt, RecordKind::RetirementComplete)?.is_some())
}
fn central_name(owner: &controls::Owner) -> std::ffi::OsString {
    let path = match owner.kind {
        WindowsJournalKind::SameVolumeOverwrite => {
            windows_journal_path(Path::new(""), owner.transaction)
        }
        WindowsJournalKind::CrossVolume => {
            windows_cross_volume_journal_path(Path::new(""), owner.transaction)
        }
    };
    path.into_os_string()
}
fn intent_policy(intent: Option<&Event>, identity: Identity) -> Option<&Policy> {
    let Phase::Intent(nodes) = &intent?.body.phase else {
        return None;
    };
    nodes
        .iter()
        .find(|n| n.identity == identity)
        .map(|n| &n.policy)
}
fn policy(
    file: &File,
    directory: bool,
    identity: Identity,
    intent: Option<&Event>,
) -> Result<Policy, PlatformError> {
    let (policy, state) = Policy::observe(file, directory)?;
    if let Some(expected) = intent_policy(intent, identity) {
        if expected != &policy {
            return Err(io("retirement access evidence changed"));
        }
    } else if state == State::Frozen || intent.is_some() {
        return Err(io("write exclusion has no matching retirement intent"));
    }
    Ok(policy)
}
fn file_safe(file: &File) -> Result<(), PlatformError> {
    if !windows_stream_set_is_copy_safe(windows_file_streams(file)?, false)
        || windows_extended_attribute_size(file)? != 0
    {
        return Err(io("journal has additional data; it was preserved"));
    }
    let mut standard = FILE_STANDARD_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle().cast(),
            FileStandardInfo,
            ptr::from_mut(&mut standard).cast(),
            mem::size_of_val(&standard) as u32,
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    if standard.NumberOfLinks != 1 {
        return Err(io("journal has additional links; it was preserved"));
    }
    Ok(())
}
struct Journal {
    file: PerObjectFile,
    policy: Policy,
    evidence: controls::FileEvidence,
}
impl Journal {
    fn open(
        parent: &File,
        name: &OsStr,
        evidence: &controls::FileEvidence,
        intent: Option<&Event>,
    ) -> Result<Option<Self>, PlatformError> {
        let file = match PerObjectFile::open(parent, name) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io(e)),
        };
        evidence.verify(&file)?;
        file_safe(&file)?;
        let policy = policy(&file, false, evidence.identity, intent)?;
        Ok(Some(Self {
            file,
            policy,
            evidence: evidence.clone(),
        }))
    }
    fn verify(&mut self) -> Result<(), PlatformError> {
        self.evidence.verify(&self.file)?;
        file_safe(&self.file)
    }
    fn node(&self) -> Node {
        Node {
            identity: self.evidence.identity,
            policy: self.policy.clone(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Directory {
    Present {
        path: PathBuf,
        parent: PathBuf,
        parent_identity: Identity,
        policy: Policy,
        eligible: bool,
    },
    Absent {
        volume: PathBuf,
        volume_identity: Identity,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Group {
    owner: controls::Owner,
    directory: Directory,
    central: bool,
    local: bool,
}
/// Opaque mutation admission, captured off-thread. Paths include each actual
/// parent/private directory or the known volume proving a retired ID absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsMoveCleanupPlan {
    root: PathBuf,
    receipt: Receipt,
    groups: Vec<Group>,
    paths: Vec<PathBuf>,
    intent: Option<Event>,
}
impl WindowsMoveCleanupPlan {
    pub fn capture(transaction: [u8; 16]) -> Result<Option<Self>, PlatformError> {
        let root = windows_recovery_root()?;
        let receipt = find(&root, transaction)?
            .ok_or_else(|| io("Undo completion receipt is unavailable"))?;
        let parent = windows_fs::open_directory(&root, FILE_GENERIC_READ).map_err(io)?;
        if event(&parent, &receipt, RecordKind::RetirementComplete)?.is_some() {
            return Ok(None);
        }
        let intent = event(&parent, &receipt, RecordKind::RetirementIntent)?;
        let mut volumes = windows_fs::Volumes::new(&parent)?;
        let mut groups = Vec::new();
        let mut paths = vec![root.clone()];
        for owner in &receipt.body.controls {
            let central = Journal::open(
                &parent,
                &central_name(owner),
                &owner.central,
                intent.as_ref(),
            )?
            .is_some();
            let hint = owner.directory.decode().map_err(io)?;
            let private = match windows_fs::open_directory(&hint, FILE_GENERIC_READ) {
                Ok(f)
                    if move_recovery::stable_identity_from_handle(&f).map_err(io)?
                        == owner.identity =>
                {
                    Some(f)
                }
                _ => {
                    volumes.resolve_optional(owner.identity, FILE_GENERIC_READ, &mut |_| Ok(()))?
                }
            };
            let (directory, local) = if let Some(private) = private {
                let path = windows_fs::location(&private).map_err(io)?;
                let parent_path = path
                    .parent()
                    .ok_or_else(|| io("private directory has no parent"))?
                    .to_path_buf();
                let held =
                    windows_fs::open_directory(&parent_path, FILE_GENERIC_READ).map_err(io)?;
                let parent_identity =
                    move_recovery::stable_identity_from_handle(&held).map_err(io)?;
                let eligible = parent_identity == owner.parent_identity
                    && path.file_name() == hint.file_name();
                let policy = policy(&private, true, owner.identity, intent.as_ref())?;
                let local = Journal::open(
                    &private,
                    OsStr::new(WINDOWS_LOCAL_JOURNAL_NAME),
                    &owner.local,
                    intent.as_ref(),
                )?
                .is_some();
                paths.extend([path.clone(), parent_path.clone()]);
                (
                    Directory::Present {
                        path,
                        parent: parent_path,
                        parent_identity,
                        policy,
                        eligible,
                    },
                    local,
                )
            } else {
                let anchor = volumes.volume_anchor(owner.identity)?;
                let volume = windows_fs::location(anchor)
                    .map_err(|e| io(format!("cleanup volume anchor location: {e}")))?;
                let volume_identity =
                    move_recovery::stable_identity_from_handle(anchor).map_err(io)?;
                paths.push(volume.clone());
                (
                    Directory::Absent {
                        volume,
                        volume_identity,
                    },
                    false,
                )
            };
            groups.push(Group {
                owner: owner.clone(),
                directory,
                central,
                local,
            });
        }
        paths.sort();
        paths.dedup();
        Ok(Some(Self {
            root,
            receipt,
            groups,
            paths,
            intent,
        }))
    }
    pub(in crate::move_transaction) fn matches_completion(&self, receipt: &Receipt) -> bool {
        self.receipt == *receipt
    }
    pub fn transaction_id(&self) -> [u8; 16] {
        self.receipt.id().expect("validated completion ID")
    }
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
    pub fn execute(&self) -> Result<WindowsMoveCleanupReport, PlatformError> {
        let _lock = acquire_windows_recovery_lock()?;
        self.execute_locked()
    }
    fn execute_locked(&self) -> Result<WindowsMoveCleanupReport, PlatformError> {
        if windows_recovery_root()? != self.root
            || find(&self.root, self.transaction_id())?.as_ref() != Some(&self.receipt)
        {
            return Err(io("cleanup completion evidence changed; refresh Activity"));
        }
        let parent = windows_fs::open_directory_with_sharing(
            &self.root,
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )
        .map_err(io)?;
        if let Some(done) = event(&parent, &self.receipt, RecordKind::RetirementComplete)? {
            let Phase::Complete(report) = done.body.phase else {
                unreachable!()
            };
            parent.sync_all().map_err(io)?;
            return Ok(report);
        }
        let intent = event(&parent, &self.receipt, RecordKind::RetirementIntent)?;
        if self.intent.is_some() && self.intent != intent {
            return Err(io("cleanup intent changed"));
        }
        let mut held = Vec::new();
        for group in &self.groups {
            held.push(Held::open(&parent, group, intent.as_ref())?);
        }
        if intent.is_none() {
            let mut nodes = Vec::new();
            for group in &held {
                nodes.extend(group.nodes()?);
            }
            write_event(&self.root, &parent, &self.receipt, Phase::Intent(nodes))?;
        }
        let result: Result<WindowsMoveCleanupReport, PlatformError> = (|| {
            for group in &held {
                group.freeze()?;
            }
            #[cfg(test)]
            windows_move_boundary(WindowsMoveBoundary::RetirementAccessFrozen, &self.root);
            for group in &mut held {
                group.verify()?;
            }
            let mut report = WindowsMoveCleanupReport::default();
            for group in &mut held {
                group.retire_local(&mut report)?;
            }
            for group in &mut held {
                group.retire_central(&parent, &mut report)?;
            }
            parent.sync_all().map_err(io)?;
            // Restore any preserved directory before acknowledging completion.
            for group in &held {
                group.restore()?;
            }
            write_event(
                &self.root,
                &parent,
                &self.receipt,
                Phase::Complete(report.clone()),
            )?;
            Ok(report)
        })();
        if let Err(original) = &result {
            let mut errors = Vec::new();
            for group in &held {
                if let Err(error) = group.restore() {
                    errors.push(error.to_string());
                }
            }
            if !errors.is_empty() {
                return Err(io(format!(
                    "{original}; access restoration also failed: {}",
                    errors.join("; ")
                )));
            }
        }
        result
    }
}
struct Held {
    central: Option<Journal>,
    central_name: std::ffi::OsString,
    local: Option<Journal>,
    private: Option<PerObjectFile>,
    parent: Option<File>,
    policy: Option<Policy>,
    eligible: bool,
    private_name: Option<std::ffi::OsString>,
}
impl Held {
    fn open(root: &File, group: &Group, intent: Option<&Event>) -> Result<Self, PlatformError> {
        let central = Journal::open(
            root,
            &central_name(&group.owner),
            &group.owner.central,
            intent,
        )?;
        if central.is_some() != group.central {
            return Err(io("central journal presence changed; refresh Activity"));
        }
        match &group.directory {
            Directory::Absent {
                volume,
                volume_identity,
            } => {
                let anchor = windows_fs::open_directory(volume, FILE_GENERIC_READ).map_err(io)?;
                if move_recovery::stable_identity_from_handle(&anchor).map_err(io)?
                    != *volume_identity
                {
                    return Err(io("cleanup volume changed"));
                }
                match windows_fs::open_directory_by_identity(
                    &anchor,
                    group.owner.identity,
                    FILE_GENERIC_READ,
                ) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(io(e)),
                    Ok(_) => return Err(io("private directory reappeared; refresh Activity")),
                }
                Ok(Self {
                    central,
                    central_name: central_name(&group.owner),
                    local: None,
                    private: None,
                    parent: None,
                    policy: None,
                    eligible: false,
                    private_name: None,
                })
            }
            Directory::Present {
                path,
                parent,
                parent_identity,
                policy: expected,
                eligible,
            } => {
                let parent = windows_fs::open_directory_with_sharing(
                    parent,
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                )
                .map_err(io)?;
                if move_recovery::stable_identity_from_handle(&parent).map_err(io)?
                    != *parent_identity
                {
                    return Err(io("private parent changed"));
                }
                let mut private =
                    PerObjectFile::open(&parent, path.file_name().unwrap()).map_err(io)?;
                if move_recovery::stable_identity_from_handle(&private).map_err(io)?
                    != group.owner.identity
                {
                    return Err(io("private directory changed"));
                }
                if policy(&private, true, group.owner.identity, intent)? != *expected {
                    return Err(io("private access policy changed"));
                }
                // A post-crash handle opened under the frozen policy lacks
                // directory-flush rights. Restore the known original state,
                // then reacquire the full per-object capability before work.
                if Policy::observe(&private, true)?.1 == State::Frozen {
                    expected.apply(&private, State::Full)?;
                    drop(private);
                    private =
                        PerObjectFile::open(&parent, path.file_name().unwrap()).map_err(io)?;
                    if move_recovery::stable_identity_from_handle(&private).map_err(io)?
                        != group.owner.identity
                        || policy(&private, true, group.owner.identity, intent)? != *expected
                    {
                        return Err(io("private directory changed while restoring access"));
                    }
                }
                let local = Journal::open(
                    &private,
                    OsStr::new(WINDOWS_LOCAL_JOURNAL_NAME),
                    &group.owner.local,
                    intent,
                )?;
                if local.is_some() != group.local {
                    return Err(io("local journal presence changed; refresh Activity"));
                }
                Ok(Self {
                    central,
                    central_name: central_name(&group.owner),
                    local,
                    private: Some(private),
                    parent: Some(parent),
                    policy: Some(expected.clone()),
                    eligible: *eligible,
                    private_name: Some(path.file_name().unwrap().to_os_string()),
                })
            }
        }
    }
    fn nodes(&self) -> Result<Vec<Node>, PlatformError> {
        let mut nodes = Vec::new();
        if let (Some(private), Some(policy)) = (&self.private, &self.policy) {
            nodes.push(Node {
                identity: move_recovery::stable_identity_from_handle(private).map_err(io)?,
                policy: policy.clone(),
            });
        }
        for file in [&self.central, &self.local].into_iter().flatten() {
            nodes.push(file.node());
        }
        Ok(nodes)
    }
    fn freeze(&self) -> Result<(), PlatformError> {
        if let (Some(private), Some(policy)) = (&self.private, &self.policy) {
            policy.apply(private, State::Frozen)?;
        }
        for file in [&self.central, &self.local].into_iter().flatten() {
            file.policy.apply(&file.file, State::Frozen)?;
        }
        Ok(())
    }
    fn restore(&self) -> Result<(), PlatformError> {
        for file in [&self.central, &self.local].into_iter().flatten() {
            file.policy.apply(&file.file, State::Full)?;
        }
        if let (Some(private), Some(policy)) = (&self.private, &self.policy) {
            policy.apply(private, State::Full)?;
            private.sync_all().map_err(io)?;
        }
        Ok(())
    }
    fn verify(&mut self) -> Result<(), PlatformError> {
        for file in [&mut self.central, &mut self.local].into_iter().flatten() {
            file.verify()?;
        }
        Ok(())
    }
    fn retire_local(&mut self, report: &mut WindowsMoveCleanupReport) -> Result<(), PlatformError> {
        if let Some(local) = self.local.as_ref() {
            remove(&local.file)?;
            self.local.take();
            if windows_undo::observe(
                self.private.as_ref().unwrap(),
                OsStr::new(WINDOWS_LOCAL_JOURNAL_NAME),
            )?
            .is_some()
            {
                return Err(io(
                    "local journal is still in use; retry cleanup after its readers close",
                ));
            }
            report.journals_removed += 1;
            #[cfg(test)]
            windows_move_boundary(WindowsMoveBoundary::RetirementLocalRemoved, Path::new(""));
        }
        if let Some(private) = &self.private {
            private.sync_all().map_err(io)?;
            if self.eligible
                && windows_fs::directory_is_empty(private).map_err(io)?
                && windows_stream_set_is_copy_safe(windows_file_streams(private)?, true)
                && windows_extended_attribute_size(private)? == 0
            {
                remove(private)?;
                self.private.take();
                self.policy.take();
                report.directories_removed += 1;
                self.parent.as_ref().unwrap().sync_all().map_err(io)?;
                if windows_undo::observe(
                    self.parent.as_ref().unwrap(),
                    self.private_name.as_ref().unwrap(),
                )?
                .is_some()
                {
                    return Err(io("retired private directory is still in use"));
                }
                #[cfg(test)]
                windows_move_boundary(
                    WindowsMoveBoundary::RetirementDirectoryRemoved,
                    Path::new(""),
                );
            } else {
                report.directories_kept += 1;
            }
        }
        Ok(())
    }
    fn retire_central(
        &mut self,
        root: &File,
        report: &mut WindowsMoveCleanupReport,
    ) -> Result<(), PlatformError> {
        if let Some(central) = self.central.as_ref() {
            remove(&central.file)?;
            self.central.take();
            if windows_undo::observe(root, &self.central_name)?.is_some() {
                return Err(io(
                    "central journal is still in use; retry after its readers close",
                ));
            }
            root.sync_all().map_err(io)?;
            report.journals_removed += 1;
            #[cfg(test)]
            windows_move_boundary(WindowsMoveBoundary::RetirementCentralRemoved, Path::new(""));
        }
        Ok(())
    }
}
fn remove(file: &File) -> Result<(), PlatformError> {
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE
            | FILE_DISPOSITION_FLAG_FORCE_IMAGE_SECTION_CHECK,
    };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            FileDispositionInfoEx,
            ptr::from_ref(&disposition).cast_mut().cast(),
            mem::size_of_val(&disposition) as u32,
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
