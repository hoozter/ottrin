//! Durable deletion progress derived from the existing Copy evidence.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Owner {
    None,
    Copy,
    Original,
}

/// Binding to one peer's durable, pending unlink. A runtime writer must verify
/// the native peer record and slot; this token alone never permits a deletion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PeerUnlink {
    pub(super) index: usize,
    pub(super) transaction: String,
    pub(super) control: WindowsStableIdentity,
    pub(super) parent: WindowsStableIdentity,
    pub(super) name: Vec<u16>,
    pub(super) peer_index: usize,
    pub(super) sequence: u64,
    pub(super) intent_digest: [u8; 32],
}

impl PeerUnlink {
    // The same peer entry cannot be counted again through another local alias,
    // a rewritten sequence or a different claimed digest.
    pub(super) fn key(&self) -> Result<[u8; 32], PlatformError> {
        super::super::evidence_digest(&(&self.transaction, self.control, self.peer_index))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum CleanupRecord {
    Begin {
        owner: Owner,
        count: usize,
        digest: [u8; 32],
    },
    EntryIntent {
        index: usize,
    },
    EntryRemoved {
        index: usize,
        parent_after: Option<Box<EntryVersion>>,
        alias_after: Option<Box<EntryVersion>>,
    },
    PeerUnlinkIntent {
        proof: Box<PeerUnlink>,
    },
    PeerUnlinkRemoved {
        key: [u8; 32],
        after: Box<EntryVersion>,
    },
    PayloadRemoved,
    MetadataRemovalIntent,
}

#[derive(Debug)]
pub(super) struct Entry {
    pub(super) parent: Option<usize>,
    pub(super) name: Vec<u16>,
    pub(super) version: EntryVersion,
    pub(super) links: u32,
}

#[derive(Debug)]
pub(super) struct Manifest {
    pub(super) owner: Owner,
    pub(super) entries: Vec<Entry>,
    pub(super) order: Vec<usize>,
    pub(super) digest: [u8; 32],
    groups: BTreeMap<(u64, [u8; 16]), Vec<usize>>,
}

impl Manifest {
    fn has_later_alias(&self, index: usize, offset: usize) -> bool {
        self.entries.len() - 1 - self.same_object(index)[0] > offset
    }
    pub(super) fn same_object(&self, index: usize) -> &[usize] {
        &self.groups[&self.entries[index].version.identity.object_key()]
    }
}

#[derive(Debug)]
pub(super) struct Progress {
    pub(super) manifest: Manifest,
    pub(super) completed: usize,
    pub(super) pending: bool,
    pub(super) payload_removed: bool,
    pub(super) metadata_removal_intent: bool,
    pub(super) peer_unlink: Option<Box<PeerUnlink>>,
    peer_removed: BTreeSet<[u8; 32]>,
    versions: BTreeMap<(u64, [u8; 16]), EntryVersion>,
    removed_links: BTreeMap<(u64, [u8; 16]), u32>,
}

impl Progress {
    pub(super) fn new(manifest: Manifest) -> Self {
        Self {
            manifest,
            completed: 0,
            pending: false,
            payload_removed: false,
            metadata_removal_intent: false,
            peer_unlink: None,
            peer_removed: BTreeSet::new(),
            versions: BTreeMap::new(),
            removed_links: BTreeMap::new(),
        }
    }
    pub(super) fn expected(&self, index: usize) -> &EntryVersion {
        let entry = &self.manifest.entries[index];
        self.versions
            .get(&entry.version.identity.object_key())
            .unwrap_or(&entry.version)
    }

    /// Original hard links may also exist outside this tree. Only names removed
    /// by this transaction or acknowledged peer unlinks reduce the expected count;
    /// no outside name is
    /// discovered, adopted or granted deletion authority.
    pub(super) fn expected_links(&self, index: usize) -> Result<u32, PlatformError> {
        let entry = &self.manifest.entries[index];
        let removed = self
            .removed_links
            .get(&entry.version.identity.object_key())
            .copied()
            .unwrap_or(0);
        entry
            .links
            .checked_sub(removed)
            .ok_or_else(|| io("Copy cleanup link count underflow"))
    }

    fn remaining_alias(&self, index: usize) -> bool {
        self.manifest.has_later_alias(index, self.completed)
    }
}

impl State {
    pub(super) fn cleanup_manifest(&self) -> Result<Manifest, PlatformError> {
        if !self.completion_verified
            || self.undo_intent && !self.undo_complete()
            || self.readonly_pending.is_some()
            || self.rename_creation.is_some()
        {
            return Err(io("Copy cleanup requires a terminal verified transaction"));
        }
        let header = self.header.as_ref().unwrap();
        let (owner, entries, digest) = if self.undo_complete() {
            let staged = self
                .nodes
                .iter()
                .map(|node| (node.parent, &node.name, node.staged.as_ref().unwrap()))
                .collect::<Vec<_>>();
            let entries = self
                .nodes
                .iter()
                .enumerate()
                .map(|(index, node)| Entry {
                    parent: node.parent,
                    name: if index == 0 {
                        header.candidate_name().encode_wide().collect()
                    } else {
                        node.name.clone()
                    },
                    version: if index == 0 {
                        self.undone.as_ref().unwrap().clone()
                    } else {
                        self.final_versions[index].clone()
                    },
                    links: 1,
                })
                .collect();
            (
                Owner::Copy,
                entries,
                super::super::evidence_digest(&(
                    header,
                    Owner::Copy,
                    &staged,
                    &self.planned,
                    &self.final_versions,
                    &self.undone,
                ))?,
            )
        } else if let Some(original) = &self.original {
            if !original.sealed || self.original_retained.is_none() || self.original_restore_intent
            {
                return Err(io("Copy cleanup original is not durably retained"));
            }
            let entries = original
                .evidence
                .entries
                .iter()
                .enumerate()
                .map(|(index, entry)| Entry {
                    parent: entry.parent,
                    name: if index == 0 {
                        header.original_name().encode_wide().collect()
                    } else {
                        entry.name.clone()
                    },
                    version: if index == 0 {
                        self.original_retained.as_ref().unwrap().clone()
                    } else {
                        entry.version.clone()
                    },
                    links: entry.links,
                })
                .collect();
            (
                Owner::Original,
                entries,
                super::super::evidence_digest(&(
                    header,
                    Owner::Original,
                    &original.evidence,
                    &self.original_retained,
                ))?,
            )
        } else {
            (
                Owner::None,
                Vec::new(),
                super::super::evidence_digest(&(header, Owner::None, &self.final_versions))?,
            )
        };
        let entries: Vec<Entry> = entries;
        let mut identities = BTreeMap::new();
        for (index, entry) in entries.iter().enumerate() {
            if !valid_leaf(&entry.name)
                || index == 0 && entry.parent.is_some()
                || index > 0
                    && !entry.parent.is_some_and(|parent| {
                        parent < index
                            && entries[parent].version.directory
                            && entries[parent].version.link.is_none()
                    })
                || entry.links == 0
            {
                return Err(io("Invalid Copy cleanup topology"));
            }
            let observed = identities
                .entry(entry.version.identity.object_key())
                .or_insert((entry.links, 0u32));
            if observed.0 != entry.links || entry.version.directory && observed.1 != 0 {
                return Err(io("Inconsistent Copy cleanup aliases"));
            }
            observed.1 = observed
                .1
                .checked_add(1)
                .ok_or_else(|| io("Copy cleanup alias overflow"))?;
            if observed.1 > observed.0 {
                return Err(io("Copy cleanup graph exceeds native link count"));
            }
        }
        let order: Vec<_> = (0..entries.len()).rev().collect();
        let mut groups: BTreeMap<_, Vec<usize>> = BTreeMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let group = groups
                .entry(entry.version.identity.object_key())
                .or_default();
            if let Some(&first) = group.first() {
                if entries[first].version != entry.version {
                    return Err(io("Copy cleanup aliases have inconsistent versions"));
                }
                if owner == Owner::Original {
                    let original = &self.original.as_ref().unwrap().evidence.entries;
                    if original[first].streams != original[index].streams
                        || original[first].compression != original[index].compression
                        || original[first].owner_group_dacl != original[index].owner_group_dacl
                    {
                        return Err(io(
                            "Copy cleanup aliases have inconsistent payload evidence",
                        ));
                    }
                }
            }
            group.push(index);
        }
        Ok(Manifest {
            owner,
            order,
            groups,
            entries,
            digest,
        })
    }

    pub(super) fn check_cleanup_record(&self, record: &Record) -> Result<bool, PlatformError> {
        let invalid = || io("Invalid Copy cleanup transition; all remaining data retained");
        let Record::Cleanup { entry } = record else {
            return if self.cleanup.is_some() {
                Err(invalid())
            } else {
                Ok(false)
            };
        };
        if let CleanupRecord::Begin {
            owner,
            count,
            digest,
        } = entry
        {
            if self.cleanup.is_some() {
                return Err(invalid());
            }
            let manifest = self.cleanup_manifest()?;
            if *owner != manifest.owner
                || *count != manifest.entries.len()
                || *digest != manifest.digest
            {
                return Err(invalid());
            }
            return Ok(true);
        }
        let progress = self.cleanup.as_ref().ok_or_else(invalid)?;
        if matches!(entry, CleanupRecord::MetadataRemovalIntent) {
            return if progress.payload_removed
                && !progress.metadata_removal_intent
                && progress.peer_unlink.is_none()
            {
                Ok(true)
            } else {
                Err(invalid())
            };
        }
        if progress.payload_removed {
            return Err(invalid());
        }
        match entry {
            CleanupRecord::PeerUnlinkIntent { proof } => {
                let header = self.header.as_ref().unwrap();
                let Some(node) = progress.manifest.entries.get(proof.index) else {
                    return Err(invalid());
                };
                let local_parent = node
                    .parent
                    .map(|index| progress.expected(index).identity)
                    .unwrap_or(header.parent);
                let peer_name = OsString::from(format!("{}.copy-journal", proof.transaction));
                if progress.manifest.owner != Owner::Original
                    || progress.pending
                    || progress.peer_unlink.is_some()
                    || !progress.manifest.order[progress.completed..].contains(&proof.index)
                    || node.version.directory
                    || progress.expected_links(proof.index)? < 2
                    || discovery::registry_id(&peer_name).as_deref()
                        != Some(proof.transaction.as_str())
                    || proof.transaction == header.transaction
                    || !proof.control.is_regular_file_without_reparse()
                    || proof.control == header.central_file
                    || proof.control == header.local_file
                    || proof.control.volume_serial_number() != header.central.volume_serial_number()
                    || !proof.parent.is_directory_without_reparse()
                    || proof.parent.volume_serial_number()
                        != node.version.identity.volume_serial_number()
                    || proof.parent == local_parent && proof.name == node.name
                    || !valid_leaf(&proof.name)
                    || proof.sequence == 0
                    || proof.intent_digest == [0; 32]
                    || proof.peer_index >= MAX_LOG as usize / FRAME_HEADER
                    || progress.peer_removed.contains(&proof.key()?)
                {
                    return Err(invalid());
                }
                return Ok(true);
            }
            CleanupRecord::PeerUnlinkRemoved { key, after } => {
                let proof = progress.peer_unlink.as_ref().ok_or_else(invalid)?;
                let mut expected = progress.expected(proof.index).clone();
                expected.changed = after.changed;
                if *key != proof.key()?
                    || **after != expected
                    || progress.expected_links(proof.index)? < 2
                {
                    return Err(invalid());
                }
                return Ok(true);
            }
            _ => {}
        }
        if progress.peer_unlink.is_some() {
            return Err(invalid());
        }
        if matches!(entry, CleanupRecord::PayloadRemoved) {
            return if progress.completed == progress.manifest.entries.len() && !progress.pending {
                Ok(true)
            } else {
                Err(invalid())
            };
        }
        let index = *progress
            .manifest
            .order
            .get(progress.completed)
            .ok_or_else(invalid)?;
        if progress.expected_links(index)? == 0 {
            return Err(invalid());
        }
        match entry {
            CleanupRecord::EntryIntent { index: actual }
                if *actual == index && !progress.pending => {}
            CleanupRecord::EntryRemoved {
                index: actual,
                parent_after,
                alias_after,
            } if *actual == index && progress.pending => {
                let node = &progress.manifest.entries[index];
                match (node.parent, parent_after) {
                    (None, None) => {}
                    (Some(parent), Some(after)) => {
                        let mut expected = progress.expected(parent).clone();
                        expected.modified = after.modified;
                        expected.changed = after.changed;
                        if **after != expected {
                            return Err(invalid());
                        }
                    }
                    _ => return Err(invalid()),
                }
                match (progress.remaining_alias(index), alias_after) {
                    (false, None) => {}
                    (true, Some(after)) if !node.version.directory => {
                        let mut expected = progress.expected(index).clone();
                        expected.changed = after.changed;
                        if **after != expected {
                            return Err(invalid());
                        }
                    }
                    _ => return Err(invalid()),
                }
            }
            _ => return Err(invalid()),
        }
        Ok(true)
    }

    pub(super) fn apply_cleanup_record(
        &mut self,
        record: CleanupRecord,
    ) -> Result<(), PlatformError> {
        if matches!(record, CleanupRecord::Begin { .. }) {
            self.cleanup = Some(Progress::new(self.cleanup_manifest()?));
            return Ok(());
        }
        let progress = self.cleanup.as_mut().unwrap();
        match record {
            CleanupRecord::PeerUnlinkIntent { proof } => progress.peer_unlink = Some(proof),
            CleanupRecord::PeerUnlinkRemoved { key, after } => {
                *progress
                    .removed_links
                    .entry(after.identity.object_key())
                    .or_default() += 1;
                progress
                    .versions
                    .insert(after.identity.object_key(), *after);
                progress.peer_removed.insert(key);
                progress.peer_unlink = None;
            }
            CleanupRecord::EntryIntent { .. } => progress.pending = true,
            CleanupRecord::EntryRemoved {
                index,
                parent_after,
                alias_after,
            } => {
                for version in [parent_after, alias_after].into_iter().flatten() {
                    progress
                        .versions
                        .insert(version.identity.object_key(), *version);
                }
                let identity = progress.manifest.entries[index]
                    .version
                    .identity
                    .object_key();
                *progress.removed_links.entry(identity).or_default() += 1;
                progress.completed += 1;
                progress.pending = false;
            }
            CleanupRecord::PayloadRemoved => progress.payload_removed = true,
            CleanupRecord::MetadataRemovalIntent => progress.metadata_removal_intent = true,
            CleanupRecord::Begin { .. } => unreachable!(),
        }
        Ok(())
    }
}

impl Journal {
    /// Budget every remaining intent/ack before the first payload mutation.
    /// Unknown future native timestamp widths use the longest signed encoding.
    pub(super) fn cleanup_tail_size(&self) -> Result<u64, PlatformError> {
        let manifest;
        let (manifest, completed, pending, done) = if let Some(progress) = &self.state.cleanup {
            (
                &progress.manifest,
                progress.completed,
                progress.pending,
                progress.payload_removed,
            )
        } else {
            manifest = self.state.cleanup_manifest()?;
            (&manifest, 0, false, false)
        };
        if done {
            return if self.state.cleanup.as_ref().unwrap().metadata_removal_intent {
                Ok(0)
            } else {
                Ok(FRAME_HEADER as u64
                    + record_payload(&Record::Cleanup {
                        entry: CleanupRecord::MetadataRemovalIntent,
                    })?
                    .len() as u64)
            };
        }
        let mut bytes = 0u64;
        let mut add = |entry| -> Result<(), PlatformError> {
            bytes = bytes
                .checked_add(
                    FRAME_HEADER as u64 + record_payload(&Record::Cleanup { entry })?.len() as u64,
                )
                .ok_or_else(|| io("Copy cleanup reservation overflow"))?;
            Ok(())
        };
        if let Some(proof) = self
            .state
            .cleanup
            .as_ref()
            .and_then(|progress| progress.peer_unlink.as_ref())
        {
            let progress = self.state.cleanup.as_ref().unwrap();
            let mut after = progress.expected(proof.index).clone();
            after.changed = i64::MIN;
            add(CleanupRecord::PeerUnlinkRemoved {
                key: [255; 32],
                after: Box::new(after),
            })?;
        }
        if self.state.cleanup.is_none() {
            add(CleanupRecord::Begin {
                owner: manifest.owner,
                count: manifest.entries.len(),
                digest: [255; 32],
            })?;
        }
        let bound = |index: usize| {
            let mut version = manifest.entries[index].version.clone();
            version.changed = i64::MIN;
            version.modified = i64::MIN;
            Box::new(version)
        };
        for (offset, &index) in manifest.order.iter().enumerate().skip(completed) {
            if offset != completed || !pending {
                add(CleanupRecord::EntryIntent { index })?;
            }
            let alias = manifest.has_later_alias(index, offset);
            add(CleanupRecord::EntryRemoved {
                index,
                parent_after: manifest.entries[index].parent.map(bound),
                alias_after: alias.then(|| bound(index)),
            })?;
        }
        add(CleanupRecord::PayloadRemoved)?;
        add(CleanupRecord::MetadataRemovalIntent)?;
        Ok(bytes)
    }

