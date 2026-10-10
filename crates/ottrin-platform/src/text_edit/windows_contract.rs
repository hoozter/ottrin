//! Publication ordering shared by the native boundary and portable fault tests.
use super::TextEditError;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    VerifySource,
    RetainSource,
    VerifyRetained,
    FlushRetained,
    Publish,
    VerifyPublished,
    FlushPublished,
    RestoreVacant,
}

/// Staging is already complete and its independent draft has been flushed.
/// No step removes a file. Both renames and restoration must refuse replacement.
pub(super) fn publish(
    recovery: &Path,
    mut run: impl FnMut(Step) -> Result<(), TextEditError>,
) -> Result<(), TextEditError> {
    run(Step::VerifySource).map_err(|error| {
        TextEditError::StageRetentionRequired(format!(
            "{error}; recovery directory: {recovery:?}; all staging is retained"
        ))
    })?;
    for step in [
        // Once a mutation is attempted, an I/O error must not imply that the
        // namespace is unchanged. Require explicit reload even on claim failure.
        Step::RetainSource,
        Step::VerifyRetained,
        Step::FlushRetained,
        Step::Publish,
        Step::VerifyPublished,
        Step::FlushPublished,
    ] {
        if let Err(error) = run(step) {
            // A retained hard link restores only a vacant name. Even after
            // publication, never overwrite an occupant or unlink recovery.
            let restoration = run(Step::RestoreVacant);
            return Err(TextEditError::RecoveryConflict {
                recovery: recovery.to_path_buf(),
                detail: format!(
                    "Windows save stopped at {step:?}: {error}; vacant-name restoration: {restoration:?}; all retained versions must be inspected"
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_withdrawal_error_requires_reload_even_if_namespace_outcome_is_uncertain() {
        let result = publish(Path::new("recovery"), |step| {
            if step == Step::RetainSource {
                Err(TextEditError::Write(
                    "uncertain native rename outcome".into(),
                ))
            } else {
                Ok(())
            }
        });
        assert!(matches!(
            result,
            Err(TextEditError::RecoveryConflict { .. })
        ));
    }

    #[test]
    fn windows_publication_retains_and_flushes_before_publishing() {
        let mut calls = Vec::new();
        publish(Path::new("recovery"), |step| {
            calls.push(step);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            calls,
            [
                Step::VerifySource,
                Step::RetainSource,
                Step::VerifyRetained,
                Step::FlushRetained,
                Step::Publish,
                Step::VerifyPublished,
                Step::FlushPublished
            ]
        );
    }

    #[test]
    fn windows_failure_at_every_boundary_preserves_recovery_and_stops() {
        let steps = [
            Step::VerifySource,
            Step::RetainSource,
            Step::VerifyRetained,
            Step::FlushRetained,
            Step::Publish,
            Step::VerifyPublished,
            Step::FlushPublished,
        ];
        for (index, failed) in steps.iter().enumerate() {
            for restoration_fails in [false, true] {
                let mut calls = Vec::new();
                let result = publish(Path::new("exact recovery"), |step| {
                    calls.push(step);
                    if step == *failed || (step == Step::RestoreVacant && restoration_fails) {
                        Err(TextEditError::Write("injected failure".into()))
                    } else {
                        Ok(())
                    }
                });
                assert_eq!(&calls[..=index], &steps[..=index]);
                if index == 0 {
                    assert!(matches!(
                        result,
                        Err(TextEditError::StageRetentionRequired(_))
                    ));
                    assert_eq!(calls.len(), index + 1);
                } else {
                    assert!(
                        matches!(result, Err(TextEditError::RecoveryConflict { recovery, .. }) if recovery == Path::new("exact recovery"))
                    );
                    assert_eq!(calls.last(), Some(&Step::RestoreVacant));
                    assert_eq!(calls.len(), index + 2);
                }
            }
        }
    }

    #[test]
    fn windows_collision_keeps_external_target_and_retained_original() {
        // Model namespace entries separately from file identities: withdrawing
        // the original must not authorize overwriting a later target occupant.
        let mut target = Some("original");
        let mut retained = None;
        let result = publish(Path::new("recovery"), |step| {
            match step {
                Step::RetainSource => retained = target.take(),
                Step::Publish => {
                    target = Some("external");
                    return Err(TextEditError::Write("name occupied".into()));
                }
                Step::RestoreVacant if target.is_some() => {
                    return Err(TextEditError::Write("restoration refuses occupant".into()));
                }
                Step::RestoreVacant => target = retained,
                _ => {}
            }
            Ok(())
        });
        assert!(matches!(
            result,
            Err(TextEditError::RecoveryConflict { .. })
        ));
        assert_eq!(target, Some("external"));
        assert_eq!(retained, Some("original"));
    }
}
