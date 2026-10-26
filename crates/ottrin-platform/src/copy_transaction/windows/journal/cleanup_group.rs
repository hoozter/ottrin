//! Bound admission and start barrier for selected cleanup cohorts. Physical
//! removal is separate; selected IDs alone never grant mutation authority.
use super::super::registry::AnchoredRoot;
use super::*;
use std::collections::BTreeMap;
use std::path::Path;

type Object = (u64, [u8; 16]);

#[derive(Clone, Debug)]
struct Control {
    digest: [u8; 32],
    version: EntryVersion,
    security: security::PrivateSecurity,
    creator: Vec<u8>,
}

/// The initial catalog, including unselected peers. Only this module constructs
/// a selection from held native records; callers cannot supply exclusion IDs.
#[derive(Clone, Debug)]
pub(super) struct Selection {
    selected: BTreeSet<String>,
    records: BTreeMap<OsString, Control>,
    names: Vec<OsString>,
    root: EntryVersion,
    security: security::PrivateSecurity,
    creator: Vec<u8>,
}

impl Selection {
    fn capture(
        central: &File,
        ids: &[String],
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        if ids.is_empty() || ids.len() > discovery::MAX_REGISTRY_ENTRIES {
            return Err(io("Invalid Copy cleanup selection size"));
        }
        let mut selected = BTreeSet::new();
        for id in ids {
            let name = OsString::from(format!("{id}.copy-journal"));
            if discovery::registry_id(&name).as_deref() != Some(id.as_str())
                || !selected.insert(id.clone())
            {
                return Err(io("Invalid or duplicate Copy cleanup selection"));
            }
        }
        let root = super::super::version(central)?;
        let creator = windows_fs::current_owner()?;
        let security = security::PrivateSecurity::capture(central, &creator)?;
        let names =
            windows_fs::directory_children_limited(central, discovery::MAX_REGISTRY_ENTRIES)
                .map_err(io)?;
        let mut records = BTreeMap::new();
        for name in &names {
            cancel(0)?;
            let Some(_) = discovery::registry_id(name) else {
                if discovery::registry_candidate(name) {
                    return Err(io("Copy group catalog has an unrecognized recovery record"));
                }
                continue;
            };
            let record = discovery::RegisteredJournal::dependency(central, name)?;
            let header = record.state.header.as_ref().unwrap();
            records.insert(
                name.clone(),
                Control {
                    digest: Sha256::digest(&record.bytes).into(),
                    version: super::super::version(&record.file)?,
                    security: header.central_security.clone(),
                    creator: header.creator.clone(),
                },
            );
        }
        for id in &selected {
            if !records.contains_key(&OsString::from(format!("{id}.copy-journal"))) {
                return Err(io("Selected Copy cleanup record is missing"));
            }
        }
        let result = Self {
            selected,
            records,
            names,
            root,
            security,
            creator,
        };
        result.validate(central, cancel)?;
        Ok(result)
    }

    pub(super) fn fingerprint(&self, header: &Header) -> Result<[u8; 32], PlatformError> {
        if header.central != self.root.identity {
            return Err(io("Copy group registry binding changed"));
        }
        cleanup_dependencies::catalog_fingerprint(
            header.central,
            &self.root,
            &self.security,
            &self.creator,
            &self.names,
            self.names.iter().filter_map(|name| {
                if *name == header.central_name() {
                    return None;
                }
                self.records
                    .get(name)
                    .map(|record| (name.as_os_str(), record.digest, &record.version))
            }),
        )
    }

    pub(super) fn advance_catalog(
        &self,
        header: &Header,
        catalog: &mut [u8; 32],
        after: &Self,
    ) -> Result<(), PlatformError> {
        if *catalog != self.fingerprint(header)? {
            return Err(io("Copy group catalog handoff does not match admission"));
        }
        *catalog = after.fingerprint(header)?;
        Ok(())
    }

    fn accept_payload(
        &mut self,
        completion: &cleanup_finalize::Completion,
        change: &cleanup_finalize::PayloadChange,
    ) -> Result<(), PlatformError> {
        self.accept_payload_change(completion.header(), completion.journal_digest(), change)
    }

