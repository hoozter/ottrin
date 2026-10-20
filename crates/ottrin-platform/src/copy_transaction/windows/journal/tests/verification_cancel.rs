use super::*;

#[test]
fn windows_native_copy_verification_cancellation_keeps_unpublished_durable_stage() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let original = vec![37u8; super::super::super::BUFFER_BYTES * 4 + 17];
    let path = root.path().join("source/nested/file");
    std::fs::write(&path, &original).unwrap();
    let src = source(&root.path().join("source"));
    let parent = owner(&root.path().join("destination"));
    let central = owner(&root.path().join("central"));
    let destination = DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap();
    let mut complete_callbacks = 0;
    let error = match DurableStage::create(
        &src,
        &parent,
        &central,
        &destination,
        OsStr::new("source"),
        100,
        |bytes| {
            if bytes == original.len() as u64 {
                complete_callbacks += 1;
                if complete_callbacks == 3 {
                    return Err(io("stopped during durable stream verification"));
                }
            }
            Ok(())
        },
    ) {
        Ok(_) => panic!("verification cancellation was ignored"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("stopped during durable stream verification")
    );
    assert_eq!(complete_callbacks, 3);
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert!(!root.path().join("destination/copy").exists());
    let name = stage_name(&root.path().join("destination"));
    let bytes = std::fs::read(root.path().join("destination").join(&name).join(LOCAL)).unwrap();
    let journal = decode(&bytes).unwrap();
    let header = journal.state.header.as_ref().unwrap();
    assert_eq!(
        std::fs::read(
            root.path()
                .join("destination")
                .join(header.candidate_name())
                .join("nested/file")
        )
        .unwrap(),
        original
    );
    assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
    assert_eq!(
        std::fs::read(root.path().join("destination").join(&name).join(LOCAL)).unwrap(),
        bytes
    );
}
