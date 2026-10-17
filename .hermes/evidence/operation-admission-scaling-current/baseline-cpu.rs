use std::{path::{Path,PathBuf},time::Instant,hint::black_box};
struct OperationPath {path:PathBuf,writes:bool}
fn path_move_paths_overlap(
    first_before: &Path,
    first_after: &Path,
    second_before: &Path,
    second_after: &Path,
) -> bool {
    [first_before, first_after].into_iter().any(|first| {
        [second_before, second_after]
            .into_iter()
            .any(|second| first.starts_with(second) || second.starts_with(first))
    })
}

fn conflicts(paths:&[OperationPath],requested:&[OperationPath])->bool {
            paths.iter().any(|first| {
                requested.iter().any(|second| {
                    (first.writes || second.writes)
                        && path_move_paths_overlap(
                            &first.path,
                            &first.path,
                            &second.path,
                            &second.path,
                        )
                })
            })
}
fn main(){ for n in [5000usize] {
 let mut held:Vec<_>=(0..n).map(|i|OperationPath{path:PathBuf::from(format!("/held/item-{i}")),writes:true}).collect();
 let mut requested:Vec<_>=(0..n).map(|i|OperationPath{path:PathBuf::from(format!("/requested/item-{i}")),writes:true}).collect();
 held.push(OperationPath{path:PathBuf::from("/first"),writes:true});requested.push(OperationPath{path:PathBuf::from("/second"),writes:true});
 let mut times=Vec::new();for _ in 0..1 {let start=Instant::now();assert!(!black_box(conflicts(black_box(&held),black_box(&requested))));times.push(start.elapsed().as_nanos());}
 requested[0].writes=true;requested[0].path=held[0].path.clone();assert!(conflicts(&held,&requested));times.sort();println!("sources={n} median_ns={} samples_ns={times:?}",times[0]);
}}
