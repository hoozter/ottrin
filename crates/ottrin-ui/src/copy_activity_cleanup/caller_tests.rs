use super::*;
use crate::privileged_copy_activity::test_support::ProbePlatform;

fn selections() -> Vec<Selection> {
    vec![
        Selection {
            transactions: vec!["a".repeat(32)],
            privileged: false,
        },
        Selection {
            transactions: vec!["b".repeat(32)],
            privileged: true,
        },
    ]
}

#[test]
fn mixed_cleanup_uses_one_authenticated_command_for_the_complete_selection() {
    let platform = ProbePlatform::new(PrivilegedStatus::Success);
    let slot = Arc::new(Mutex::new(None));
    let outcome = run(
        &selections(),
        &platform,
        &slot,
        &AtomicBool::new(false),
        None,
        &mut |_| {},
    );
    assert!(outcome.error.is_none());
    assert_eq!(
        outcome.completed,
        HashSet::from(["a".repeat(32), "b".repeat(32)])
    );
    assert!(slot.lock().unwrap().is_none());
    let commands = platform.commands.lock().unwrap();
    assert!(
        matches!(commands.as_slice(), [PrivilegedCommand::CleanupCopy { transactions }]
        if transactions == &vec!["a".repeat(32), "b".repeat(32)])
    );
}

#[test]
fn stopped_or_refused_mixed_cleanup_keeps_the_entire_selection_owned() {
    for stopped in [false, true] {
        let platform = ProbePlatform::new(PrivilegedStatus::Failed);
        let slot = Arc::new(Mutex::new(None));
        let outcome = run(
            &selections(),
            &platform,
            &slot,
            &AtomicBool::new(stopped),
            None,
            &mut |_| {},
        );
        assert!(outcome.error.is_some());
        assert!(outcome.completed.is_empty());
        assert!(slot.lock().unwrap().is_none());
        assert_eq!(
            platform.commands.lock().unwrap().len(),
            usize::from(!stopped)
        );
    }
}
