    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires a nonce-owned disposable generated-cache copy"]
    fn profile_staged_storage_phases() {
        use std::time::Instant;
        fn cpu() -> f64 {
            let mut time = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            // SAFETY: the process clock is valid on Linux and time is writable.
            assert_eq!(
                unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut time) },
                0
            );
            time.tv_sec as f64 + time.tv_nsec as f64 / 1_000_000_000.0
        }
        fn measure<T>(
            phases: &mut Vec<serde_json::Value>,
            name: &str,
            action: impl FnOnce() -> T,
        ) -> T {
            let wall = Instant::now();
            let start = cpu();
            let result = action();
            phases.push(serde_json::json!({"phase":name,"cpu_seconds":cpu()-start,"wall_seconds":wall.elapsed().as_secs_f64()}));
            result
        }
        let root = PathBuf::from(
            std::env::var_os("OTTRIN_PHASE_PROFILE_ROOT").expect("owned profile missing"),
        )
        .canonicalize()
        .unwrap();
        assert_eq!(root.parent(), Some(Path::new("/var/tmp")));
        assert!(
            root.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".ottrin-index-phase-")
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".phase-owner")).unwrap(),
            std::env::var("OTTRIN_PHASE_PROFILE_NONCE").unwrap()
        );
        let path = search_db_path();
        assert!(path.starts_with(&root));
        let config: ottrin_core::SearchConfig =
            serde_json::from_slice(&std::fs::read(root.join("phase-config.json")).unwrap())
                .unwrap();
        assert!(
            !config.content_indexing_enabled,
            "this replay must never read source file contents"
        );
        let plan = ScanPlan::new(&crate::resolved_search_config(config)).unwrap();
        let mut phases = Vec::new();
        let mut store = measure(&mut phases, "open", || SearchStore::open(path).unwrap());
        let snapshot = measure(&mut phases, "load_metadata_snapshot", || {
            store
                .load(&plan)
                .unwrap()
                .expect("copied generation must match configured plan")
        });
        let generation = store.begin_staged_generation(&plan).unwrap();
        attach_generation_content_db(
            &store.conn,
            &generation_content_db_path(&store.path, generation),
        )
        .unwrap();
        for chunk in snapshot.items().chunks(8192) {
            let tx = store.conn.transaction().unwrap();
            measure(&mut phases, "metadata_rows", || {
                upsert_generation_item_rows(&tx, generation, chunk, &plan).unwrap()
            });
            measure(&mut phases, "filename_fts", || {
                insert_generation_path_rows(&tx, generation, chunk).unwrap()
            });
            measure(&mut phases, "content_state", || {
                insert_generation_content(&tx, generation, chunk, &plan).unwrap()
            });
            measure(&mut phases, "commit", || tx.commit().unwrap());
            measure(&mut phases, "checkpoint", || {
                checkpoint_wal_passively(&store.conn).unwrap()
            });
        }
        detach_generation_content_db(&store.conn);
        let fence = ActivationFence::new();
        let count = measure(&mut phases, "validate_publish_retire", || {
            store
                .activate_staged_generation(
                    &plan,
                    generation,
                    snapshot.items().len(),
                    &fence,
                    fence.capture(),
                )
                .unwrap()
        });
        assert_eq!(count, snapshot.items().len());
        assert_eq!(
            store
                .load_count_cancellable(&plan, &AtomicBool::new(false))
                .unwrap(),
            Some(count)
        );
        let version: String = store
            .conn
            .query_row("SELECT sqlite_version()", [], |row| row.get(0))
            .unwrap();
        let report = serde_json::json!({"scope":"Storage-phase replay of a private generated-cache copy; no filesystem freshness or complete daemon acceptance", "items":count,"sqlite_version":version,"phases":phases});
        std::fs::write(
            root.join("PHASES.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
