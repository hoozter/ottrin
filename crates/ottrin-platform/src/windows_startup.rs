//! Package-owned startup follows MSIX updates and is removed with the package.
//! A normal scheduled task cannot safely name a versioned WindowsApps executable.
use windows::ApplicationModel::{StartupTask, StartupTaskState};
use windows::Win32::Foundation::{APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER};
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::core::{HSTRING, PWSTR};

const INDEX_TASK_ID: &str = "OttrinIndex";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupState {
    Disabled,
    DisabledByUser,
    Enabled,
    DisabledByPolicy,
    EnabledByPolicy,
}

impl StartupState {
    pub fn enabled(self) -> bool {
        matches!(self, Self::Enabled | Self::EnabledByPolicy)
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled at login",
            Self::DisabledByUser => {
                "Disabled in Windows Startup apps; enable Ottrin there to restore automatic indexing"
            }
            Self::Enabled => "Enabled at login",
            Self::DisabledByPolicy => "Disabled by Windows policy",
            Self::EnabledByPolicy => "Enabled by Windows policy",
        }
    }

    fn needs_enable(self) -> Result<bool, String> {
        match self {
            Self::Disabled => Ok(true),
            Self::Enabled | Self::EnabledByPolicy => Ok(false),
            Self::DisabledByUser | Self::DisabledByPolicy => Err(self.detail().into()),
        }
    }

    fn needs_disable(self) -> Result<bool, String> {
        match self {
            Self::Enabled => Ok(true),
            Self::EnabledByPolicy => {
                Err("Windows policy requires automatic indexing at login".into())
            }
            Self::Disabled | Self::DisabledByUser | Self::DisabledByPolicy => Ok(false),
        }
    }

    fn from_native(state: StartupTaskState) -> Result<Self, String> {
        match state {
            StartupTaskState::Disabled => Ok(Self::Disabled),
            StartupTaskState::DisabledByUser => Ok(Self::DisabledByUser),
            StartupTaskState::Enabled => Ok(Self::Enabled),
            StartupTaskState::DisabledByPolicy => Ok(Self::DisabledByPolicy),
            StartupTaskState::EnabledByPolicy => Ok(Self::EnabledByPolicy),
            _ => Err(format!(
                "Windows returned an unknown startup state: {}",
                state.0
            )),
        }
    }
}

/// Identity is authoritative: an inspection error must not fall back to creating
/// an unpackaged task with an unrelated lifetime or activation context.
pub fn is_packaged() -> Result<bool, String> {
    let mut length = 0;
    let result = unsafe { GetCurrentPackageFullName(&mut length, PWSTR::null()) };
    match result {
        APPMODEL_ERROR_NO_PACKAGE => Ok(false),
        ERROR_INSUFFICIENT_BUFFER if length > 0 => Ok(true),
        other => Err(format!(
            "Cannot inspect the Windows package identity: {}",
            other.0
        )),
    }
}

fn with_index_task<T: Send + 'static>(
    action: impl FnOnce(StartupTask) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    // Keep blocking completion off the UI apartment. The windows binding owns
    // the process-lived MTA used by its cached activation factories. Explicitly
    // closing WinRT after each call unloads the cached factory's implementation
    // and leaves the next call with a dangling interface pointer.
    std::thread::Builder::new()
        .name("ottrin-startup".into())
        .spawn(move || {
            let task = StartupTask::GetAsync(&HSTRING::from(INDEX_TASK_ID))
                .and_then(|operation| operation.get())
                .map_err(|error| {
                    format!("Cannot inspect the package's index startup task: {error}")
                })?;
            action(task)
        })
        .map_err(|error| format!("Cannot start Windows startup inspection: {error}"))?
        .join()
        .map_err(|_| "Windows startup inspection failed unexpectedly".to_string())?
}

pub fn index_state() -> Result<StartupState, String> {
    with_index_task(|task| {
        StartupState::from_native(
            task.State()
                .map_err(|error| format!("Cannot read Windows startup state: {error}"))?,
        )
    })
}

pub fn enable_index() -> Result<(), String> {
    with_index_task(|task| {
        let state = StartupState::from_native(
            task.State()
                .map_err(|error| format!("Cannot read Windows startup state: {error}"))?,
        )?;
        if !state.needs_enable()? {
            return Ok(());
        }
        let state = task
            .RequestEnableAsync()
            .and_then(|operation| operation.get())
            .map_err(|error| format!("Cannot enable automatic indexing at login: {error}"))?;
        let state = StartupState::from_native(state)?;
        if state.enabled() {
            Ok(())
        } else {
            Err(format!(
                "Automatic indexing was not enabled: {}",
                state.detail()
            ))
        }
    })
}

pub fn disable_index() -> Result<(), String> {
    with_index_task(|task| {
        let state = StartupState::from_native(
            task.State()
                .map_err(|error| format!("Cannot read Windows startup state: {error}"))?,
        )?;
        if !state.needs_disable()? {
            return Ok(());
        }
        task.Disable()
            .map_err(|error| format!("Cannot disable automatic indexing at login: {error}"))
    })
}

pub fn open_settings() -> Result<String, String> {
    opener::open("ms-settings:startupapps")
        .map_err(|error| format!("Cannot open Windows Startup apps settings: {error}"))?;
    Ok("Opened Windows Startup apps settings.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabling_cannot_override_windows_user_or_policy_choices() {
        for native in [
            StartupTaskState::DisabledByUser,
            StartupTaskState::DisabledByPolicy,
        ] {
            let state = StartupState::from_native(native).unwrap();
            assert!(state.needs_enable().is_err());
            assert!(!state.enabled());
            assert!(!state.needs_disable().unwrap());
        }
        assert!(!StartupState::EnabledByPolicy.needs_enable().unwrap());
        assert!(StartupState::EnabledByPolicy.needs_disable().is_err());
        assert!(StartupState::Disabled.needs_enable().unwrap());
        assert!(StartupState::from_native(StartupTaskState(55)).is_err());
    }
}
