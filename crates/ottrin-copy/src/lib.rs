use ottrin_core::FileCommand;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictPolicy {
    Skip,
    Overwrite,
    Rename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Conflict,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferJob {
    pub id: u64,
    pub command: FileCommand,
    pub status: JobStatus,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub destination: PathBuf,
    pub started_at_unix_secs: Option<u64>,
    pub finished_at_unix_secs: Option<u64>,
    pub speed_bytes_per_sec: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Error)]
pub enum QueueError {
    #[error("invalid command for transfer queue")]
    InvalidCommand,
    #[error("transfer job not found: {0}")]
    JobNotFound(u64),
}

#[derive(Debug)]
pub struct TransferQueue {
    next_id: u64,
    pub default_policy: ConflictPolicy,
    pub jobs: VecDeque<TransferJob>,
}

impl Default for TransferQueue {
    fn default() -> Self {
        Self {
            next_id: 1,
            default_policy: ConflictPolicy::Rename,
            jobs: VecDeque::new(),
        }
    }
}

impl TransferQueue {
    pub fn set_default_policy(&mut self, policy: ConflictPolicy) {
        self.default_policy = policy;
    }

    pub fn enqueue(&mut self, command: FileCommand, destination: PathBuf) -> Result<u64, QueueError> {
        match command {
            FileCommand::Copy { .. } | FileCommand::Move { .. } => {
                let id = self.next_id;
                self.next_id += 1;
                self.jobs.push_back(TransferJob {
                    id,
                    command,
                    status: JobStatus::Pending,
                    bytes_done: 0,
                    bytes_total: 0,
                    destination,
                    started_at_unix_secs: None,
                    finished_at_unix_secs: None,
                    speed_bytes_per_sec: None,
                    eta_seconds: None,
                    last_error: None,
                });
                Ok(id)
            }
            _ => Err(QueueError::InvalidCommand),
        }
    }

    pub fn mark_running(&mut self, id: u64) -> Result<(), QueueError> {
        let job = self
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or(QueueError::JobNotFound(id))?;
        job.status = JobStatus::Running;
        job.started_at_unix_secs = Some(now_unix_secs());
        job.finished_at_unix_secs = None;
        job.last_error = None;
        Ok(())
    }

    pub fn set_expected_bytes(&mut self, id: u64, bytes_total: u64) -> Result<(), QueueError> {
        let job = self
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or(QueueError::JobNotFound(id))?;
        job.bytes_total = bytes_total;
        job.eta_seconds = None;
        Ok(())
    }

    pub fn mark_completed(&mut self, id: u64, bytes_done: u64) -> Result<(), QueueError> {
        let job = self
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or(QueueError::JobNotFound(id))?;
        job.status = JobStatus::Completed;
        job.bytes_done = bytes_done;
        if job.bytes_total == 0 {
            job.bytes_total = bytes_done;
        }
        let finished = now_unix_secs();
        job.finished_at_unix_secs = Some(finished);
        if let Some(started) = job.started_at_unix_secs {
            let elapsed = finished.saturating_sub(started).max(1);
            let speed = bytes_done / elapsed;
            job.speed_bytes_per_sec = Some(speed);
            job.eta_seconds = Some(0);
        }
        job.last_error = None;
        Ok(())
    }

    pub fn mark_failed(&mut self, id: u64, message: String) -> Result<(), QueueError> {
        let job = self
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or(QueueError::JobNotFound(id))?;
        job.status = JobStatus::Failed;
        job.finished_at_unix_secs = Some(now_unix_secs());
        job.last_error = Some(message);
        Ok(())
    }

    pub fn cancel_pending(&mut self) -> usize {
        let mut canceled = 0usize;
        for job in &mut self.jobs {
            if job.status == JobStatus::Pending {
                job.status = JobStatus::Canceled;
                canceled += 1;
            }
        }
        canceled
    }

    pub fn failed_jobs(&self) -> Vec<(u64, FileCommand)> {
        self.jobs
            .iter()
            .filter(|job| job.status == JobStatus::Failed)
            .map(|job| (job.id, job.command.clone()))
            .collect()
    }

    pub fn clear_completed(&mut self) -> usize {
        let before = self.jobs.len();
        self.jobs
            .retain(|job| job.status != JobStatus::Completed && job.status != JobStatus::Canceled);
        before.saturating_sub(self.jobs.len())
    }

    pub fn mark_retry_pending(&mut self, id: u64) -> Result<(), QueueError> {
        let job = self
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or(QueueError::JobNotFound(id))?;
        job.status = JobStatus::Pending;
        job.started_at_unix_secs = None;
        job.finished_at_unix_secs = None;
        job.speed_bytes_per_sec = None;
        job.eta_seconds = None;
        job.last_error = None;
        Ok(())
    }
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