    fn accept_payload_change(
        &mut self,
        header: &Header,
        after_journal: [u8; 32],
        change: &cleanup_finalize::PayloadChange,
    ) -> Result<(), PlatformError> {
        let record = self
            .records
            .get_mut(&header.central_name())
            .ok_or_else(|| io("Completed Copy member is absent from group"))?;
        if !self.selected.contains(&header.transaction)
            || record.digest != change.before_journal
            || record.version.identity != change.control_version.identity
            || change.control_version.identity != header.central_file
        {
            return Err(io(
                "Copy payload acknowledgement does not match frozen control",
            ));
        }
        let mut names = self.names.clone();
        let mut root = self.root.clone();
        if names != change.registry_names {
            if !header.parent.same_object(header.central) {
                return Err(io("Copy payload changed an unrelated registry namespace"));
            }
            let retained = change
                .retained
                .as_ref()
                .ok_or_else(|| io("Copy payload has no retained root acknowledgement"))?;
            let index = names
                .iter()
                .position(|name| name == retained)
                .ok_or_else(|| io("Copy payload root was not in the frozen registry"))?;
            names.remove(index);
            root.modified = change.registry_stamp.modified;
            root.changed = change.registry_stamp.changed;
        }
        if names != change.registry_names || root != change.registry_stamp {
            return Err(io(
                "Copy payload acknowledgement has an unexpected registry change",
            ));
        }
        record.digest = after_journal;
        record.version = change.control_version.clone();
        self.names = names;
        self.root = root;
        Ok(())
    }

    /// Only a successful held writer supplies this epoch. No reopened ID or
    /// freshly observed external catalog change is accepted as our own work.
    fn accept_control(
        &mut self,
        header: &Header,
        before: [u8; 32],
        after: [u8; 32],
        version: &EntryVersion,
    ) -> Result<(), PlatformError> {
        let record = self
            .records
            .get_mut(&header.central_name())
            .ok_or_else(|| io("Changed Copy member is absent from group"))?;
        if !self.selected.contains(&header.transaction)
            || record.digest != before
            || record.version.identity != version.identity
            || version.identity != header.central_file
        {
            return Err(io(
                "Copy control acknowledgement does not match frozen member",
            ));
        }
        record.digest = after;
        record.version = version.clone();
        Ok(())
    }

    fn accept_change(&mut self, started: &cleanup::ControlChange) -> Result<(), PlatformError> {
        self.accept_control(
            started.admission.header(),
            started.before,
            started.after,
            &started.central,
        )
    }

    /// Reconcile only metadata names already authorized by this retained Plan.
    /// Every surviving control and all unrelated names/attributes stay frozen.
    fn reconcile_metadata(
        &self,
        plan: &cleanup_finalize::Plan,
        parent: &File,
        central: &File,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let header = plan.header();
        let root = super::super::version(central)?;
        let names =
            windows_fs::directory_children_limited(central, discovery::MAX_REGISTRY_ENTRIES)
                .map_err(io)?;
        let mut expected = self.names.clone();
        let mut allowed = vec![header.central_name()];
        if header.parent.same_object(header.central) {
            allowed.push(header.stage_name());
        }
        for name in &allowed {
            if !names.contains(name) {
                expected.retain(|entry| entry != name);
            }
        }
        let mut expected_root = self.root.clone();
        if expected != self.names {
            expected_root.modified = root.modified;
            expected_root.changed = root.changed;
        }
        if names != expected || root != expected_root {
            return Err(io("Copy metadata cleanup has an unrelated registry change"));
        }
        let mut next = self.clone();
        next.root = root;
        next.names = names;
        if !next.names.contains(&header.central_name()) {
            next.records.remove(&header.central_name());
        }
        next.validate(central, cancel)?;
        plan.validate_in_group(parent, central, &next, &mut *cancel)?;
        next.validate(central, cancel)?;
        Ok(next)
    }

    fn check(&self, record: &discovery::RegisteredJournal) -> Result<(), PlatformError> {
        let header = record.state.header.as_ref().unwrap();
        let expected = self
            .records
            .get(&header.central_name())
            .ok_or_else(|| io("Copy group catalog has an unexpected record"))?;
        if super::super::version(&record.file)? != expected.version
            || <[u8; 32]>::from(Sha256::digest(&record.bytes)) != expected.digest
        {
            return Err(io("Copy group catalog record changed during admission"));
        }
        expected.security.validate(&record.file, &expected.creator)
    }

    pub(super) fn admits(
        &self,
        record: &discovery::RegisteredJournal,
    ) -> Result<bool, PlatformError> {
        self.check(record)?;
        Ok(self
            .selected
            .contains(&record.state.header.as_ref().unwrap().transaction))
    }

