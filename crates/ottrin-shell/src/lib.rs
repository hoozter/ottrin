//! MSIX-owned Explorer registration and its single-value visibility preference.
//! Windows owns COM registration/update/removal; this library never writes shell
//! verbs, defaults, or receipts into the host's Classes hive.
#[cfg(target_os = "windows")]
mod server;
#[cfg(target_os = "windows")]
mod settings;
#[cfg(target_os = "windows")]
pub use settings::{enabled, package_directory, set_enabled};
