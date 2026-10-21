use super::*;
use crate::test_oplock::Oplock;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;

pub(super) enum Recovery {
    Forward(discovery::RetainedCopyResumePlan),
    Cleanup(Box<RetainedCopyCleanupPlan>),
    SessionUndo {
        receipt: Box<crate::PathMoveUndoReceipt>,
        complete: bool,
    },
}
impl Recovery {
    pub(super) fn run(
        &mut self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        match self {
            Self::Forward(plan) => plan.resume(progress),
            Self::Cleanup(plan) => plan.reclaim(progress),
            Self::SessionUndo { receipt, complete } => {
                crate::PlatformOps::undo_copy_with_receipt(
                    &crate::DefaultPlatform,
                    receipt,
                    &mut progress,
                )?;
                *complete = true;
                Ok(())
            }
        }
    }
    pub(super) fn complete(&self) -> bool {
        match self {
            Self::Forward(plan) => plan.complete(),
            Self::Cleanup(plan) => plan.complete(),
            Self::SessionUndo { complete, .. } => *complete,
        }
    }
}
#[test]
fn windows_native_copy_oplock_cancellation_keeps_recovery_bytes_and_retries_source_free() {
    blocked_cases("forward");
}
#[test]
fn windows_native_copy_oplock_cancellation_keeps_cleanup_bytes_and_retries_source_free() {
    blocked_cases("cleanup");
}
#[test]
fn windows_native_copy_oplock_cancellation_keeps_session_undo_bytes_and_retries_source_free() {
    blocked_cases("session-undo");
}
fn blocked_cases(operation: &str) {
    for blocked in ["central", "local", "payload"] {
        let root = tempfile::tempdir().unwrap();
        let registry = root.path().join("central");
        let destination = root.path().join("destination");
        let (name, candidate, _cleanup) =
            staged_resume_fixture_at(root.path(), &destination, true, false, Some(&registry));
        remove_undo_fixture_source(root.path());
        let local = destination.join(&name).join(LOCAL);
        let header = decode(&std::fs::read(&local).unwrap())
            .unwrap()
            .state
            .header
            .unwrap();
        let central = registry.join(header.central_name());
        let payload = if operation == "session-undo" {
            destination.join("copy/nested/file")
        } else {
            destination.join(&candidate).join("nested/file")
        };
        std::fs::write(destination.join("foreign"), b"keep unrelated data").unwrap();
        let mut plan = if operation != "forward" {
            let parent = owner(&destination);
            let central = owner(&registry);
            let mut publication =
                PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            publication.apply(|_| Ok(())).unwrap();
            drop(publication);
            if operation == "cleanup" {
                undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
                drop((parent, central));
                Recovery::Cleanup(Box::new(
                    RetainedCopyCleanupPlan::observe_at(
                        &registry,
                        std::slice::from_ref(&header.transaction),
                        |_| Ok(()),
                    )
                    .unwrap(),
                ))
            } else {
                Recovery::SessionUndo {
                    receipt: Box::new(
                        discovery::session_receipt(&parent, &central, &name, |_| Ok(())).unwrap(),
                    ),
                    complete: false,
                }
            }
        } else {
            Recovery::Forward(
                discovery::RetainedCopyResumePlan::admit(
                    &registry,
                    &header.transaction,
                    |_| Ok(()),
                )
                .unwrap(),
            )
        };
        let before_local = std::fs::read(&local).unwrap();
        let before_central = std::fs::read(&central).unwrap();
        let before_payload = std::fs::read(&payload).unwrap();
        let mut lock = Oplock::request(match blocked {
            "central" => &central,
            "local" => &local,
            _ => &payload,
        })
        .unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancelled.clone();
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let guard = crate::NativeIoCancellation::start(worker_cancel.clone()).unwrap();
            let result = plan.run(|_| {
                if worker_cancel.load(Ordering::Acquire) {
                    Err(io("stopped between native calls"))
                } else {
                    Ok(())
                }
            });
            let finish = guard.finish();
            tx.send((plan, result, finish)).unwrap();
        });
        let broken = lock.wait_break();
        let pending = rx.recv_timeout(Duration::from_millis(100));
        let start = Instant::now();
        cancelled.store(true, Ordering::Release);
        let stopped = rx.recv_timeout(Duration::from_secs(3));
        let elapsed = start.elapsed();
        // Unblock the worker even if a cancellation assertion will fail. The
        // overlapped request is completed before its buffers can be freed.
        let close = lock.close();
        let joined = worker.join();
        close.unwrap();
        joined.unwrap();
        assert_eq!(broken, WAIT_OBJECT_0, "{blocked}: no real oplock break");
        assert!(
            matches!(pending, Err(mpsc::RecvTimeoutError::Timeout)),
            "{blocked}: operation was not blocked"
        );
        let (mut plan, result, finish) = stopped.unwrap();
        finish.unwrap();
        assert!(!plan.complete());
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("995"),
            "{blocked}: expected native operation-aborted, got {error}"
        );
        println!(
            "oplock cancellation: operation={operation} target={blocked} elapsed_ms={}",
            elapsed.as_millis()
        );
        assert_eq!(std::fs::read(&local).unwrap(), before_local);
        assert_eq!(std::fs::read(&central).unwrap(), before_central);
        assert_eq!(std::fs::read(&payload).unwrap(), before_payload);
        assert_eq!(
            destination.join("copy").exists(),
            operation == "session-undo"
        );
        let plan = std::thread::spawn(move || {
            let guard =
                crate::NativeIoCancellation::start(Arc::new(AtomicBool::new(false))).unwrap();
            let result = plan.run(|_| Ok(()));
            guard.finish().unwrap();
            result.unwrap();
            plan
        })
        .join()
        .unwrap();
        assert!(plan.complete());
        if operation == "cleanup" {
            assert!(!payload.exists());
            assert!(!local.exists());
            assert!(discover_at(&registry).unwrap().is_empty());
        } else if operation == "session-undo" {
            assert!(!destination.join("copy").exists());
            assert_eq!(
                std::fs::read(destination.join(&candidate).join("nested/file")).unwrap(),
                before_payload
            );
        } else {
            assert_eq!(
                std::fs::read(destination.join("copy/nested/file")).unwrap(),
                before_payload
            );
        }
        assert_eq!(
            std::fs::read(destination.join("foreign")).unwrap(),
            b"keep unrelated data"
        );
        assert!(!root.path().join("source").exists());
    }
}
