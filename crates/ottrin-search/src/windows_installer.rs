//! Transaction control that remains available before/after MSI payload changes.
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexInstallerPhase {
    Prepare,
    Snapshot,
    Rollback,
    Commit,
}

impl FromStr for IndexInstallerPhase {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "Prepare" => Ok(Self::Prepare),
            "Snapshot" => Ok(Self::Snapshot),
            "Rollback" => Ok(Self::Rollback),
            "Commit" => Ok(Self::Commit),
            _ => Err("Unknown index installer phase"),
        }
    }
}

#[cfg(target_os = "windows")]
pub fn run(phase: IndexInstallerPhase, snapshot: &std::path::Path) -> Result<(), String> {
    if !snapshot.is_absolute() {
        return Err("Index transaction snapshot path must be absolute".into());
    }
    let script_phase = match phase {
        IndexInstallerPhase::Prepare | IndexInstallerPhase::Snapshot => "Snapshot",
        IndexInstallerPhase::Rollback => "Rollback",
        IndexInstallerPhase::Commit => "Commit",
    };
    // Parameters are environment data, never interpolated into executable code.
    // The existing task/transaction implementations are embedded once in indexd;
    // no installed script or copied transaction directory is required.
    let script = format!(
        "{}\n& {{\n{}\n}} -Phase $env:OTTRIN_INSTALLER_PHASE -Snapshot $env:OTTRIN_INSTALLER_SNAPSHOT",
        super::WINDOWS_INDEX_IDENTITY_SCRIPT,
        include_str!("../../../packaging/windows/index-task-transaction.ps1"),
    );
    let output = super::internal_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .env("OTTRIN_INSTALLER_PHASE", script_phase)
        .env("OTTRIN_INSTALLER_SNAPSHOT", snapshot)
        .output()
        .map_err(|error| format!("Cannot run index transaction: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Index transaction {phase:?} failed ({}): {} {}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    if phase == IndexInstallerPhase::Prepare {
        // Capture original task/running intent first. Stop without deleting the
        // task, its login/disabled state, configuration or saved index.
        super::SearchService::stop_sqlite_schedule()?;
    }
    Ok(())
}
