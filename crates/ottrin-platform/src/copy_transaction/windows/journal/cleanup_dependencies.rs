//! Registry dependency leases held for the duration of explicit cleanup.
use super::cleanup_protocol::Manifest;
use super::*;

pub(super) struct Dependencies {
    central: WindowsStableIdentity,
    stamp: EntryVersion,
    security: security::PrivateSecurity,
    creator: Vec<u8>,
    // Reading headers alone does not grant payload deletion authority. Keeping
    // the leases alive prevents a peer from changing those dependency claims.
    peers: Vec<(OsString, discovery::RegisteredJournal)>,
    names: Vec<OsString>,
}

impl Dependencies {
    pub(super) fn open(
        central: &File,
        header: &Header,
        manifest: &Manifest,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let creator = security::current_owner()?;
        let security = security::PrivateSecurity::capture(central, &creator)?;
        let stamp = super::super::version(central)?;
        let mut protected: BTreeSet<_> = manifest
            .entries
            .iter()
            .map(|entry| entry.version.identity.object_key())
            .collect();
        protected.extend(
            [header.stage, header.local_file, header.central_file].map(|id| id.object_key()),
        );
        let names =
            windows_fs::directory_children_limited(central, discovery::MAX_REGISTRY_ENTRIES)
                .map_err(io)?;
        let mut peers = Vec::new();
        for name in &names {
            cancel(0)?;
            if *name == header.central_name() || discovery::registry_id(name).is_none() {
                continue;
            }
            let peer = discovery::RegisteredJournal::dependency(central, name)?;
            let other = peer.state.header.as_ref().unwrap();
            // Incomplete source/original manifests cannot certify an absent
            // dependency. They must be resolved before destructive cleanup.
            if !peer.state.verified
                || other.original_admission.is_some()
                    && !peer.state.original.as_ref().is_some_and(|plan| plan.sealed)
            {
                return Err(io(format!(
                    "Copy cleanup needs completed dependency evidence from {}",
                    other.transaction
                )));
            }
            let mut references = vec![
                other.parent,
                other.stage,
                other.local_file,
                other.central_file,
            ];
            references.extend(peer.state.nodes.iter().filter_map(|node| node.created));
            if let Some(original) = &peer.state.original {
                references.extend(
                    original
                        .evidence
                        .entries
                        .iter()
                        .map(|entry| entry.version.identity),
                );
            }
            if references
                .iter()
                .any(|id| protected.contains(&id.object_key()))
            {
                return Err(io(format!(
                    "Copy cleanup would affect dependent transaction {}",
                    other.transaction
                )));
            }
            peers.push((name.clone(), peer));
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

    pub(super) fn validate(&self, central: &File) -> Result<(), PlatformError> {
        if stable_identity_from_handle(central).map_err(io)? != self.central
            || super::super::version(central)? != self.stamp
        {
            return Err(io("Copy cleanup registry epoch changed"));
        }
        self.security.validate(central, &self.creator)
    }

    pub(super) fn revalidate_peers(&self, central: &File) -> Result<(), PlatformError> {
        self.validate(central)?;
        self.peer_bindings(central)
    }

    fn peer_bindings(&self, central: &File) -> Result<(), PlatformError> {
        for (name, peer) in &self.peers {
            let header = peer.state.header.as_ref().unwrap();
            bound(central, name, header.central_file)?;
            if read(&peer.file)? != peer.bytes {
                return Err(io("Copy cleanup dependency record changed"));
            }
            header
                .central_security
                .validate(&peer.file, &header.creator)?;
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
