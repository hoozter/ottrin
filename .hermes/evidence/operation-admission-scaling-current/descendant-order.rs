#[path=r"/home/campbell/Projects/Ottrin/crates/ottrin-ui/src/operation_paths.rs"] mod operation_paths;
use operation_paths::{OperationPath,OperationPathIndex};
use std::path::PathBuf;
fn main(){let paths=[OperationPath::write(PathBuf::from("a-b")),OperationPath::write(PathBuf::from("a/b"))];let held=[OperationPath::read(PathBuf::from("a"))];assert!(OperationPathIndex::new(&paths).conflicts(&held));println!("Sibling cannot hide a component descendant: pass");}
