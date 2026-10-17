//! Bound validation and acknowledgement of a recorded peer unlink. No token or absent
//! pathname independently grants permission to remove or acknowledge data.
use super::cleanup_protocol::PeerUnlink;
use super::cleanup_tree::Remaining;
use super::*;

/// A successful owner writer's pending entry; it contains no deletion handle.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Intent {
    pub(super) transaction: String,
    pub(super) control: WindowsStableIdentity,
    pub(super) parent: WindowsStableIdentity,
    pub(super) name: Vec<u16>,
    pub(super) index: usize,
    pub(super) sequence: u64,
    pub(super) digest: [u8; 32],
    pub(super) expected: EntryVersion,
    pub(super) links: u32,
}

impl Intent {
    pub(super) fn capture(journal: &Journal) -> Result<Self, PlatformError> {
        let progress = journal
            .state
            .cleanup
            .as_ref()
            .ok_or_else(|| io("Missing owner cleanup"))?;
        let index = *progress
            .manifest
            .order
            .get(progress.completed)
            .ok_or_else(|| io("No remaining owner entry"))?;
        let node = &progress.manifest.entries[index];
        if !progress.pending
            || progress.peer_unlink.is_some()
            || progress.manifest.owner != cleanup_protocol::Owner::Original
            || node.version.directory
            || progress.expected_links(index)? < 2
        {
            return Err(io("Owner has no pending shared-file unlink"));
        }
        let parent = node
            .parent
            .map(|p| progress.expected(p).identity)
            .unwrap_or(journal.header.parent);
        Ok(Self {
            transaction: journal.header.transaction.clone(),
            control: journal.header.central_file,
            parent,
            name: node.name.clone(),
            index,
            sequence: journal.sequence,
            digest: Sha256::digest(read(&journal.local)?).into(),
            expected: progress.expected(index).clone(),
            links: progress.expected_links(index)?,
        })
    }

    pub(super) fn bind_local(&self, local: &State) -> Result<PeerUnlink, PlatformError> {
        let progress = local
            .cleanup
            .as_ref()
            .ok_or_else(|| io("Missing local cleanup"))?;
        let index = progress.manifest.order[progress.completed..]
            .iter()
            .copied()
            .find(|&index| progress.expected(index).identity == self.expected.identity)
            .ok_or_else(|| io("Peer does not retain the shared object"))?;
        if progress.expected(index) != &self.expected
            || progress.expected_links(index)? != self.links
        {
            return Err(io("Peer retained version/count differs from owner intent"));
        }
        Ok(self.proof(index))
    }

    fn proof(&self, index: usize) -> PeerUnlink {
        PeerUnlink {
            index,
            transaction: self.transaction.clone(),
            control: self.control,
            parent: self.parent,
            name: self.name.clone(),
            peer_index: self.index,
            sequence: self.sequence,
            intent_digest: self.digest,
        }
    }

    pub(super) fn acknowledged_by(&self, local: &State) -> Result<bool, PlatformError> {
        let progress = local
            .cleanup
            .as_ref()
            .ok_or_else(|| io("Missing peer cleanup"))?;
        let Some(index) = progress.manifest.order[progress.completed..]
            .iter()
            .copied()
            .find(|&i| progress.expected(i).identity == self.expected.identity)
        else {
            return Ok(false);
        };
        if !progress.has_peer_acknowledgement(&self.proof(index).key()?) {
            return Ok(false);
        }
        let actual = progress.expected(index);
        let mut expected = self.expected.clone();
        expected.changed = actual.changed;
        if progress.peer_unlink.is_some()
            || progress.pending
            || *actual != expected
            || progress.expected_links(index)?
                != self
                    .links
                    .checked_sub(1)
                    .ok_or_else(|| io("Invalid peer alias count"))?
        {
            return Err(io("Acknowledged peer alias no longer matches owner unlink"));
        }
        Ok(true)
    }
}

pub(super) struct Observation {
    control: File,
    snapshot: Snapshot,
    remaining: Remaining,
    parent: File,
    control_version: EntryVersion,
    parent_security: [u8; 32],
    removed: bool,
    local_control: WindowsStableIdentity,
    proof: PeerUnlink,
}

impl Observation {
    pub(super) fn open(
        central: &File,
        local: &State,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        cancel(0)?;
        let progress = local
            .cleanup
            .as_ref()
            .ok_or_else(|| io("Copy has no peer cleanup state"))?;
        let proof = progress
            .peer_unlink
            .as_deref()
            .ok_or_else(|| io("Copy has no recorded peer unlink intent"))?;
        let record = discovery::RegisteredJournal::dependency(
            central,
            &OsString::from(format!("{}.copy-journal", proof.transaction)),
        )?;
        let control_version = super::super::version(&record.file)?;
        let header = record.state.header.as_ref().unwrap();
        let local_header = local.header.as_ref().unwrap();
        if header.transaction != proof.transaction
            || header.central_file != proof.control
            || header.central != local_header.central
            || header.creator != local_header.creator
        {
            return Err(io("Copy peer control does not match recorded unlink"));
        }
        let (prefix, end) = decode_until(&record.bytes, Some(proof.sequence))?;
        if prefix.state.header.as_ref() != Some(header)
            || <[u8; 32]>::from(Sha256::digest(&record.bytes[..end])) != proof.intent_digest
        {
            return Err(io("Copy peer intent prefix changed"));
        }
        let before = prefix
            .state
            .cleanup
            .as_ref()
            .ok_or_else(|| io("Copy peer prefix has no cleanup intent"))?;
        Self::pending_entry(proof, progress, before, header)?;
        let current = record
            .state
            .cleanup
            .as_ref()
            .ok_or_else(|| io("Copy peer cleanup state is missing"))?;
        if current.manifest.digest != before.manifest.digest
            || current.completed < before.completed
            || current.peer_unlink.is_some()
            || current.metadata_removal_intent
        {
            return Err(io("Copy peer cleanup cannot certify this unlink"));
        }
        if current.completed == before.completed && !current.pending {
            return Err(io("Copy peer has lost the recorded pending unlink"));
        }
        let parent = discovery::Volumes::new(central)?.resolve(
            header.parent,
            FILE_GENERIC_READ,
            &mut cancel,
        )?;
        let snapshot =
            Snapshot::read_with_control(&parent, central, &header.stage_name(), &record.file)?;
        if snapshot.bytes != record.bytes || snapshot.state.header.as_ref() != Some(header) {
            return Err(io("Copy peer unlink mirrors disagree"));
        }
        let remaining = Remaining::open(&parent, &snapshot.state, current, &mut cancel)?;
        let removed = current.completed > before.completed || remaining.pending_absent;
        let parent_security = security::owner_group_dacl_digest(&parent)?;
        let result = Self {
            control: record.file,
            snapshot,
            remaining,
            parent,
            control_version,
            parent_security,
            removed,
            local_control: local_header.central_file,
            proof: proof.clone(),
        };
        result.revalidate(central, &mut cancel)?;
        Ok(result)
    }

