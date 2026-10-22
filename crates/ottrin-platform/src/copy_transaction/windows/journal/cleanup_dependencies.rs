//! Registry dependency leases held for the duration of explicit cleanup.
use super::cleanup_protocol::Manifest;
use super::*;

/// Retain only the claim's immutable digest and native control lease. The
/// decoded payload graph is needed for overlap checking, not for the lifetime
/// of every other peer in a potentially large catalog.
struct Peer {
    file: File,
    version: EntryVersion,
    security: security::PrivateSecurity,
    creator: Vec<u8>,
    digest: [u8; 32],
    // Terminal incomplete peers need both controls and their native directory
    // bindings held, not merely a central record that says staging ended.
    _pair: Vec<File>,
}

pub(super) struct Dependencies {
    central: WindowsStableIdentity,
    stamp: EntryVersion,
    security: security::PrivateSecurity,
    creator: Vec<u8>,
    // Reading headers alone does not grant payload deletion authority. Keeping
    // the leases alive prevents a peer from changing those dependency claims.
    peers: Vec<(OsString, Peer)>,
    names: Vec<OsString>,
}

enum Scope<'a> {
    Ordinary,
    Current(WindowsStableIdentity),
    Inspect(&'a super::cleanup_group::Selection),
    Entry(
        &'a super::cleanup_group::Selection,
        &'a super::cleanup_peer::Intent,
        bool,
    ),
}

impl Dependencies {
    pub(super) fn open_selected(
        central: &File,
        state: &State,
        manifest: &Manifest,
        selection: Option<&super::cleanup_group::Selection>,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        Self::open_scope(
            central,
            state,
            manifest,
            selection.map(Scope::Inspect).unwrap_or(Scope::Ordinary),
            cancel,
        )
    }

    /// Only the journal's actual next entry is a payload target. No selected
    /// peer reference is waived by this ordinary per-entry scope.
    pub(super) fn current_entry(
        central: &File,
        journal: &Journal,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        journal.bindings()?;
        let progress = journal
            .state
            .cleanup
            .as_ref()
            .ok_or_else(|| io("Missing current cleanup entry"))?;
        if progress.peer_unlink.is_some() {
            return Err(io("A pending peer unlink must be acknowledged first"));
        }
        let &index = progress
            .manifest
            .order
            .get(progress.completed)
            .ok_or_else(|| io("No current cleanup entry"))?;
        Self::open_scope(
            central,
            &journal.state,
            &progress.manifest,
            Scope::Current(progress.expected(index).identity),
            cancel,
        )
    }

    pub(super) fn shared_unlink(
        central: &File,
        journal: &Journal,
        selection: &super::cleanup_group::Selection,
        intent: &super::cleanup_peer::Intent,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        Self::shared_entry(central, journal, selection, intent, false, cancel)
    }

    pub(super) fn shared_acknowledgement(
        central: &File,
        journal: &Journal,
        selection: &super::cleanup_group::Selection,
        intent: &super::cleanup_peer::Intent,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        Self::shared_entry(central, journal, selection, intent, true, cancel)
    }

    fn shared_entry(
        central: &File,
        journal: &Journal,
        selection: &super::cleanup_group::Selection,
        intent: &super::cleanup_peer::Intent,
        acknowledged: bool,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        journal.bindings()?;
        if super::cleanup_peer::Intent::capture(journal)? != *intent {
            return Err(io("Shared entry does not match held owner intent"));
        }
        Self::open_scope(
            central,
            &journal.state,
            &journal.state.cleanup.as_ref().unwrap().manifest,
            Scope::Entry(selection, intent, acknowledged),
            cancel,
        )
    }

