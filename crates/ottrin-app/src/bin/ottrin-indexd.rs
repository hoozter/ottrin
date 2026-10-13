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

#[cfg(target_os = "windows")]
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !args.is_empty() && args != ["--index-update"] {
        eprintln!("usage: ottrin-indexd [--index-update]");
        std::process::exit(2);
    }
    if let Err(error) =
        ottrin_search::SearchService::new(ottrin_ui::load_config().search).run_single_update(false)
    {
        eprintln!("ottrin index refresh: {error}");
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
