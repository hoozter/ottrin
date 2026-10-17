//! Narrow, durable creation-time preservation after a recorded native rename.
use super::*;

pub(super) fn pending_undo_epochs(
    parent: &File,
    snapshot: &Snapshot,
) -> Result<(Option<EntryVersion>, Option<EntryVersion>), PlatformError> {
    let state = &snapshot.state;
    let copy = if state.undone.is_none() {
        let name = snapshot.root_name(parent)?;
        if state.copy_rename_at(&name) == Some(RenameKind::Withdraw) {
            let root = namespace::open_source(parent, &name, None)?;
            state.validate_copy_metadata(0, &root, &name)?;
            Some(super::super::version(&root)?)
        } else {
            None
        }
    } else {
        None
    };
    let original = if state.original_restore_intent && state.original_restored.is_none() {
        let held = HeldOriginal::open(parent, state, &mut |_| Ok(()))?;
        held.filter(|held| held.name == OsString::from_wide(&state.header.as_ref().unwrap().target))
            .map(|held| held.tree.root().clone())
    } else {
        None
    };
    Ok((copy, original))
}

impl RetainedStage<'_> {
    pub(super) fn pending_copy_version(&self) -> Option<&EntryVersion> {
        self.snapshot
            .state
            .copy_rename_at(&self.tree.root_name)
            .map(|_| &self.tree.entries[0].observed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum RenameKind {
    Publish,
    Withdraw,
    RetainOriginal,
    RestoreOriginal,
}

#[derive(Debug)]
pub(super) struct RenameCreation {
    pub(super) kind: RenameKind,
    pub(super) observed_before: EntryVersion,
    pub(super) epoch: EntryVersion,
    pub(super) applied: Option<EntryVersion>,
}

impl RenameKind {
    pub(super) fn acknowledges(self, record: &Record) -> bool {
        matches!(
            (self, record),
            (Self::Publish, Record::Published)
                | (Self::Withdraw, Record::Undone { .. })
                | (Self::RetainOriginal, Record::OriginalRetained { .. })
                | (Self::RestoreOriginal, Record::OriginalRestored { .. })
        )
    }
    fn active(self, state: &State) -> bool {
        match self {
            Self::Publish => {
                state.publication_intent
                    && !state.published
                    && state.original_ready_for_publication()
            }
            Self::Withdraw => {
                state.completion_verified && state.undo_intent && state.undone.is_none()
            }
            Self::RetainOriginal => {
                state.original_retain_intent
                    && state.original_retained.is_none()
                    && state.original.as_ref().is_some_and(|plan| plan.sealed)
            }
            Self::RestoreOriginal => {
                state.original_retained.is_some()
                    && state.original_restore_intent
                    && state.original_restored.is_none()
            }
        }
    }
    pub(super) fn destination(self, header: &Header) -> OsString {
        match self {
            Self::Publish | Self::RestoreOriginal => OsString::from_wide(&header.target),
            Self::Withdraw => header.candidate_name(),
            Self::RetainOriginal => header.original_name(),
        }
    }
}

impl State {
    pub(super) fn validate_creation_location(
        &self,
        kind: RenameKind,
        name: &OsStr,
    ) -> Result<(), PlatformError> {
        if self
            .rename_creation
            .as_ref()
            .is_some_and(|intent| intent.kind == kind)
            && name != kind.destination(self.header.as_ref().unwrap())
        {
            return Err(io(
                "Copy creation-time owner moved from its recorded rename destination",
            ));
        }
        Ok(())
    }
    pub(super) fn admitted_rename_matches(
        &self,
        kind: RenameKind,
        expected: &EntryVersion,
        actual: &EntryVersion,
    ) -> bool {
        expected == actual
            || self.rename_creation.as_ref().is_some_and(|intent| {
                intent.kind == kind
                    && (expected == &intent.observed_before
                        || expected == &intent.epoch
                        || intent.applied.as_ref() == Some(expected))
                    && self.validate_creation_epoch(kind, actual).is_ok()
            })
    }
    pub(super) fn rename_expected(&self, kind: RenameKind) -> Result<EntryVersion, PlatformError> {
        let invalid = || io("Copy rename has no recorded creation-time authority");
        match kind {
            RenameKind::Publish | RenameKind::Withdraw => {
                let node = self.nodes.first().ok_or_else(invalid)?;
                let mut version = node.source.clone();
                version.identity = node.staged.as_ref().ok_or_else(invalid)?.candidate_identity;
                Ok(version)
            }
            RenameKind::RetainOriginal | RenameKind::RestoreOriginal => self
                .original
                .as_ref()
                .and_then(|plan| plan.evidence.entries.first())
                .map(|entry| entry.version.clone())
                .ok_or_else(invalid),
        }
    }

    pub(super) fn copy_rename_at(&self, name: &OsStr) -> Option<RenameKind> {
        let header = self.header.as_ref()?;
        [RenameKind::Publish, RenameKind::Withdraw]
            .into_iter()
            .find(|kind| kind.active(self) && name == kind.destination(header))
    }

    /// Before a metadata intent, the recorded rename can change only creation
    /// and native change time. All payload, security and other metadata remain
    /// the caller's independently verified evidence. Once intent exists, its
    /// observed epoch or the exact intended creation value are the only options.
    pub(super) fn validate_creation_epoch(
        &self,
        kind: RenameKind,
        actual: &EntryVersion,
    ) -> Result<(), PlatformError> {
        if !kind.active(self) {
            return Err(io("Copy rename creation phase is not active"));
        }
        let mut expected = self.rename_expected(kind)?;
        expected.created = actual.created;
        expected.changed = actual.changed;
        if matches!(kind, RenameKind::Publish | RenameKind::Withdraw)
            && self.readonly_pending == Some(0)
        {
            // The staged metadata verifier independently checks the one allowed
            // readonly transition; this comparison must not add another policy.
            expected.attributes = actual.attributes;
        }
        if *actual != expected {
            return Err(io("Unrelated Copy metadata changed during rename"));
        }
        if let Some(intent) = &self.rename_creation {
            if intent.kind != kind {
                return Err(io("Another rename owns the creation-time intent"));
            }
            if let Some(applied) = &intent.applied {
                let mut expected = applied.clone();
                if self.readonly_pending == Some(0) {
                    expected.changed = actual.changed;
                    expected.attributes = actual.attributes;
                }
                if actual != &expected {
                    return Err(io("Copy creation-time acknowledgement changed"));
                }
            } else {
                let mut before = intent.epoch.clone();
                if self.readonly_pending == Some(0) {
                    before.changed = actual.changed;
                    before.attributes = actual.attributes;
                }
                let mut completed = before.clone();
                completed.created = self.rename_expected(kind)?.created;
                completed.changed = actual.changed;
                if actual != &before && actual != &completed {
                    return Err(io("Copy creation-time intent epoch changed; data retained"));
                }
            }
        }
        Ok(())
    }

    pub(super) fn check_creation_record(&self, record: &Record) -> Result<bool, PlatformError> {
        let invalid = || io("Invalid Copy rename creation-time transition");
        if let Some(intent) = &self.rename_creation {
            let allowed = match &intent.applied {
                None => {
                    matches!(record, Record::RenameCreationApplied { kind, .. } if *kind == intent.kind)
                }
                Some(applied) => {
                    intent.kind.acknowledges(record)
                        && match record {
                            Record::Undone { root_version }
                            | Record::OriginalRetained { root_version }
                            | Record::OriginalRestored { root_version } => {
                                root_version.as_ref() == applied
                            }
                            Record::Published => true,
                            _ => false,
                        }
                }
            };
            let readonly = matches!(intent.kind, RenameKind::Publish | RenameKind::Withdraw)
                && matches!(
                    record,
                    Record::ReadonlyReopenIntent { index: 0 }
                        | Record::ReadonlyReopened { index: 0, .. }
                );
            if !allowed && !readonly {
                return Err(invalid());
            }
        }
        match record {
            Record::RenameCreationIntent { kind, before } => {
                if self.rename_creation.is_some()
                    || !kind.active(self)
                    || before.created == self.rename_expected(*kind)?.created
                    || self.readonly_pending.is_some()
                {
                    return Err(invalid());
                }
                self.validate_creation_epoch(*kind, before)?;
                Ok(true)
            }
            Record::RenameCreationApplied { kind, after } => {
                let intent = self.rename_creation.as_ref().ok_or_else(invalid)?;
                let mut expected = intent.epoch.clone();
                expected.created = self.rename_expected(*kind)?.created;
                expected.changed = after.changed;
                if intent.kind != *kind || intent.applied.is_some() || **after != expected {
                    return Err(invalid());
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub(super) fn validate_copy_metadata(
        &self,
        index: usize,
        file: &File,
        name: &OsStr,
    ) -> Result<(), PlatformError> {
        let mut evidence = self.nodes[index].staged.as_ref().unwrap().clone();
        if index == 0
            && let Some(intent) = &self.rename_creation
            && matches!(intent.kind, RenameKind::Publish | RenameKind::Withdraw)
        {
            self.validate_creation_location(intent.kind, name)?;
        }
        if index == 0
            && let Some(kind) = self.copy_rename_at(name)
        {
            let actual = super::super::version(file)?;
            self.validate_creation_epoch(kind, &actual)?;
            evidence.source.created = actual.created;
        }
        evidence.validate_reopening_metadata(file, self.readonly_pending == Some(index))
    }
}

impl Journal {
    pub(super) fn creation_tail(&self, kind: RenameKind) -> Result<Vec<Record>, PlatformError> {
        let mut after = self.state.rename_expected(kind)?;
        after.changed = i64::MIN;
        let mut before = after.clone();
        before.created = i64::MIN;
        if let Some(intent) = &self.state.rename_creation
            && intent.kind == kind
        {
            return Ok(if intent.applied.is_some() {
                Vec::new()
            } else {
                vec![Record::RenameCreationApplied {
                    kind,
                    after: Box::new(after),
                }]
            });
        }
        Ok(vec![
            Record::RenameCreationIntent {
                kind,
                before: Box::new(before),
            },
            Record::RenameCreationApplied {
                kind,
                after: Box::new(after),
            },
        ])
    }

    /// Caller has verified the complete tree and destination binding under the
    /// recorded rename phase, holding its native root capability throughout.
    pub(super) fn finish_creation(
        &mut self,
        kind: RenameKind,
        root: &File,
    ) -> Result<EntryVersion, PlatformError> {
        if self.failed {
            return Err(io(
                "Copy creation-time owner is uncertain; reopen for recovery",
            ));
        }
        self.bindings()?;
        let actual = super::super::version(root)?;
        self.state.validate_creation_epoch(kind, &actual)?;
        bound(
            &self.parent_dir,
            &kind.destination(&self.header),
            actual.identity,
        )?;
        let created = self.state.rename_expected(kind)?.created;
        if self.state.rename_creation.is_none() {
            if actual.created == created {
                return Ok(actual);
            }
            self.persist(Record::RenameCreationIntent {
                kind,
                before: Box::new(actual.clone()),
            })?;
        }
        if self
            .state
            .rename_creation
            .as_ref()
            .unwrap()
            .applied
            .is_some()
        {
            return Ok(actual);
        }
        self.failed = true;
        if actual.created != created {
            metadata::set_creation_time(root, &actual, created)?;
        }
        boundary(self.sequence, Step::CreationRestored)?;
        root.sync_all().map_err(io)?;
        boundary(self.sequence, Step::CreationFlushed)?;
        let after = super::super::version(root)?;
        self.state.validate_creation_epoch(kind, &after)?;
        self.bindings()?;
        self.failed = false;
        self.persist(Record::RenameCreationApplied {
            kind,
            after: Box::new(after.clone()),
        })?;
        Ok(after)
    }
}
