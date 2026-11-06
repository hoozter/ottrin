//! One externally anchored, durable metadata-retirement cohort per registry.
//! Original paired controls admit the snapshot; missing IDs never create it.
use super::*;
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_DISPOSITION_FLAG_ON_CLOSE, FILE_DISPOSITION_INFO_EX, FILE_GENERIC_WRITE,
    FileDispositionInfoEx, SetFileInformationByHandle,
};

const NAME: &str = "active.cohort-journal";
const MAGIC: &[u8; 8] = b"OTCCW001";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    protocol: String,
    registry: Binding,
    coordinator: Binding,
    file: WindowsStableIdentity,
    security: security::PrivateSecurity,
    selection: Selection,
    plans: Vec<cleanup_finalize::Plan>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum Record {
    Header(Box<Snapshot>),
    Pending(usize),
    Done { index: usize, root: EntryVersion },
}

/// Retained only to license retry if the final owned log unlink succeeds but
/// its subsequent directory flush fails. No handles or caller-supplied IDs.
#[derive(Clone, Debug)]
pub(super) struct Receipt {
    snapshot: Snapshot,
    selection: Selection,
    completed: Vec<bool>,
    pending: Option<usize>,
}

impl Receipt {
    fn apply(&mut self, record: Record) -> Result<(), PlatformError> {
        match record {
            Record::Pending(index)
                if index < self.completed.len()
                    && !self.completed[index]
                    && self.pending.is_none() =>
            {
                self.pending = Some(index);
            }
            Record::Done { index, root } if self.pending == Some(index) => {
                let header = self.snapshot.plans[index].header();
                let mut expected = self.selection.root.clone();
                expected.modified = root.modified;
                expected.changed = root.changed;
                if root != expected {
                    return Err(io(
                        "Cohort completion changed unrelated registry attributes",
                    ));
                }
                self.selection.names.retain(|name| {
                    *name != header.central_name()
                        && !(header.parent.same_object(header.central)
                            && *name == header.stage_name())
                });
                self.selection.records.remove(&header.central_name());
                self.selection.root = root;
                self.completed[index] = true;
                self.pending = None;
            }
            _ => return Err(io("Invalid cohort checkpoint transition")),
        }
        Ok(())
    }

    fn all_done(&self) -> bool {
        self.pending.is_none() && self.completed.iter().all(|done| *done)
    }