    fn pending_entry(
        proof: &PeerUnlink,
        local: &cleanup_protocol::Progress,
        peer: &cleanup_protocol::Progress,
        header: &Header,
    ) -> Result<(), PlatformError> {
        if !peer.pending
            || peer.payload_removed
            || peer.peer_unlink.is_some()
            || peer.manifest.order.get(peer.completed) != Some(&proof.peer_index)
        {
            return Err(io(
                "Copy peer prefix does not end at the recorded entry intent",
            ));
        }
        let entry = peer
            .manifest
            .entries
            .get(proof.peer_index)
            .ok_or_else(|| io("Copy peer entry is missing"))?;
        let parent = entry
            .parent
            .map(|index| peer.expected(index).identity)
            .unwrap_or(header.parent);
        if entry.name != proof.name
            || parent != proof.parent
            || peer.expected(proof.peer_index) != local.expected(proof.index)
            || peer.expected_links(proof.peer_index)? != local.expected_links(proof.index)?
        {
            return Err(io("Copy peer entry does not match retained alias evidence"));
        }
        Ok(())
    }

    pub(super) fn removed(&self) -> bool {
        self.removed
    }

    pub(super) fn removed_identity(
        &self,
        local: &State,
    ) -> Result<WindowsStableIdentity, PlatformError> {
        let progress = local
            .cleanup
            .as_ref()
            .ok_or_else(|| io("Missing peer cleanup state"))?;
        if !self.removed
            || local.header.as_ref().map(|header| header.central_file) != Some(self.local_control)
            || progress.peer_unlink.as_deref() != Some(&self.proof)
        {
            return Err(io(
                "Peer observation does not certify this local pending unlink",
            ));
        }
        Ok(progress.expected(self.proof.index).identity)
    }

    pub(super) fn revalidate(
        &self,
        central: &File,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        cancel(0)?;
        let header = self.snapshot.state.header.as_ref().unwrap();
        bound(central, &header.central_name(), header.central_file)?;
        namespace::validate_single_link(&self.control)?;
        header
            .central_security
            .validate(&self.control, &header.creator)?;
        if super::super::version(&self.control)? != self.control_version
            || read(&self.control)? != self.snapshot.bytes
            || super::super::version(&self.control)? != self.control_version
            || security::owner_group_dacl_digest(&self.parent)? != self.parent_security
        {
            return Err(io("Copy peer control or parent changed during observation"));
        }
        self.snapshot.revalidate(&self.parent, central)?;
        self.remaining.revalidate(
            &self.parent,
            &self.snapshot.state,
            self.snapshot.state.cleanup.as_ref().unwrap(),
            &mut cancel,
        )
    }
}

impl Journal {
    /// Record one peer unlink only after its canonical intent/absence and every
    /// local retained alias have been checked under held read capabilities.
    /// This removes no payload and grants no authority for another unlink.
    pub(super) fn acknowledge_peer_unlink(
        &mut self,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        cancel(0)?;
        if self.failed {
            return Err(io("Copy journal write is uncertain; data retained"));
        }
        self.bindings()?;
        self.reserve_cleanup_tail()?;
        let parent_security = security::owner_group_dacl_digest(&self.parent_dir)?;
        let observation = Observation::open(&self.central_dir, &self.state, &mut cancel)?;
        let remaining =
            Remaining::after_peer_unlink(&self.parent_dir, &self.state, &observation, &mut cancel)?;
        let progress = self.state.cleanup.as_ref().unwrap();
        let proof = progress.peer_unlink.as_deref().unwrap();
        let after = remaining.observed[proof.index]
            .as_ref()
            .ok_or_else(|| io("Copy peer acknowledgement has no surviving local alias"))?
            .clone();
        let key = proof.key()?;
        observation.revalidate(&self.central_dir, &mut cancel)?;
        remaining.revalidate(&self.parent_dir, &self.state, progress, &mut cancel)?;
        if security::owner_group_dacl_digest(&self.parent_dir)? != parent_security {
            return Err(io("Copy local parent changed during peer acknowledgement"));
        }
        self.bindings()?;
        self.persist(Record::Cleanup {
            entry: cleanup_protocol::CleanupRecord::PeerUnlinkRemoved {
                key,
                after: Box::new(after),
            },
        })
    }
}
