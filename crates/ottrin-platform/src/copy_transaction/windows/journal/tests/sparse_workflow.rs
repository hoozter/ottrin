use super::*;

#[test]
fn windows_native_copy_sparse_journal_publication_source_free_undo_and_cleanup() {
    for primary_sparse in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let registry = root.path().join("central");
        std::fs::remove_dir(&registry).unwrap();
        drop(super::super::super::registry::AnchoredRoot::open_at(&registry, true).unwrap());
        let file = root.path().join("source/nested/file");
        if primary_sparse {
            super::super::super::sparse::fixture(&owner(&file), 1 << 40, Some(65536));
        }
        for path in [&file, &root.path().join("source")] {
            let named = format!("{}:thin", path.display());
            std::fs::write(&named, b"").unwrap();
            super::super::super::sparse::fixture(
                &owner(Path::new(&named)),
                4 * 1024 * 1024,
                Some(131072),
            );
        }
        let empty = root.path().join("source/empty-sparse");
        std::fs::write(&empty, b"").unwrap();
        super::super::super::sparse::fixture(&owner(&empty), 0, None);
        std::fs::write(
            root.path().join("destination/foreign"),
            b"keep outside transaction",
        )
        .unwrap();
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let candidate = staged.stage.candidate_name.clone();
        let id = staged.journal.header.transaction.clone();
        drop((src, staged));
        let mut publication =
            PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        publication.apply(|_| Ok(())).unwrap();
        drop(publication);
        let published = root.path().join("destination/copy/nested/file");
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
        let registry = root.path().join("central");
        let statuses = discover_at(&registry).unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].phase, Some(WindowsCopyPhase::Completed));
        assert_eq!(
            statuses[0].evidence,
            WindowsCopyRecoveryEvidence::VerifiedTree
        );
        std::fs::remove_dir_all(root.path().join("source")).unwrap();
        undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
        assert!(!root.path().join("destination/copy").exists());
        let retained = root
            .path()
            .join("destination")
            .join(&candidate)
            .join("nested/file");
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
        assert!(!root.path().join("destination").join(candidate).exists());
        assert!(!root.path().join("destination").join(name).exists());
        assert_eq!(
            std::fs::read(root.path().join("destination/foreign")).unwrap(),
            b"keep outside transaction"
        );
    }
}