    fn open_scope(
        central: &File,
        state: &State,
        manifest: &Manifest,
        scope: Scope<'_>,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let header = state.header.as_ref().unwrap();
        let creator = windows_fs::current_owner()?;
        let security = security::PrivateSecurity::capture(central, &creator)?;
        let stamp = super::super::version(central)?;
        let mut protected: BTreeSet<_> = match &scope {
            Scope::Entry(_, intent, _) => BTreeSet::from([intent.expected.identity.object_key()]),
            Scope::Current(identity) => BTreeSet::from([identity.object_key()]),
            _ => {
                if let Some(progress) = &state.cleanup {
                    progress.manifest.order[progress.completed..]
                        .iter()
                        .map(|&index| {
                            progress.manifest.entries[index]
                                .version
                                .identity
                                .object_key()
                        })
                        .collect()
                } else {
                    manifest
                        .entries
                        .iter()
                        .map(|entry| entry.version.identity.object_key())
                        .collect()
                }
            }
        };
        protected.extend(
            [header.stage, header.local_file, header.central_file].map(|id| id.object_key()),
        );
        let names =
            windows_fs::directory_children_limited(central, discovery::MAX_REGISTRY_ENTRIES)
                .map_err(io)?;
        let mut peers = Vec::new();
        let mut volumes = None;
        for name in &names {
            cancel(0)?;
            if *name == header.central_name() {
                continue;
            }
            if discovery::registry_id(name).is_none() {
                if discovery::registry_candidate(name) {
                    return Err(io(format!(
                        "Copy cleanup needs review of unrecognized registry record {}",
                        name.to_string_lossy()
                    )));
                }
                continue;
            }
            let peer = discovery::RegisteredJournal::dependency(central, name)?;
            let other = peer.state.header.as_ref().unwrap();
            // Active incomplete manifests can still add creation claims. An
            // ended peer is frozen, but only a matching, leased pair can certify
            // that terminal state. Recorded creation identities remain protected.
            if !peer.state.dependency_claims_complete() {
                return Err(io(format!(
                    "Copy cleanup needs completed dependency evidence from {}",
                    other.transaction
                )));
            }
            let pair = if peer.state.aborted {
                if volumes.is_none() {
                    volumes = Some(windows_fs::Volumes::new(central)?);
                }
                let parent = volumes.as_mut().unwrap().resolve(
                    other.parent,
                    FILE_GENERIC_READ,
                    &mut cancel,
                )?;
                peer.paired_leases(parent, central)?
            } else {
                Vec::new()
            };
            let references = references(&peer.state);
            let selection = match &scope {
                Scope::Ordinary | Scope::Current(_) => None,
                Scope::Inspect(selection) | Scope::Entry(selection, _, _) => Some(*selection),
            };
            let selected = selection
                .map(|s| s.admits(&peer))
                .transpose()?
                .unwrap_or(false);
            let overlaps = references
                .iter()
                .filter(|id| protected.contains(&id.object_key()))
                .collect::<Vec<_>>();
            if !overlaps.is_empty() {
                let allowed = match &scope {
                    Scope::Ordinary | Scope::Current(_) => false,
                    Scope::Inspect(_) => selected,
                    Scope::Entry(_, intent, acknowledged) => {
                        let matching = if *acknowledged {
                            intent.acknowledged_by(&peer.state)?
                        } else {
                            let proof = intent.bind_local(&peer.state)?;
                            peer.state
                                .cleanup
                                .as_ref()
                                .is_some_and(|p| p.peer_unlink.as_deref() == Some(&proof))
                        };
                        selected
                            && matching
                            && overlaps.iter().all(|id| {
                                **id == intent.expected.identity || **id == intent.control
                            })
                    }
                };
                if !allowed {
                    return Err(io(format!(
                        "Copy cleanup would affect dependent transaction {}",
                        other.transaction
                    )));
                }
            }
            let lease = Peer {
                version: super::super::version(&peer.file)?,
                security: other.central_security.clone(),
                creator: other.creator.clone(),
                digest: Sha256::digest(&peer.bytes).into(),
                file: peer.file,
                _pair: pair,
            };
            // The raw record and decoded graph drop at this iteration's end.
            peers.push((name.clone(), lease));
        }
        if windows_fs::directory_children_limited(central, discovery::MAX_REGISTRY_ENTRIES)
            .map_err(io)?
            != names
            || super::super::version(central)? != stamp
        {
            return Err(io(
                "Copy cleanup registry changed during dependency admission",
            ));
        }
        let result = Self {
            central: header.central,
            stamp,
            security,
            creator,
            peers,
            names,
        };
        result.validate(central)?;
        Ok(result)
    }

    pub(super) fn namespace_proof(&self) -> (EntryVersion, Vec<OsString>) {
        (self.stamp.clone(), self.names.clone())
    }

    pub(super) fn validate(&self, central: &File) -> Result<(), PlatformError> {
        if stable_identity_from_handle(central).map_err(io)? != self.central
            || super::super::version(central)? != self.stamp
        {
            return Err(io("Copy cleanup registry epoch changed"));
        }
        self.security.validate(central, &self.creator)
    }