    /// Reserve both peer records and all remaining own cleanup acknowledgements
    /// before publishing this intent. Runtime peer validation belongs to the
    /// bound group writer; this method only enforces the durable codec/budget.
    pub(super) fn prepare_peer_unlink(&mut self, proof: PeerUnlink) -> Result<(), PlatformError> {
        if self.failed {
            return Err(io("Copy journal write is uncertain; data retained"));
        }
        self.bindings()?;
        if self
            .state
            .cleanup
            .as_ref()
            .and_then(|progress| progress.peer_unlink.as_deref())
            == Some(&proof)
        {
            return self.reserve_cleanup_tail();
        }
        let record = Record::Cleanup {
            entry: CleanupRecord::PeerUnlinkIntent {
                proof: Box::new(proof.clone()),
            },
        };
        self.state.check(&record)?;
        let progress = self.state.cleanup.as_ref().unwrap();
        let mut after = progress.expected(proof.index).clone();
        after.changed = i64::MIN;
        let ack = Record::Cleanup {
            entry: CleanupRecord::PeerUnlinkRemoved {
                key: [255; 32],
                after: Box::new(after),
            },
        };
        let intent_bytes = record_payload(&record)?.len() as u64;
        let acknowledgement_bytes = record_payload(&ack)?.len() as u64;
        let bytes = self
            .length
            .checked_add(self.cleanup_tail_size()?)
            .and_then(|bytes| bytes.checked_add(2 * FRAME_HEADER as u64))
            .and_then(|bytes| bytes.checked_add(intent_bytes))
            .and_then(|bytes| bytes.checked_add(acknowledgement_bytes))
            .ok_or_else(|| io("Copy peer unlink reservation overflow"))?;
        if bytes > log_budget() {
            return Err(io(
                "Copy journal has insufficient peer unlink space; all data retained",
            ));
        }
        self.persist(record)
    }

    pub(super) fn reserve_cleanup_tail(&self) -> Result<(), PlatformError> {
        if self
            .length
            .checked_add(self.cleanup_tail_size()?)
            .is_none_or(|length| length > log_budget())
        {
            return Err(io(
                "Copy journal has insufficient cleanup space; all data retained",
            ));
        }
        Ok(())
    }
}