    fn validate(
        &self,
        central: &File,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if super::super::version(central)? != self.root
            || windows_fs::directory_children_limited(central, discovery::MAX_REGISTRY_ENTRIES)
                .map_err(io)?
                != self.names
        {
            return Err(io("Copy group catalog namespace or epoch changed"));
        }
        self.security.validate(central, &self.creator)?;
        for name in self.records.keys() {
            cancel(0)?;
            self.check(&discovery::RegisteredJournal::dependency(central, name)?)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(super) enum Member {
    Payload(cleanup::Admission),
    Completion(cleanup_finalize::Completion),
    Metadata(cleanup_finalize::Plan),
    Complete(Box<Header>),
}

impl Member {
    pub(super) fn header(&self) -> &Header {
        match self {
            Self::Payload(value) => value.header(),
            Self::Completion(value) => value.header(),
            Self::Metadata(value) => value.header(),
            Self::Complete(header) => header,
        }
    }
    fn advance_catalog(
        &mut self,
        before: &Selection,
        after: &Selection,
    ) -> Result<(), PlatformError> {
        match self {
            Self::Payload(value) => value.advance_catalog(before, after),
            Self::Completion(value) => value.advance_catalog(before, after),
            Self::Metadata(_) | Self::Complete(_) => Ok(()),
        }
    }
}

/// No file handles, raw journals or decoded peer graphs survive this observation.
/// Shared retained objects and cross-member references are explicit inputs for
/// the eventual executor; they are not silently declined or treated as absent.
#[derive(Clone, Debug)]
pub(super) struct Observation {
    pub(super) registry: std::path::PathBuf,
    pub(super) selection: Selection,
    pub(super) members: Vec<Member>,
    pub(super) references: BTreeSet<(usize, usize)>,
    pub(super) shared: BTreeMap<Object, Vec<(usize, usize)>>,
    pending_metadata: Option<usize>,
    pending_unlink: Option<(usize, super::cleanup_peer::Intent)>,
}

impl Observation {
    pub(super) fn started(&self) -> bool {
        self.members.iter().any(|member| match member {
            Member::Payload(admission) => admission.cleanup_started(),
            _ => true,
        })
    }

    pub(super) fn requires_retained_retry(&self) -> bool {
        self.pending_metadata.is_some() || self.pending_unlink.as_ref().is_some_and(|(owner, _)| {
            matches!(&self.members[*owner], Member::Payload(admission) if admission.pending_entry_absent())
        })
    }

    fn accept_member_change(
        &mut self,
        index: usize,
        started: cleanup::ControlChange,
        central: &File,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        let mut selection = self.selection.clone();
        selection.accept_change(&started)?;
        let mut members = self.members.clone();
        members[index] = Member::Payload(started.admission);
        for member in &mut members {
            match member {
                Member::Payload(admission) => {
                    admission.advance_catalog(&self.selection, &selection)?
                }
                Member::Completion(completion) => {
                    completion.advance_catalog(&self.selection, &selection)?
                }
                Member::Metadata(_) | Member::Complete(_) => {}
            }
        }
        // Retain our writer proof before a cancellation or external-state
        // recheck can fail. No handles escape this method.
        self.selection = selection;
        self.members = members;
        self.selection.validate(central, cancel)?;
        Ok(())
    }

    /// Commit every selected payload member's cleanup intent before physical
    /// removal is allowed. Each successful start advances only that writer's
    /// catalog record; failures preserve acknowledged earlier starts for retry.
    /// This method never removes payloads or transaction metadata.
    pub(super) fn start_all(
        &mut self,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self.pending_metadata.is_some() {
            return Err(io(
                "Finish the pending Copy metadata member before another group action",
            ));
        }
        cancel(0)?;
        let manager = AnchoredRoot::open_at(&self.registry, false)?
            .ok_or_else(|| io("Configured Copy group registry is missing"))?;
        let central = manager.writable_root()?;
        self.selection.validate(&central, &mut cancel)?;
        let mut volumes = windows_fs::Volumes::new(&central)?;
        // Validate every confirmed payload before the first durable begin.
        for member in &self.members {
            if matches!(member, Member::Complete(_)) {
                continue;
            }
            let header = member.header();
            let parent = volumes.resolve(header.parent, FILE_GENERIC_READ, &mut cancel)?;
            match member {
                Member::Payload(admission) => {
                    admission.validate_in_group(&parent, &central, &self.selection, &mut cancel)?
                }
                Member::Completion(completion) => {
                    completion.validate_in_group(&parent, &central, &self.selection, &mut cancel)?
                }
                Member::Complete(_) => {}
                Member::Metadata(plan) => {
                    let current = cleanup_finalize::Plan::observe_in_group(
                        &parent,
                        &central,
                        &header.stage_name(),
                        &self.selection,
                        &mut cancel,
                    )?;
                    if current != *plan {
                        return Err(io("Copy group metadata member changed before start"));
                    }
                }
            }
        }
        self.selection.validate(&central, &mut cancel)?;
        for index in 0..self.members.len() {
            let Member::Payload(admission) = &self.members[index] else {
                continue;
            };
            cancel(0)?;
            let parent = if admission
                .header()
                .parent
                .same_object(admission.header().central)
            {
                central.try_clone().map_err(io)?
            } else {
                volumes.resolve(
                    admission.header().parent,
                    FILE_GENERIC_READ | windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE,
                    &mut cancel,
                )?
            };
            let started =
                admission.start_in_group(&parent, &central, &self.selection, &mut cancel)?;
            self.accept_member_change(index, started, &central, &mut cancel)?;
        }
        manager.validate()
    }

    /// Prepare the exact next shared owner entry and each affected peer intent.
    /// No payload is removed. A partially prepared cohort can retry this owner;
    /// its successful control epochs are retained before cancellation checks.
    pub(super) fn prepare_shared_unlink(
        &mut self,
        owner: usize,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<super::cleanup_peer::Intent, PlatformError> {
        if owner >= self.members.len()
            || self
                .pending_unlink
                .as_ref()
                .is_some_and(|(index, _)| *index != owner)
        {
            return Err(io("Invalid or different pending shared unlink owner"));
        }
        self.start_all(&mut cancel)?;
        let manager = AnchoredRoot::open_at(&self.registry, false)?
            .ok_or_else(|| io("Configured Copy group registry is missing"))?;
        let central = manager.writable_root()?;
        self.selection.validate(&central, &mut cancel)?;
        let owner_header = self.members[owner].header().clone();
        // A fresh observer must not prepare another owner while durable peers
        // are waiting for an existing owner, even before any physical unlink.
        for member in &self.members {
            if matches!(member, Member::Complete(_)) {
                continue;
            }
            let record = discovery::RegisteredJournal::dependency(
                &central,
                &member.header().central_name(),
            )?;
            self.selection.check(&record)?;
            if record.state.cleanup.as_ref().is_some_and(|progress| {
                progress.pending && member.header().transaction != owner_header.transaction
                    || progress
                        .peer_unlink
                        .as_deref()
                        .is_some_and(|proof| proof.transaction != owner_header.transaction)
            }) {
                return Err(io("Another durable owner or peer unlink must finish first"));
            }
        }
        let Member::Payload(admission) = &self.members[owner] else {
            return Err(io("Shared unlink owner has no retained payload"));
        };
        let shared = self
            .shared
            .iter()
            .filter(|(_, members)| members.iter().any(|(member, _)| *member == owner))
            .map(|(object, _)| *object)
            .collect();
        let parent = Self::writable_parent(&central, &owner_header, &mut cancel)?;
        let (started, intent) = admission.prepare_owner_unlink(
            &parent,
            &central,
            &self.selection,
            &shared,
            &mut cancel,
        )?;
        if intent.digest != started.after {
            return Err(io(
                "Owner intent does not match acknowledged control prefix",
            ));
        }
        if self
            .pending_unlink
            .as_ref()
            .is_some_and(|(_, prior)| prior != &intent)
        {
            return Err(io("Prepared shared unlink intent changed"));
        }
        self.pending_unlink = Some((owner, intent.clone()));
        self.accept_member_change(owner, started, &central, &mut cancel)?;
        let peers = self
            .shared
            .get(&intent.expected.identity.object_key())
            .ok_or_else(|| io("Prepared owner object is not shared"))?
            .iter()
            .filter_map(|(member, _)| (*member != owner).then_some(*member))
            .collect::<BTreeSet<_>>();
        if peers.is_empty() {
            return Err(io("Prepared unlink has no selected peer"));
        }
        for peer in peers {
            cancel(0)?;
            let Member::Payload(admission) = &self.members[peer] else {
                return Err(io("Shared peer has no retained payload"));
            };
            let parent = Self::writable_parent(&central, admission.header(), &mut cancel)?;
            let started = admission.prepare_peer_unlink(
                &parent,
                &central,
                &self.selection,
                &intent,
                &mut cancel,
            )?;
            self.references.insert((peer, owner));
            self.accept_member_change(peer, started, &central, &mut cancel)?;
        }
        manager.validate()?;
        Ok(intent)
    }

    /// Execute one prepared shared alias, acknowledge each peer, then close the
    /// owner's pending entry. Only successful held changes advance the cohort.
    pub(super) fn unlink_shared_entry(
        &mut self,
        owner: usize,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        let intent = match &self.pending_unlink {
            Some((pending, intent))
                if *pending == owner
                    && matches!(self.members.get(owner), Some(Member::Payload(admission)) if admission.pending_entry_absent()) =>
            {
                // A successful held physical change is already retained.
                // Peers still have their pre-unlink epochs until their ack;
                // validate that exact delta instead of starting them again.
                intent.clone()
            }
            _ => self.prepare_shared_unlink(owner, &mut cancel)?,
        };
        let manager = AnchoredRoot::open_at(&self.registry, false)?
            .ok_or_else(|| io("Configured Copy group registry is missing"))?;
        let central = manager.writable_root()?;
        self.selection.validate(&central, &mut cancel)?;
        let peers = self
            .shared
            .get(&intent.expected.identity.object_key())
            .ok_or_else(|| io("Shared unlink object is missing"))?
            .iter()
            .filter_map(|(member, _)| (*member != owner).then_some(*member))
            .collect::<BTreeSet<_>>();
        let Member::Payload(admission) = &self.members[owner] else {
            return Err(io("Missing shared owner payload"));
        };
        if !admission.pending_entry_absent() {
            let parent = Self::writable_parent(&central, admission.header(), &mut cancel)?;
            let (change, delta) = admission.unlink_shared_entry(
                &parent,
                &central,
                &self.selection,
                &intent,
                &mut cancel,
            )?;
            let mut selection = self.selection.clone();
            selection.accept_payload_change(change.admission.header(), change.after, &delta)?;
            let mut members = self.members.clone();
            for (index, member) in members.iter_mut().enumerate() {
                if index != owner {
                    member.advance_catalog(&self.selection, &selection)?;
                }
            }
            // This admission already contains the successful post-unlink root
            // catalog; do not advance that root twice.
            members[owner] = Member::Payload(change.admission);
            self.selection = selection;
            self.members = members;
            self.selection.validate(&central, &mut cancel)?;
        }
        for peer in peers {
            let Member::Payload(admission) = &self.members[peer] else {
                return Err(io("Missing shared peer payload"));
            };
            let parent = Self::writable_parent(&central, admission.header(), &mut cancel)?;
            let change = admission.acknowledge_shared_peer(
                &parent,
                &central,
                &self.selection,
                &intent,
                &mut cancel,
            )?;
            self.accept_member_change(peer, change, &central, &mut cancel)?;
        }
        let Member::Payload(admission) = &self.members[owner] else {
            return Err(io("Missing shared owner completion"));
        };
        let parent = Self::writable_parent(&central, admission.header(), &mut cancel)?;
        let change = admission.finish_shared_entry(
            &parent,
            &central,
            &self.selection,
            &intent,
            &mut cancel,
        )?;
        // Publish our known control change before an external-state/cancel
        // check can fail; a fresh observer can replay the paired entry ack.
        let mut selection = self.selection.clone();
        selection.accept_change(&change)?;
        let mut members = self.members.clone();
        members[owner] = Member::Payload(change.admission);
        for member in &mut members {
            member.advance_catalog(&self.selection, &selection)?;
        }
        self.selection = selection;
        self.members = members;
        if let Some(owners) = self.shared.get_mut(&intent.expected.identity.object_key()) {
            owners.retain(|&(member, index)| member != owner || index != intent.index);
            if owners
                .iter()
                .map(|(member, _)| *member)
                .collect::<BTreeSet<_>>()
                .len()
                < 2
            {
                self.shared.remove(&intent.expected.identity.object_key());
            }
        }
        self.pending_unlink = None;
        self.selection.validate(&central, &mut cancel)?;
        manager.validate()
    }

    fn writable_parent(
        central: &File,
        header: &Header,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<File, PlatformError> {
        if header.parent.same_object(header.central) {
            central.try_clone().map_err(io)
        } else {
            windows_fs::Volumes::new(central)?.resolve(
                header.parent,
                FILE_GENERIC_READ | windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE,
                cancel,
            )
        }
    }

    fn accept_entry_change(
        &mut self,
        index: usize,
        change: cleanup::EntryChange,
    ) -> Result<(), PlatformError> {
        let mut selection = self.selection.clone();
        let replacement = match change {
            cleanup::EntryChange::Remaining(control, delta) => {
                selection.accept_payload_change(
                    control.admission.header(),
                    control.after,
                    &delta,
                )?;
                Member::Payload(control.admission)
            }
            cleanup::EntryChange::Complete(reclaimed) => {
                selection.accept_payload(&reclaimed.completion, &reclaimed.change)?;
                reclaimed.completion.validate_catalog(&selection)?;
                Member::Completion(reclaimed.completion)
            }
        };
        let mut members = self.members.clone();
        for (other, member) in members.iter_mut().enumerate() {
            if other != index {
                member.advance_catalog(&self.selection, &selection)?;
            }
        }
        // The writer already captured the post-change catalog for this member.
        members[index] = replacement;
        self.members = members;
        self.selection = selection;
        Ok(())
    }

    /// Finish a confirmed cohort in journal order, coordinating each shared
    /// alias before resuming ordinary entries. Errors retain acknowledged work.
    pub(super) fn reclaim_all(
        &mut self,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if let Some(index) = self.pending_metadata {
            self.finish_metadata(index, &mut cancel)?;
        }
        if let Some((owner, _)) = self.pending_unlink.as_ref() {
            let owner = *owner;
            self.unlink_shared_entry(owner, &mut cancel)?;
        }
        self.start_all(&mut cancel)?;
        loop {
            let manager = AnchoredRoot::open_at(&self.registry, false)?
                .ok_or_else(|| io("Configured Copy registry is missing"))?;
            let central = manager.writable_root()?;
            self.selection.validate(&central, &mut cancel)?;
            let mut next = Vec::new();
            for (index, member) in self.members.iter().enumerate() {
                if let Member::Payload(admission) = member {
                    let parent = Self::writable_parent(&central, admission.header(), &mut cancel)?;
                    next.push((
                        index,
                        admission.next_in_group(&parent, &central, &mut cancel)?,
                    ));
                }
            }
            manager.validate()?;
            let priority = next
                .iter()
                .find_map(|(_, step)| match step {
                    cleanup::Next::Peer(transaction) => Some(
                        self.members
                            .iter()
                            .position(|member| member.header().transaction == *transaction)
                            .ok_or_else(|| {
                                io("Pending shared owner is outside the confirmed group")
                            }),
                    ),
                    _ => None,
                })
                .transpose()?;
            let selected = priority
                .or_else(|| {
                    next.iter().find_map(|(index, step)| {
                        matches!(step, cleanup::Next::Entry { pending: true, .. }).then_some(*index)
                    })
                })
                .or_else(|| next.first().map(|(index, _)| *index));
            let Some(index) = selected else {
                break;
            };
            let step = next
                .iter()
                .find(|(member, _)| *member == index)
                .map(|(_, step)| step)
                .ok_or_else(|| io("Pending shared owner has no remaining payload"))?;
            let shared = match step {
                cleanup::Next::Entry { identity, .. } => self
                    .shared
                    .get(&identity.object_key())
                    .is_some_and(|owners| owners.iter().any(|(owner, _)| *owner == index)),
                cleanup::Next::Finish => false,
                cleanup::Next::Peer(_) => {
                    return Err(io("Pending shared owner cannot itself await a peer"));
                }
            };
            // Release registry handles before the next guarded writer opens them.
            drop(central);
            drop(manager);
            if shared {
                self.unlink_shared_entry(index, &mut cancel)?;
            } else {
                let manager = AnchoredRoot::open_at(&self.registry, false)?
                    .ok_or_else(|| io("Configured Copy registry is missing"))?;
                let central = manager.writable_root()?;
                self.selection.validate(&central, &mut cancel)?;
                let Member::Payload(admission) = &self.members[index] else {
                    return Err(io("Selected cleanup member has no payload"));
                };
                let parent = Self::writable_parent(&central, admission.header(), &mut cancel)?;
                let change = admission.reclaim_next_in_group(&parent, &central, &mut cancel)?;
                self.accept_entry_change(index, change)?;
                self.selection.validate(&central, &mut cancel)?;
                manager.validate()?;
            }
        }
        for index in 0..self.members.len() {
            self.finish_metadata(index, &mut cancel)?;
        }
        Ok(())
    }

    /// Reclaim one member through the ordinary physical writer. This retains
    /// its completion proof and advances peers only from that writer's exact
    /// control/registry acknowledgement. Shared aliases need a coordinated
    /// per-unlink protocol; this primitive cannot bypass their protection.
    #[cfg(test)]
    pub(super) fn reclaim_payload(
        &mut self,
        index: usize,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self.pending_unlink.is_some() {
            return Err(io(
                "Finish the prepared shared unlink before another group action",
            ));
        }
        if index >= self.members.len() {
            return Err(io("Copy group member index is invalid"));
        }
        if self
            .shared
            .values()
            .any(|owners| owners.iter().any(|&(owner, _)| owner == index))
        {
            return Err(io(
                "Shared retained aliases require coordinated unlink acknowledgements",
            ));
        }
        self.start_all(&mut cancel)?;
        let Member::Payload(admission) = &self.members[index] else {
            return Ok(());
        };
        let manager = AnchoredRoot::open_at(&self.registry, false)?
            .ok_or_else(|| io("Configured Copy registry is missing"))?;
        let central = manager.writable_root()?;
        self.selection.validate(&central, &mut cancel)?;
        let parent = if admission
            .header()
            .parent
            .same_object(admission.header().central)
        {
            central.try_clone().map_err(io)?
        } else {
            windows_fs::Volumes::new(&central)?.resolve(
                admission.header().parent,
                FILE_GENERIC_READ | windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE,
                &mut cancel,
            )?
        };
        let reclaimed = admission.reclaim_with_ack(&parent, &central, &mut cancel)?;
        self.accept_entry_change(index, cleanup::EntryChange::Complete(reclaimed))?;
        self.selection.validate(&central, &mut cancel)?;
        manager.validate()
    }

    pub(super) fn finish_metadata(
        &mut self,
        index: usize,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self.pending_unlink.is_some() {
            return Err(io(
                "Finish the prepared shared unlink before another group action",
            ));
        }
        if index >= self.members.len() {
            return Err(io("Copy group member index is invalid"));
        }
        if self
            .pending_metadata
            .is_some_and(|pending| pending != index)
        {
            return Err(io(
                "Finish the pending Copy metadata member before another group action",
            ));
        }
        if matches!(self.members[index], Member::Complete(_)) {
            return Ok(());
        }
        if self
            .members
            .iter()
            .any(|member| matches!(member, Member::Payload(_)))
        {
            return Err(io("Complete all group payloads before metadata removal"));
        }
        cancel(0)?;
        let manager = AnchoredRoot::open_at(&self.registry, false)?
            .ok_or_else(|| io("Configured Copy registry is missing"))?;
        let central = manager.writable_root()?;
        let header = self.members[index].header().clone();
        let parent = if header.parent.same_object(header.central) {
            central.try_clone().map_err(io)?
        } else {
            windows_fs::Volumes::new(&central)?.resolve(
                header.parent,
                FILE_GENERIC_READ | windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE,
                &mut cancel,
            )?
        };
        if self.pending_metadata.is_none() {
            self.selection.validate(&central, &mut cancel)?;
            // The entire surviving cohort is still bound before any intent.
            for member in &self.members {
                if matches!(member, Member::Complete(_)) {
                    continue;
                }
                let peer_parent = windows_fs::Volumes::new(&central)?.resolve(
                    member.header().parent,
                    FILE_GENERIC_READ,
                    &mut cancel,
                )?;
                match member {
                    Member::Completion(completion) => completion.validate_in_group(
                        &peer_parent,
                        &central,
                        &self.selection,
                        &mut cancel,
                    )?,
                    Member::Metadata(plan) => plan.validate_in_group(
                        &peer_parent,
                        &central,
                        &self.selection,
                        &mut cancel,
                    )?,
                    Member::Payload(_) | Member::Complete(_) => unreachable!(),
                }
            }
            if let Member::Completion(completion) = &self.members[index] {
                let authorized = completion.begin_with_ack(&parent, &central, &mut cancel)?;
                let mut selection = self.selection.clone();
                selection.accept_control(
                    authorized.plan.header(),
                    authorized.before_journal,
                    authorized.plan.journal_digest(),
                    &authorized.control_version,
                )?;
                let mut members = self.members.clone();
                for (other, member) in members.iter_mut().enumerate() {
                    if other != index {
                        member.advance_catalog(&self.selection, &selection)?;
                    }
                }
                members[index] = Member::Metadata(authorized.plan);
                self.members = members;
                self.selection = selection;
            }
            self.pending_metadata = Some(index);
        }
        let Member::Metadata(plan) = &self.members[index] else {
            return Err(io("Copy group member lacks metadata authority"));
        };
        // A retained Plan, never a missing ID, licenses reconciliation after an
        // uncertain own metadata unlink. Freeze all other members until done.
        let selection = self
            .selection
            .reconcile_metadata(plan, &parent, &central, &mut cancel)?;
        let mut members = self.members.clone();
        for member in &mut members {
            member.advance_catalog(&self.selection, &selection)?;
        }
        self.members = members;
        self.selection = selection;
        let Member::Metadata(plan) = &self.members[index] else {
            unreachable!()
        };
        let result = plan.finish(&parent, &central, &mut cancel);
        let selection = self
            .selection
            .reconcile_metadata(plan, &parent, &central, &mut cancel)?;
        let mut members = self.members.clone();
        for member in &mut members {
            member.advance_catalog(&self.selection, &selection)?;
        }
        self.members = members;
        self.selection = selection;
        result?;
        manager.validate()?;
        self.members[index] = Member::Complete(Box::new(header));
        self.pending_metadata = None;
        Ok(())
    }

    pub(super) fn observe_at(
        path: &Path,
        ids: &[String],
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        cancel(0)?;
        let manager = AnchoredRoot::open_at(path, false)?
            .ok_or_else(|| io("Configured Copy registry is missing"))?;
        let central = manager.root();
        let selection = Selection::capture(central, ids, &mut cancel)?;
        let mut volumes = windows_fs::Volumes::new(central)?;
        let mut members = Vec::new();
        let mut protected: BTreeMap<Object, BTreeSet<usize>> = BTreeMap::new();
        let mut retained: BTreeMap<Object, Vec<(usize, usize)>> = BTreeMap::new();
        for (member_index, id) in ids.iter().enumerate() {
            cancel(0)?;
            let record = discovery::RegisteredJournal::open(
                central,
                &OsString::from(format!("{id}.copy-journal")),
            )?;
            selection.check(&record)?;
            let header = record.state.header.as_ref().unwrap().clone();
            let metadata = record
                .state
                .cleanup
                .as_ref()
                .is_some_and(|p| p.metadata_removal_intent);
            drop(record);
            let parent = volumes.resolve(header.parent, FILE_GENERIC_READ, &mut cancel)?;
            let member = if metadata {
                Member::Metadata(cleanup_finalize::Plan::observe_in_group(
                    &parent,
                    central,
                    &header.stage_name(),
                    &selection,
                    &mut cancel,
                )?)
            } else {
                let admission = cleanup::Admission::observe_in_group(
                    &parent,
                    central,
                    &header.stage_name(),
                    &selection,
                    &mut cancel,
                )?;
                let snapshot =
                    Snapshot::load(&parent, central, &header.stage_name(), SnapshotAccess::Read)?;
                let manifest;
                let (manifest, completed) = if let Some(progress) = &snapshot.state.cleanup {
                    (&progress.manifest, progress.completed)
                } else {
                    manifest = snapshot.state.cleanup_manifest()?;
                    (&manifest, 0)
                };
                for &entry in &manifest.order[completed..] {
                    let key = manifest.entries[entry].version.identity.object_key();
                    protected.entry(key).or_default().insert(member_index);
                    retained.entry(key).or_default().push((member_index, entry));
                }
                admission.validate_metadata(&parent, central, &mut cancel)?;
                Member::Payload(admission)
            };
            if member.header() != &header {
                return Err(io("Copy group member header changed"));
            }
            for identity in [header.stage, header.local_file, header.central_file] {
                protected
                    .entry(identity.object_key())
                    .or_default()
                    .insert(member_index);
            }
            members.push(member);
        }
        let mut references = BTreeSet::new();
        for (index, member) in members.iter().enumerate() {
            cancel(0)?;
            let record =
                discovery::RegisteredJournal::open(central, &member.header().central_name())?;
            selection.check(&record)?;
            for identity in cleanup_dependencies::references(&record.state) {
                if let Some(owners) = protected.get(&identity.object_key()) {
                    references.extend(
                        owners
                            .iter()
                            .filter(|&&owner| owner != index)
                            .map(|&owner| (index, owner)),
                    );
                }
            }
            drop(record);
            let parent = volumes.resolve(member.header().parent, FILE_GENERIC_READ, &mut cancel)?;
            match member {
                Member::Payload(admission) => {
                    admission.validate_metadata(&parent, central, &mut cancel)?
                }
                Member::Completion(completion) => {
                    completion.validate_in_group(&parent, central, &selection, &mut cancel)?
                }
                Member::Complete(_) => {}
                Member::Metadata(plan) => {
                    let current = cleanup_finalize::Plan::observe_in_group(
                        &parent,
                        central,
                        &plan.header().stage_name(),
                        &selection,
                        &mut cancel,
                    )?;
                    if current != *plan {
                        return Err(io("Copy metadata group member changed"));
                    }
                }
            }
        }
        selection.validate(central, &mut cancel)?;
        manager.validate()?;
        retained.retain(|_, owners| {
            owners
                .iter()
                .map(|(owner, _)| owner)
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        });
        Ok(Self {
            registry: manager.path().to_path_buf(),
            selection,
            members,
            references,
            shared: retained,
            pending_metadata: None,
            pending_unlink: None,
        })
    }
}
