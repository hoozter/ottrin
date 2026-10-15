#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
struct DaemonLaunch {
    config: PathBuf,
    store: PathBuf,
    socket: PathBuf,
    administrator: bool,
}

#[cfg(unix)]
fn daemon_launch(args: &[String]) -> Result<DaemonLaunch, String> {
    if args == ["--administrator-index-daemon"] {
        return Ok(DaemonLaunch {
            config: PathBuf::from("/etc/ottrin-administrator/search.json"),
            store: PathBuf::from("/var/lib/ottrin-administrator/search.db"),
            socket: PathBuf::from("/run/ottrin-administrator/index-v1.sock"),
            administrator: true,
        });
    }
    if !args.is_empty() && args != ["--index-daemon"] {
        return Err(
            "usage: ottrin-indexd [--index-daemon | --administrator-index-daemon]".to_string(),
        );
    }
    Ok(DaemonLaunch {
        config: PathBuf::new(),
        store: ottrin_search::SearchService::search_db_path(),
        socket: ottrin_search::default_daemon_socket_path()?,
        administrator: false,
    })
}

#[cfg(unix)]
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    #[cfg(target_os = "linux")]
    if args == ["--native-package-stop"] {
        if let Err(error) = ottrin_search::SearchDaemonClient::stop_native_package_daemon() {
            eprintln!("ottrin index package cleanup: {error}");
            std::process::exit(1);
        }
        return;
    }
    let launch = daemon_launch(&args).unwrap_or_else(|error| {
        eprintln!("ottrin index daemon: {error}");
        std::process::exit(2);
    });
    if launch.administrator && unsafe { libc::geteuid() } != 0 {
        eprintln!("ottrin index daemon: Administrator index mode requires root");
        std::process::exit(1);
    }
    let config = if launch.administrator {
        std::fs::read(&launch.config)
            .map_err(|error| {
                format!(
                    "could not read protected Administrator index configuration {}: {error}",
                    launch.config.display()
                )
            })
            .and_then(|bytes| {
                serde_json::from_slice::<ottrin_core::SearchConfig>(&bytes).map_err(|error| {
                    format!("invalid protected Administrator index configuration: {error}")
                })
            })
            .unwrap_or_else(|error| {
                eprintln!("ottrin index daemon: {error}");
                std::process::exit(1);
            })
    } else {
        ottrin_ui::load_config().search
    };
    if let Err(error) = ottrin_search::run_search_daemon(config, launch.store, launch.socket) {
        eprintln!("ottrin index daemon: {error}");
        std::process::exit(1);
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, PartialEq, Eq)]
enum WindowsLaunch {
    Serve,
    Stop,
    Transaction(ottrin_search::IndexInstallerPhase, std::path::PathBuf),
}

#[cfg(any(target_os = "windows", test))]
fn windows_launch(args: &[std::ffi::OsString]) -> Result<WindowsLaunch, &'static str> {
    match args {
        [] => Ok(WindowsLaunch::Serve),
        [arg] if arg == "--index-daemon" => Ok(WindowsLaunch::Serve),
        [arg] if arg == "--stop" => Ok(WindowsLaunch::Stop),
        [arg, phase, path] if arg == "--installer-transaction" => {
            let phase = phase.to_str().ok_or("Invalid installer phase")?.parse()?;
            Ok(WindowsLaunch::Transaction(phase, path.into()))
        }
        _ => Err(
            "usage: ottrin-indexd [--index-daemon | --stop | --installer-transaction PHASE SNAPSHOT]",
        ),
    }
}

#[cfg(target_os = "windows")]
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let launch = windows_launch(&args).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    let result = match launch {
        WindowsLaunch::Serve => ottrin_search::run_windows_search_daemon(
            ottrin_ui::load_config().search,
            ottrin_search::SearchService::search_db_path(),
        ),
        WindowsLaunch::Stop => ottrin_search::SearchService::stop_sqlite_schedule().map(|_| ()),
        WindowsLaunch::Transaction(phase, snapshot) => {
            ottrin_search::run_windows_index_installer_transaction(phase, &snapshot)
        }
    };
    if let Err(error) = result {
        eprintln!("ottrin index daemon: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
fn main() {
    eprintln!("ottrin-indexd is currently supported only on Unix platforms");
    std::process::exit(1);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn administrator_mode_uses_only_protected_fixed_paths() {
        let launch = daemon_launch(&["--administrator-index-daemon".to_string()])
            .expect("accepted Administrator daemon mode");

        assert_eq!(
            launch.config,
            std::path::PathBuf::from("/etc/ottrin-administrator/search.json")
        );
        assert_eq!(
            launch.store,
            std::path::PathBuf::from("/var/lib/ottrin-administrator/search.db")
        );
        assert_eq!(
            launch.socket,
            std::path::PathBuf::from("/run/ottrin-administrator/index-v1.sock")
        );
    }
}

#[cfg(test)]
mod windows_launch_tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }
    #[test]
    fn installer_actions_are_explicit_and_preserve_unicode_snapshot_paths() {
        assert_eq!(windows_launch(&[]), Ok(WindowsLaunch::Serve));
        assert_eq!(
            windows_launch(&args(&["--index-daemon"])),
            Ok(WindowsLaunch::Serve)
        );
        assert_eq!(windows_launch(&args(&["--stop"])), Ok(WindowsLaunch::Stop));
        let path = "C:\\Users\\QA Ω\\AppData\\Local\\Temp\\index 'snapshot'.json";
        for phase in ["Prepare", "Snapshot", "Rollback", "Activate", "Commit"] {
            assert_eq!(
                windows_launch(&args(&["--installer-transaction", phase, path])),
                Ok(WindowsLaunch::Transaction(
                    phase.parse().unwrap(),
                    path.into()
                ))
            );
        }
        for invalid in [
            vec!["--index-update"],
            vec!["--installer-transaction", "Prepare"],
            vec!["--installer-transaction", "Delete", path],
            vec!["--stop", "extra"],
        ] {
            assert!(windows_launch(&args(&invalid)).is_err());
        }
    }
}
