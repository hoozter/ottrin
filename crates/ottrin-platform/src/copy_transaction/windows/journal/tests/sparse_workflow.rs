use super::*;

#[test]
fn windows_native_copy_sparse_journal_publication_source_free_undo_and_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir_in(root.path()).unwrap();
    sparse_workflow(root.path(), destination.path());
}

#[test]
#[ignore = "requires an owned secondary NTFS volume provisioned by the native QA controller"]
fn windows_native_copy_sparse_cross_volume_publication_source_free_undo_and_cleanup() {
    let volume = std::path::PathBuf::from(
        std::env::var_os("OTTRIN_TEST_WINDOWS_COPY_DISCOVERY_VOLUME")
            .expect("owned volume missing"),
    );
    for reverse in [false, true] {
        let local = tempfile::tempdir().unwrap();
        let secondary = tempfile::tempdir_in(&volume).unwrap();
        let (root, destination) = if reverse {
            (secondary.path(), local.path().join("destination"))
        } else {
            (local.path(), secondary.path().join("destination"))
        };
        std::fs::create_dir(&destination).unwrap();
        assert_ne!(
            stable_identity_from_handle(&owner(root))
                .unwrap()
                .volume_serial_number(),
            stable_identity_from_handle(&owner(&destination))
                .unwrap()
                .volume_serial_number()
        );
        sparse_workflow(root, &destination);
    }
}

fn sparse_workflow(root: &Path, destination: &Path) {
    for primary_sparse in [false, true] {
        let case = tempfile::tempdir_in(root).unwrap();
        let root = case.path();
        fixture(root);
        let destination = tempfile::tempdir_in(destination).unwrap();
        let destination = destination.path();
        let registry = root.join("central");
        std::fs::remove_dir(&registry).unwrap();
        drop(super::super::super::registry::AnchoredRoot::open_at(&registry, true).unwrap());
        let file = root.join("source/nested/file");
        if primary_sparse {
            super::super::super::sparse::fixture(&owner(&file), 1 << 40, Some(65537));
        }
        for path in [&file, &root.join("source")] {
            let named = format!("{}:thin", path.display());
            std::fs::write(&named, b"").unwrap();
            super::super::super::sparse::fixture(
                &owner(Path::new(&named)),
                4 * 1024 * 1024,
                Some(131073),
            );
        }
        let empty = root.join("source/empty-sparse");
        std::fs::write(&empty, b"").unwrap();
        super::super::super::sparse::fixture(&owner(&empty), 0, None);
        std::fs::write(destination.join("foreign"), b"keep outside transaction").unwrap();
        let src = source(&root.join("source"));
        let parent = owner(destination);
        let central = owner(&root.join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let candidate = staged.stage.candidate_name.clone();
        let id = staged.journal.header.transaction.clone();
        drop((src, staged));
        let mut publication =
            PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        publication.apply(|_| Ok(())).unwrap();
        drop(publication);
        let published = destination.join("copy/nested/file");
        assert_eq!(
            std::fs::metadata(&published).unwrap().len(),
            if primary_sparse { 1 << 40 } else { 13 }
        );
        assert_eq!(
            std::fs::metadata(format!("{}:thin", published.display()))
                .unwrap()
                .len(),
            4 * 1024 * 1024
        );
        let registry = root.join("central");
        let statuses = discover_at(&registry).unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].phase, Some(WindowsCopyPhase::Completed));
        assert_eq!(
            statuses[0].evidence,
            WindowsCopyRecoveryEvidence::VerifiedTree
        );
        std::fs::remove_dir_all(root.join("source")).unwrap();
        undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
        assert!(!destination.join("copy").exists());
        let retained = destination.join(&candidate).join("nested/file");
        assert_eq!(
            std::fs::metadata(retained).unwrap().len(),
            if primary_sparse { 1 << 40 } else { 13 }
        );
        drop((parent, central));
        let mut cleanup =
            RetainedCopyCleanupPlan::observe_at(&registry, &[id], |_| Ok(())).unwrap();
        cleanup.reclaim(|_| Ok(())).unwrap();
        assert!(cleanup.complete());
        assert!(discover_at(&registry).unwrap().is_empty());
        assert!(!destination.join(candidate).exists());
        assert!(!destination.join(name).exists());
        assert_eq!(
            std::fs::read(destination.join("foreign")).unwrap(),
            b"keep outside transaction"
        );
    }
}
