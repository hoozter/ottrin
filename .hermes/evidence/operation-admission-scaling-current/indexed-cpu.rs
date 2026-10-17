#[path = r"/home/campbell/Projects/Ottrin/crates/ottrin-ui/src/operation_paths.rs"]
mod operation_paths;
use operation_paths::{OperationPath,OperationPathIndex};
use std::{path::PathBuf,time::Instant,hint::black_box};
fn conflicts(paths:&[OperationPath],requested:&[OperationPath])->bool { OperationPathIndex::new(requested).conflicts(paths) }
fn main(){ for n in [5000usize] {
 let mut held:Vec<_>=(0..n).map(|i|OperationPath::write(PathBuf::from(format!("/held/item-{i}")))).collect();
 let mut requested:Vec<_>=(0..n).map(|i|OperationPath::write(PathBuf::from(format!("/requested/item-{i}")))).collect();
 held.push(OperationPath::write(PathBuf::from("/first")));requested.push(OperationPath::write(PathBuf::from("/second")));
 let mut times=Vec::new();for _ in 0..1 {let start=Instant::now();assert!(!black_box(conflicts(black_box(&held),black_box(&requested))));times.push(start.elapsed().as_nanos());}
 requested[0].writes=true;requested[0].path=held[0].path.clone();assert!(conflicts(&held,&requested));times.sort();println!("sources={n} median_ns={} samples_ns={times:?}",times[0]);
}}
