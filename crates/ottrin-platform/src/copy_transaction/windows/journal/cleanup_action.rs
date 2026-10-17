//! Configured, handle-free cleanup authority for one confirmed cohort.
use super::super::registry::configured_path;
use super::*;
use std::path::Path;

/// No file handles survive admission. Physical changes use the authoritative
/// group writer, including single-member selections and retained metadata retry.
#[derive(Clone, Debug)]
pub struct RetainedCopyCleanupPlan {
    group: cleanup_group::Observation,
}

impl RetainedCopyCleanupPlan {
    pub fn observe(
        transaction_ids: &[String],
        progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        Self::observe_at(&configured_path()?, transaction_ids, progress)
    }

    pub fn observe_at(
        root: &Path,
        transaction_ids: &[String],
        progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        Ok(Self {
            group: cleanup_group::Observation::observe_at(root, transaction_ids, progress)?,
        })
    }

    pub fn transaction_ids(&self) -> Vec<&str> {
        self.group
            .members
            .iter()
            .map(|member| member.header().transaction.as_str())
            .collect()
    }

    pub fn completed_transaction_ids(&self) -> Vec<&str> {
        self.group
            .members
            .iter()
            .filter_map(|member| match member {
                cleanup_group::Member::Complete(header) => Some(header.transaction.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn complete(&self) -> bool {
        self.group
            .members
            .iter()
            .all(|member| matches!(member, cleanup_group::Member::Complete(_)))
    }

    /// Losing this proof can lose authority to finish a known metadata removal
    /// after the final control disappears. Such retries must retain this object.
    pub fn requires_retained_retry(&self) -> bool {
        self.group.requires_retained_retry()
    }

    pub fn reclaim(
        &mut self,
        progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self.complete() {
            return Ok(());
        }
        self.group
            .reclaim_all(progress)
            .map_err(|error| match error {
                PlatformError::CleanupPending { .. } | PlatformError::CopyCleanupRefused(_) => {
                    error
                }
                error if self.group.started() => PlatformError::CleanupPending {
                    transaction: self.group.registry.display().to_string(),
                    message: format!("{error}; remaining Copy recovery data retained"),
                },
                error => PlatformError::CopyCleanupRefused(error.to_string()),
            })
    }
}
