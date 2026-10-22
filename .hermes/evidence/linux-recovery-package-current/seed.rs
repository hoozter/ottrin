use ottrin_platform::move_recovery::{LinuxFileType, LinuxJournalDirectory, LinuxSourceParentDirectory, LinuxStableIdentity, PreparedRecord, PendingDestinationMetadata, SourceContentEvidence, SourceTreeSnapshot};
use std::{fs, path::{Path, PathBuf}, os::unix::fs::MetadataExt};
fn identity(path: &Path, kind: LinuxFileType) -> LinuxStableIdentity {
    let m=fs::symlink_metadata(path).unwrap();
    LinuxStableIdentity::new(m.dev(),m.ino(),kind)
}
fn main() {
    let root=PathBuf::from(std::env::args_os().nth(1).unwrap());
    assert!(root.is_absolute() && root.join("home").is_dir());
    let state=root.join("state/ottrin/moves");
    let source_parent=root.join("recovery-source");
    let destination=root.join("destination");
    assert!(!state.exists() && !source_parent.exists());
    fs::create_dir_all(&state).unwrap();fs::create_dir(&source_parent).unwrap();
    let id=[0x94;16];
    let local_name=Path::new(".ottrin-move-94949494949494949494949494949494");
    let source=source_parent.join("recovered.txt");
    fs::write(&source,b"RECOVERED MOVE PAYLOAD\n").unwrap();
    let parent=LinuxSourceParentDirectory::open(&source_parent).unwrap();
    let transaction=parent.create_transaction(id).unwrap();
    assert_eq!(transaction.name(),local_name);
    let local=parent.open_created_transaction(&transaction).unwrap();
    let local_path=source_parent.join(local_name);
    let source_id=identity(&source,LinuxFileType::Regular);
    let central=LinuxJournalDirectory::open_configured(&state).unwrap();
    let prepared=PreparedRecord::new_linux(id,&source_parent,Path::new("recovered.txt"),identity(&source_parent,LinuxFileType::Directory),source_id,local_name,identity(&local_path,LinuxFileType::Directory)).unwrap();
    let mut lock=central.try_lock_recovery().unwrap();
    lock.prepare_with_local(&local,&prepared).unwrap();
    lock.advance_to_claim_intent_with_local(&local,&prepared).unwrap();
    lock.claim_source_after_intent(&parent,&local,id).unwrap();
    lock.advance_to_claimed_with_local(&local,id).unwrap();
    let dest_parent=LinuxSourceParentDirectory::open(&destination).unwrap();
    let dest_transaction=dest_parent.create_transaction(id).unwrap();
    let dest_path=destination.join(dest_transaction.name());
    fs::copy(local_path.join("claimed"),dest_path.join("candidate")).unwrap();
    let payload=fs::read(local_path.join("claimed")).unwrap();
    let hash=std::process::Command::new("sha256sum").arg(local_path.join("claimed")).output().unwrap();
    assert!(hash.status.success());
    let hash=String::from_utf8(hash.stdout).unwrap().split_whitespace().next().unwrap().to_owned();
    let snapshot=SourceTreeSnapshot::from_evidenced_entries(vec![(PathBuf::new(),source_id,SourceContentEvidence::Regular{size:payload.len() as u64,sha256_hex:hash})]);
    lock.advance_to_pending_destination_with_local_with_metadata(&local,id,&destination.join("recovered.txt"),PendingDestinationMetadata{
        destination_transaction_name:Some(dest_transaction.name().to_path_buf()),
        destination_transaction_identity:Some(dest_transaction.observed_identity()),
        source_tree_snapshot:Some(snapshot),staged_destination_identity:Some(identity(&dest_path.join("candidate"),LinuxFileType::Regular)),expected_displaced_destination_identity:None,
    }).unwrap();
    assert!(!source.exists());
    println!("Created paired native PendingDestination records and staged payload for 94949494949494949494949494949494");
}
