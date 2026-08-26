#![cfg(target_os = "windows")]

use ottrin_preview::{IsolatedProcessLauncher, WindowsJobObjectIsolation};

#[test]
fn windows_office_boundary_is_an_explicit_launcher_capability() {
    fn assert_launcher<T: IsolatedProcessLauncher + Copy + Default>() {}

    assert_launcher::<WindowsJobObjectIsolation>();
}