    /// Freeze dependency claims for a confirmation plan that retains no leases.
    /// Raw UTF-16 names preserve native names that are not Unicode scalar text.
    pub(super) fn fingerprint(&self) -> Result<[u8; 32], PlatformError> {
        catalog_fingerprint(
            self.central,
            &self.stamp,
            &self.security,
            &self.creator,
            &self.names,
            self.peers
                .iter()
                .map(|(name, peer)| (name.as_os_str(), peer.digest, &peer.version)),
        )
    }

    pub(super) fn revalidate_peers(&self, central: &File) -> Result<(), PlatformError> {
        self.validate(central)?;
        self.peer_bindings(central)
    }

    fn peer_bindings(&self, central: &File) -> Result<(), PlatformError> {
        for (name, peer) in &self.peers {
            bound(central, name, peer.version.identity)?;
            namespace::validate_single_link(&peer.file)?;
            if super::super::version(&peer.file)? != peer.version
                || <[u8; 32]>::from(Sha256::digest(read(&peer.file)?)) != peer.digest
                || super::super::version(&peer.file)? != peer.version
            {
                return Err(io("Copy cleanup dependency record or native epoch changed"));
            }
            peer.security.validate(&peer.file, &peer.creator)?;
        }
        Ok(())
    }

    /// A destination can also be the supplied registry. Refresh its epoch only
    /// for this proven root unlink, after checking the exact expected namespace
    /// delta and all held peer records. Never normalize an unrelated change.
    pub(super) fn own_root_removed(
        &mut self,
        central: &File,
        name: &OsStr,
    ) -> Result<(), PlatformError> {
        let mut names = self.names.clone();
        let index = names
            .iter()
            .position(|entry| entry == name)
            .ok_or_else(|| io("Copy cleanup registry did not contain its retained root"))?;
        names.remove(index);
        if windows_fs::directory_children_limited(central, discovery::MAX_REGISTRY_ENTRIES)
            .map_err(io)?
            != names
        {
            return Err(io(
                "Copy cleanup registry has an unexpected namespace change",
            ));
        }
        let actual = super::super::version(central)?;
        let mut expected = self.stamp.clone();
        expected.modified = actual.modified;
        expected.changed = actual.changed;
        if actual != expected {
            return Err(io("Copy cleanup registry metadata changed"));
        }
        self.security.validate(central, &self.creator)?;
        self.peer_bindings(central)?;
        self.names = names;
        self.stamp = actual;
        Ok(())
    }
}

pub(super) fn references(state: &State) -> Vec<WindowsStableIdentity> {
    let header = state.header.as_ref().unwrap();
    let mut references = vec![
        header.parent,
        header.stage,
        header.local_file,
        header.central_file,
    ];
    // These are protected native identities, not filesystem existence leases.
    // Keep them after terminal release: a peer cannot delete claim controls as
    // payload, while old recovery may outlive a replaced reservation file.
    references.extend(header.claim.identities());
    if let Some(cleanup) = &state.cleanup {
        // A pending peer acknowledgement needs the owner's canonical journal
        // and paired metadata for native replay, even after its payload is gone.
        references.extend(cleanup.peer_unlink.as_ref().map(|proof| proof.control));
        references.extend(
            cleanup.manifest.order[cleanup.completed..]
                .iter()
                .map(|&index| cleanup.manifest.entries[index].version.identity),
        );
    } else {
        references.extend(state.nodes.iter().filter_map(|node| node.created));
        if state.original_restored.is_none()
            && let Some(original) = &state.original
        {
            references.extend(
                original
                    .evidence
                    .entries
                    .iter()
                    .map(|entry| entry.version.identity),
            );
        }
    }
    references
}

/// One encoding for held dependency leases and handle-free group catalogs.
pub(super) fn catalog_fingerprint<'a>(
    central: WindowsStableIdentity,
    stamp: &EntryVersion,
    security: &security::PrivateSecurity,
    creator: &[u8],
    names: &[OsString],
    peers: impl Iterator<Item = (&'a OsStr, [u8; 32], &'a EntryVersion)>,
) -> Result<[u8; 32], PlatformError> {
    let names: Vec<Vec<u16>> = names
        .iter()
        .map(|name| name.encode_wide().collect())
        .collect();
    let peers: Vec<_> = peers
        .map(|(name, digest, version)| (name.encode_wide().collect::<Vec<_>>(), digest, version))
        .collect();
    super::super::evidence_digest(&(central, stamp, security, creator, names, peers))
}