    fn observation(
        &self,
        manager: &AnchoredRoot,
        coordinator: &AnchoredRoot,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Observation, PlatformError> {
        if manager.binding()? != self.snapshot.registry
            || coordinator.binding()? != self.snapshot.coordinator
        {
            return Err(io("Cohort registry or external anchor admission changed"));
        }
        let authority = manager.authority();
        let central = manager.root();
        let mut selection = self.selection.clone();
        if let Some(index) = self.pending {
            let plan = &self.snapshot.plans[index];
            let parent = inspection_parent(authority, central, plan.header(), cancel)?;
            selection = selection.reconcile_metadata(authority, plan, &parent, central, cancel)?;
        }
        selection.validate(authority, central, cancel)?;
        let mut members = Vec::new();
        for (index, plan) in self.snapshot.plans.iter().enumerate() {
            cancel(0)?;
            let parent = inspection_parent(authority, central, plan.header(), cancel)?;
            plan.validate_in_group(authority, &parent, central, &selection, &mut *cancel)?;
            if self.completed[index] {
                cleanup_tree::absent_with(authority, &parent, &plan.header().stage_name())?;
                cleanup_tree::absent_with(authority, central, &plan.header().central_name())?;
                members.push(Member::Complete(Box::new(plan.header().clone())));
            } else {
                members.push(Member::Metadata(plan.clone()));
            }
        }
        selection.validate(authority, central, cancel)?;
        manager.validate()?;
        coordinator.validate()?;
        Ok(Observation {
            authority: authority.clone(),
            registry: manager.path().to_path_buf(),
            registry_binding: self.snapshot.registry.clone(),
            selection,
            members,
            references: BTreeSet::new(),
            shared: BTreeMap::new(),
            pending_metadata: self.pending,
            pending_unlink: None,
            cohort: Some(self.clone()),
        })
    }
}

fn inspection_parent(
    authority: &Authority,
    central: &File,
    header: &Header,
    cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<File, PlatformError> {
    if header.parent.same_object(header.central) {
        central.try_clone().map_err(io)
    } else {
        authority.resolve_parent(central, header.parent, FILE_GENERIC_READ, cancel)
    }
}

fn frame(sequence: u64, previous: &[u8; 32], record: &Record) -> Result<Vec<u8>, PlatformError> {
    let payload = serde_json::to_vec(record).map_err(io)?;
    if payload.is_empty() || payload.len() > MAX_FRAME {
        return Err(io("Cohort frame budget exceeded"));
    }
    let hash = frame_hash(sequence, previous, &payload);
    let mut bytes = Vec::with_capacity(FRAME_HEADER + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(previous);
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&hash);
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Result<(Receipt, u64, [u8; 32], usize), PlatformError> {
    let (mut at, mut sequence, mut previous, mut receipt) = (0, 0u64, [0; 32], None);
    while at < bytes.len() {
        if receipt.as_ref().is_some_and(Receipt::all_done) {
            return Err(io("Completed cohort has unexpected trailing bytes"));
        }
        let remaining = &bytes[at..];
        let mut prefix = MAGIC.to_vec();
        prefix.extend_from_slice(&sequence.to_le_bytes());
        prefix.extend_from_slice(&previous);
        let count = remaining.len().min(prefix.len());
        if remaining[..count] != prefix[..count] {
            return Err(io("Cohort frame chain changed"));
        }
        if remaining.len() < FRAME_HEADER {
            break;
        }
        let length = u32::from_le_bytes(remaining[48..52].try_into().unwrap()) as usize;
        if length == 0 || length > MAX_FRAME {
            return Err(io("Invalid cohort frame size"));
        }
        let end = FRAME_HEADER + length;
        if end > remaining.len() {
            break;
        }
        let payload = &remaining[FRAME_HEADER..end];
        let hash = frame_hash(sequence, &previous, payload);
        if remaining[52..84] != hash {
            return Err(io("Cohort frame checksum changed"));
        }
        let record: Record = serde_json::from_slice(payload).map_err(io)?;
        if sequence == 0 {
            let Record::Header(snapshot) = record else {
                return Err(io("Cohort lacks initial snapshot"));
            };
            if snapshot.protocol != "ottrin-windows-cleanup-cohort-1"
                || snapshot.plans.is_empty()
                || snapshot.plans.len() > discovery::MAX_REGISTRY_ENTRIES
                || snapshot.plans.iter().any(|plan| !plan.valid_saved())
                || snapshot
                    .plans
                    .iter()
                    .map(|p| p.header().transaction.clone())
                    .collect::<BTreeSet<_>>()
                    != snapshot.selection.selected
                || snapshot.plans.len() != snapshot.selection.selected.len()
                || !snapshot.security.valid_for(&snapshot.selection.creator)
                || !snapshot
                    .selection
                    .security
                    .valid_for(&snapshot.selection.creator)
                || snapshot
                    .selection
                    .names
                    .iter()
                    .any(|name| windows_fs::valid_component(name).is_err())
                || snapshot
                    .selection
                    .names
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    != snapshot.selection.names.len()
                || snapshot.selection.records.keys().any(|name| {
                    !snapshot.selection.names.contains(name)
                        || discovery::registry_id(name).is_none()
                })
                || snapshot.plans.iter().any(|plan| {
                    let header = plan.header();
                    header.central != snapshot.selection.root.identity
                        || header.creator != snapshot.selection.creator
                        || !snapshot.selection.names.contains(&header.central_name())
                        || snapshot
                            .selection
                            .records
                            .get(&header.central_name())
                            .is_none_or(|control| {
                                control.digest != plan.journal_digest()
                                    || control.version.identity != header.central_file
                                    || control.security != header.central_security
                                    || control.creator != header.creator
                            })
                })
            {
                return Err(io("Invalid cohort snapshot"));
            }
            receipt = Some(Receipt {
                selection: snapshot.selection.clone(),
                completed: vec![false; snapshot.plans.len()],
                pending: None,
                snapshot: *snapshot,
            });
        } else {
            receipt.as_mut().unwrap().apply(record)?;
        }
        at += end;
        previous = hash;
        sequence += 1;
    }
    Ok((
        receipt.ok_or_else(|| io("Cohort has no complete admission snapshot"))?,
        sequence,
        previous,
        at,
    ))
}

pub(super) struct Log {
    manager: AnchoredRoot,
    coordinator: AnchoredRoot,
    file: Option<File>,
    receipt: Receipt,
    bytes: Vec<u8>,
    sequence: u64,
    previous: [u8; 32],
    valid_length: usize,
    epoch: EntryVersion,
}

impl Log {
    fn load(
        authority: &Authority,
        path: &Path,
        writable: bool,
    ) -> Result<Option<Self>, PlatformError> {
        let Some(manager) = AnchoredRoot::open_existing(authority, path)? else {
            return Ok(None);
        };
        let Some(coordinator) = manager.cohorts(false)? else {
            return Ok(None);
        };
        let names = windows_fs::directory_children_limited(coordinator.root(), 1).map_err(io)?;
        if names.is_empty() {
            return Ok(None);
        }
        if names != [OsString::from(NAME)] {
            return Err(io("Unknown entries in Copy cohort namespace"));
        }
        let mut file = authority
            .open_child(
                coordinator.root(),
                OsStr::new(NAME),
                false,
                FILE_GENERIC_READ
                    | if writable {
                        FILE_GENERIC_WRITE | DELETE
                    } else {
                        0
                    },
                if writable {
                    FILE_SHARE_READ
                } else {
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
                },
            )
            .map_err(io)?;
        let epoch = super::super::super::version(&file)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_LOG + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() as u64 > MAX_LOG {
            return Err(io("Cohort log budget exceeded"));
        }
        let (receipt, sequence, previous, valid_length) = decode(&bytes)?;
        let snapshot = &receipt.snapshot;
        if snapshot.registry != manager.binding()?
            || snapshot.coordinator != coordinator.binding()?
            || snapshot.file != stable_identity_from_handle(&file).map_err(io)?
            || !snapshot.file.is_regular_file_without_reparse()
        {
            return Err(io("Cohort native identity binding changed"));
        }
        if cleanup_finalize::metadata(
            &file,
            snapshot.file,
            &snapshot.security,
            &snapshot.selection.creator,
            Some(&bytes),
            &mut |_| Ok(()),
        )? != epoch
        {
            return Err(io("Cohort changed while reading"));
        }
        let log = Self {
            manager,
            coordinator,
            file: Some(file),
            receipt,
            bytes,
            sequence,
            previous,
            valid_length,
            epoch,
        };
        log.check()?;
        Ok(Some(log))
    }

    pub(super) fn open(
        group: &Observation,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        if let Some(mut log) = Self::load(&group.authority, &group.registry, true)? {
            if log.receipt.snapshot.selection.selected != group.selection.selected
                || log.receipt.snapshot.registry != group.registry_binding
            {
                return Err(io("Another original Copy cohort must be finished first"));
            }
            // A partial tail grants nothing. Validate the last complete prefix
            // against every live native proof before truncating only that tail.
            log.observation(cancel)?;
            if log.valid_length != log.bytes.len() {
                log.check()?;
                let file = log.file.as_ref().unwrap();
                file.set_len(log.valid_length as u64).map_err(io)?;
                file.sync_all().map_err(io)?;
                log.bytes.truncate(log.valid_length);
                log.epoch = super::super::super::version(file)?;
            }
            return Ok(log);
        }
        if group.cohort.is_some() {
            return Err(io("Admitted Copy coordinator is missing"));
        }
        let manager = AnchoredRoot::open_bound_with(
            &group.authority,
            &group.registry,
            &group.registry_binding,
        )?;
        group
            .selection
            .validate(&group.authority, manager.root(), cancel)?;
        let mut plans = Vec::new();
        for member in &group.members {
            let Member::Metadata(plan) = member else {
                return Err(io("Every cohort member must have paired metadata intent"));
            };
            let parent =
                inspection_parent(&group.authority, manager.root(), plan.header(), cancel)?;
            plan.validate_in_group(
                &group.authority,
                &parent,
                manager.root(),
                &group.selection,
                &mut *cancel,
            )?;
            plans.push(plan.clone());
        }
        let coordinator = manager.cohorts(true)?.unwrap();
        if !windows_fs::directory_children_limited(coordinator.root(), 0)
            .map_err(io)?
            .is_empty()
        {
            return Err(io("Copy cohort namespace is busy"));
        }
        let temporary = OsString::from(format!(
            ".preparing-{}",
            super::super::super::super::random_transaction_id()?
        ));
        let mut file = group
            .authority
            .create_temporary(coordinator.root(), &temporary)
            .map_err(io)?;
        let snapshot = Snapshot {
            protocol: "ottrin-windows-cleanup-cohort-1".into(),
            registry: manager.binding()?,
            coordinator: coordinator.binding()?,
            file: stable_identity_from_handle(&file).map_err(io)?,
            security: security::PrivateSecurity::capture(&file, &group.selection.creator)?,
            selection: group.selection.clone(),
            plans,
        };
        let bytes = frame(0, &[0; 32], &Record::Header(Box::new(snapshot)))?;
        file.write_all(&bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
        #[cfg(test)]
        exit_at("header-unpublished");
        group.authority.file_operation(|| {
            crate::move_transaction::rename_windows_handle_no_replace(
                &file,
                coordinator.root(),
                OsStr::new(NAME),
            )
        })?;
        let disposition = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_ON_CLOSE,
        };
        group.authority.file_operation(|| {
            if unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle().cast(),
                    FileDispositionInfoEx,
                    ptr::from_ref(&disposition).cast(),
                    std::mem::size_of_val(&disposition) as u32,
                )
            } == 0
            {
                return Err(io(std::io::Error::last_os_error()));
            }
            Ok(())
        })?;
        file.sync_all().map_err(io)?;
        coordinator.writable_root()?.sync_all().map_err(io)?;
        manager.validate()?;
        group
            .selection
            .validate(&group.authority, manager.root(), cancel)?;
        drop(file);
        #[cfg(test)]
        exit_at("header-published");
        Self::load(&group.authority, &group.registry, true)?
            .ok_or_else(|| io("Published coordinator disappeared"))
    }

    fn check(&self) -> Result<(), PlatformError> {
        let snapshot = &self.receipt.snapshot;
        if self.manager.binding()? != snapshot.registry
            || self.coordinator.binding()? != snapshot.coordinator
        {
            return Err(io("Cohort root admission changed"));
        }
        let file = self
            .file
            .as_ref()
            .ok_or_else(|| io("Cohort writer already retired"))?;
        self.manager.authority().binding(
            self.coordinator.root(),
            OsStr::new(NAME),
            snapshot.file,
        )?;
        namespace::validate_single_link(file)?;
        snapshot
            .security
            .validate(file, &snapshot.selection.creator)?;
        if cleanup_finalize::metadata(
            file,
            snapshot.file,
            &snapshot.security,
            &snapshot.selection.creator,
            Some(&self.bytes),
            &mut |_| Ok(()),
        )? != self.epoch
            || windows_fs::directory_children_limited(self.coordinator.root(), 1).map_err(io)?
                != [OsString::from(NAME)]
        {
            return Err(io("Cohort writer or namespace epoch changed"));
        }
        Ok(())
    }

    pub(super) fn observation(
        &self,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Observation, PlatformError> {
        self.check()?;
        let group = self
            .receipt
            .observation(&self.manager, &self.coordinator, cancel)?;
        self.check()?;
        Ok(group)
    }

    fn append(&mut self, record: Record) -> Result<(), PlatformError> {
        self.check()?;
        let mut next = self.receipt.clone();
        next.apply(record.clone())?;
        let bytes = frame(self.sequence, &self.previous, &record)?;
        if self.valid_length != self.bytes.len()
            || self.bytes.len() + bytes.len() > MAX_LOG as usize
        {
            return Err(io(
                "Cohort append exceeds its validated prefix or log budget",
            ));
        }
        let file = self.file.as_mut().unwrap();
        file.seek(SeekFrom::End(0)).map_err(io)?;
        #[cfg(test)]
        if TORN.with(|f| f.get()) == Some(matches!(record, Record::Done { .. })) {
            TORN.with(|f| f.set(None));
            file.write_all(&bytes[..bytes.len() / 2]).map_err(io)?;
            file.sync_all().map_err(io)?;
            return Err(io("Injected partial cohort checkpoint write"));
        }
        file.write_all(&bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
        self.epoch = super::super::super::version(file)?;
        self.previous = frame_hash(self.sequence, &self.previous, &bytes[FRAME_HEADER..]);
        self.sequence += 1;
        self.bytes.extend_from_slice(&bytes);
        self.valid_length = self.bytes.len();
        self.receipt = next;
        Ok(())
    }

    pub(super) fn pending(
        &mut self,
        index: usize,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        cancel(0)?;
        if self.receipt.pending != Some(index) {
            self.append(Record::Pending(index))?;
        }
        self.check()?;
        #[cfg(test)]
        exit_at("pending");
        Ok(())
    }

    pub(super) fn done(
        &mut self,
        index: usize,
        root: &EntryVersion,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        cancel(0)?;
        self.append(Record::Done {
            index,
            root: root.clone(),
        })?;
        #[cfg(test)]
        if self.receipt.all_done() {
            exit_at("done-all");
        }
        #[cfg(test)]
        exit_at("done");
        Ok(())
    }

    pub(super) fn receipt(&self) -> Receipt {
        self.receipt.clone()
    }

    pub(super) fn retire(
        &mut self,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if !self.receipt.all_done() {
            return Err(io("Cannot retire an incomplete Copy cohort"));
        }
        self.observation(cancel)?;
        self.check()?;
        let file = self.file.take().unwrap();
        self.manager
            .authority()
            .file_operation(|| cleanup_tree::dispose(&file))?;
        drop(file);
        #[cfg(test)]
        exit_at("retired");
        finish_retirement_receipt(&self.receipt, &self.manager, &self.coordinator, cancel)
    }
}

fn finish_retirement_receipt(
    receipt: &Receipt,
    manager: &AnchoredRoot,
    coordinator: &AnchoredRoot,
    cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    if !receipt.all_done() {
        return Err(io("Incomplete cohort cannot prove retirement"));
    }
    receipt.observation(manager, coordinator, cancel)?;
    if !windows_fs::directory_children_limited(coordinator.root(), 0)
        .map_err(io)?
        .is_empty()
    {
        return Err(io("Retired cohort namespace has unexpected entries"));
    }
    coordinator.writable_root()?.sync_all().map_err(io)?;
    manager.writable_root()?.sync_all().map_err(io)?;
    manager.validate()?;
    coordinator.validate()
}

pub(super) fn retry_retirement(
    group: &Observation,
    cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<bool, PlatformError> {
    let Some(receipt) = &group.cohort else {
        return Ok(false);
    };
    if !receipt.all_done() {
        return Ok(false);
    }
    let manager =
        AnchoredRoot::open_bound_with(&group.authority, &group.registry, &group.registry_binding)?;
    let coordinator = manager
        .cohorts(false)?
        .ok_or_else(|| io("Admitted cohort root is missing"))?;
    if !windows_fs::directory_children_limited(coordinator.root(), 1)
        .map_err(io)?
        .is_empty()
    {
        return Ok(false);
    }
    finish_retirement_receipt(receipt, &manager, &coordinator, cancel)?;
    Ok(true)
}

pub(super) fn observe_at(
    authority: &Authority,
    path: &Path,
    ids: &[String],
    cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Option<Observation>, PlatformError> {
    let Some(log) = Log::load(authority, path, false)? else {
        return Ok(None);
    };
    let selected: BTreeSet<_> = ids.iter().cloned().collect();
    if selected.len() != ids.len() || selected != log.receipt.snapshot.selection.selected {
        return Err(io(
            "Resume the complete original Copy cohort, not a missing or surviving subset",
        ));
    }
    log.observation(cancel).map(Some)
}

pub(super) fn pending_at(
    authority: &Authority,
    path: &Path,
    cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Option<Observation>, PlatformError> {
    Log::load(authority, path, false)?
        .map(|log| log.observation(cancel))
        .transpose()
}

#[cfg(test)]
thread_local! { pub(in super::super) static TORN: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
fn exit_at(point: &str) {
    if std::env::var("OTTRIN_TEST_COHORT_EXIT_AT").as_deref() == Ok(point) {
        std::process::exit(86);
    }
}
