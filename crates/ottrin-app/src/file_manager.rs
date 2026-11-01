//! Event-driven standard desktop file-manager interface; GUI processes are on demand.
use futures_lite::StreamExt;
use std::{
    collections::HashSet,
    path::PathBuf,
    process::{Child, Command, Stdio},
};
const BUS: &str = "org.freedesktop.FileManager1";
const OBJECT: &str = "/org/freedesktop/FileManager1";
#[derive(Clone, Copy)]
enum Method {
    Folders,
    Items,
    Properties,
}
struct FileManager;
#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl FileManager {
    async fn show_folders(&self, uris: Vec<String>, startup_id: String) -> zbus::fdo::Result<()> {
        launch(Method::Folders, uris, startup_id).await
    }
    async fn show_items(&self, uris: Vec<String>, startup_id: String) -> zbus::fdo::Result<()> {
        launch(Method::Items, uris, startup_id).await
    }
    async fn show_item_properties(
        &self,
        uris: Vec<String>,
        startup_id: String,
    ) -> zbus::fdo::Result<()> {
        launch(Method::Properties, uris, startup_id).await
    }
}
fn paths(method: Method, uris: &[String]) -> Result<Vec<PathBuf>, String> {
    if uris.is_empty() {
        return Err("At least one local file URI is required".into());
    }
    let mut seen = HashSet::new();
    uris.iter()
        .map(|uri| {
            let path = ottrin_platform::local_path_from_uri(uri)?;
            let path = match method {
                Method::Folders => {
                    let resolved = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
                    if !resolved.is_dir() {
                        return Err("ShowFolders requires directories".into());
                    }
                    resolved
                }
                Method::Items => crate::resolve_select_path(&path)?,
                Method::Properties => {
                    std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                    path
                }
            };
            Ok(path)
        })
        .filter(|result| match result {
            Ok(path) => seen.insert(path.clone()),
            Err(_) => true,
        })
        .collect()
}
async fn launch(method: Method, uris: Vec<String>, startup_id: String) -> zbus::fdo::Result<()> {
    if startup_id.contains('\0') {
        return Err(zbus::fdo::Error::InvalidArgs(
            "Invalid startup identifier".into(),
        ));
    }
    let (sent, received) = async_channel::bounded(1);
    std::thread::Builder::new()
        .name("file-manager-launch".into())
        .spawn(move || {
            let mut children: Vec<Child> = Vec::new();
            let result = (|| {
                let paths = paths(method, &uris).map_err(zbus::fdo::Error::InvalidArgs)?;
                let exe =
                    ottrin_platform::application_launch_path().map_err(zbus::fdo::Error::Failed)?;
                let groups = if matches!(method, Method::Properties) {
                    vec![paths]
                } else {
                    paths.into_iter().map(|path| vec![path]).collect()
                };
                for group in groups {
                    let mut cmd = Command::new(&exe);
                    match method {
                        Method::Items => {
                            cmd.arg("--select");
                        }
                        Method::Properties => {
                            cmd.arg("--properties");
                        }
                        Method::Folders => {}
                    }
                    cmd.arg("--")
                        .args(group)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null());
                    cmd.env_remove("DESKTOP_STARTUP_ID")
                        .env_remove("XDG_ACTIVATION_TOKEN");
                    if !startup_id.is_empty() {
                        cmd.env("DESKTOP_STARTUP_ID", &startup_id);
                    }
                    children.push(cmd.spawn().map_err(|e| {
                        zbus::fdo::Error::Failed(format!("Cannot open Ottrin: {e}"))
                    })?);
                }
                Ok(())
            })();
            // Reply after validation/spawn, not after the user closes the windows. Always reap.
            let _ = sent.send_blocking(result);
            for mut child in children {
                if let Err(error) = child.wait() {
                    eprintln!("ottrin file manager: cannot reap window: {error}");
                }
            }
        })
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
    received
        .recv()
        .await
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?
}
struct Control {
    stopped: async_channel::Sender<()>,
}
#[zbus::interface(name = "org.ottrin.FileManager")]
impl Control {
    async fn stop(&self) {
        let _ = self.stopped.try_send(());
    }
}

pub(super) fn run() -> Result<(), String> {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return Err("Run the file-manager interface in the normal user session".into());
    }
    // SAFETY: fixed NUL-terminated process label, borrowed only for prctl.
    if unsafe { libc::prctl(libc::PR_SET_NAME, c"ottrin-filemgr".as_ptr(), 0, 0, 0) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    async_io::block_on(async {
        let (stopped, stop) = async_channel::bounded(1);
        let connection = zbus::connection::Builder::session()
            .map_err(|e| e.to_string())?
            .serve_at(OBJECT, Control { stopped })
            .map_err(|e| e.to_string())?
            .serve_at(OBJECT, FileManager)
            .map_err(|e| e.to_string())?
            .build()
            .await
            .map_err(|e| e.to_string())?;
        let proxy = zbus::fdo::DBusProxy::new(&connection)
            .await
            .map_err(|e| e.to_string())?;
        let mut changes = proxy
            .receive_name_owner_changed_with_args(&[(0, BUS)])
            .await
            .map_err(|e| e.to_string())?;
        let reply = connection
            .request_name_with_flags(BUS, zbus::fdo::RequestNameFlags::DoNotQueue.into())
            .await
            .map_err(|e| e.to_string())?;
        if !matches!(
            reply,
            zbus::fdo::RequestNameReply::PrimaryOwner | zbus::fdo::RequestNameReply::AlreadyOwner
        ) {
            return Err(
                "Another file manager owns the desktop interface; it has been left running".into(),
            );
        }
        let owner = connection
            .unique_name()
            .ok_or("Session bus did not assign a name")?
            .to_string();
        loop {
            let signal = futures_lite::future::race(async { changes.next().await }, async {
                let _ = stop.recv().await;
                None
            })
            .await;
            let Some(signal) = signal else {
                break;
            };
            let args = signal.args().map_err(|e| e.to_string())?;
            if args
                .old_owner()
                .as_ref()
                .is_some_and(|old| old.as_str() == owner)
            {
                break;
            }
        }
        drop(changes);
        drop(proxy);
        connection.graceful_shutdown().await;
        Ok(())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn methods_validate_the_entire_native_batch_before_launching() {
        use std::os::unix::{ffi::OsStringExt, fs::symlink};
        let root = tempfile::tempdir().unwrap();
        let link = root
            .path()
            .join(std::ffi::OsString::from_vec(b"link\x80".to_vec()));
        symlink("missing", &link).unwrap();
        let uri = ottrin_platform::local_file_uri(&link).unwrap();
        assert_eq!(
            paths(Method::Items, &[uri.clone(), uri.clone()]).unwrap(),
            vec![link.clone()]
        );
        assert_eq!(
            paths(Method::Properties, std::slice::from_ref(&uri)).unwrap(),
            vec![link]
        );
        assert!(paths(Method::Folders, std::slice::from_ref(&uri)).is_err());
        assert!(paths(Method::Items, &[uri, "https://example.com/a".into()]).is_err());
        assert!(paths(Method::Properties, &[]).is_err());
    }
}
