//! Minimal cooperating CLI caller; native paths use the shared Rust contract.
//! Run UI applications' blocking I/O on their own worker, not their event thread.
use ottrin_core::picker::{PickerMode, PickerRequest, PickerResponse};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

struct Call {
    child: Child,
    lease: Option<ChildStdin>,
}
impl Drop for Call {
    fn drop(&mut self) {
        // EOF requests cancellation. Give the chooser a bounded chance to stop
        // its own workers before terminating only this owned child on failure.
        self.lease.take();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => {
                    match self.child.kill() {
                        Ok(()) => {
                            if let Err(error) = self.child.wait() {
                                eprintln!("picker_client: cannot reap owned chooser: {error}");
                            }
                        }
                        Err(error) => {
                            eprintln!("picker_client: cannot stop owned chooser: {error}")
                        }
                    }
                    return;
                }
            }
        }
    }
}
fn choose(application: PathBuf, request: &PickerRequest) -> Result<PickerResponse, String> {
    request.validate()?;
    let payload = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    if payload.len() > 128 * 1024 {
        return Err("Picker request exceeds 128 KiB".into());
    }
    let mut child = Command::new(application)
        .args(["picker", "--cancel-on-disconnect"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| e.to_string())?;
    let lease = child.stdin.take();
    let mut call = Call { child, lease };
    let stdin = call.lease.as_mut().ok_or("Missing caller lease")?;
    stdin.write_all(&payload).map_err(|e| e.to_string())?;
    stdin.write_all(b"\n").map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())?;
    // Keep stdin open while awaiting the response: communicate()/wait_with_output()
    // would close it early and cancel the request instead of preserving the lease.
    const LIMIT: u64 = 16 * 1024 * 1024;
    let mut response = Vec::new();
    call.child
        .stdout
        .take()
        .ok_or("Missing picker output")?
        .take(LIMIT + 1)
        .read_to_end(&mut response)
        .map_err(|e| e.to_string())?;
    if response.len() as u64 > LIMIT {
        return Err("Picker response exceeds 16 MiB".into());
    }
    let status = call.child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("Picker failed ({status}); see its diagnostics"));
    }
    let response: PickerResponse = serde_json::from_slice(&response).map_err(|e| e.to_string())?;
    response.validate_for(request)?;
    Ok(response)
}
fn run() -> Result<(), String> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let [application, mode, directory, rest @ ..] = arguments.as_slice() else {
        return Err(
            "Usage: picker_client APP open|save|folder DIRECTORY [FILENAME [PARENT]]".into(),
        );
    };
    if rest.len() > 2 {
        return Err("Too many arguments".into());
    }
    let mode = match mode.to_str() {
        Some("open") => PickerMode::Open,
        Some("save") => PickerMode::Save,
        Some("folder") => PickerMode::Folder,
        _ => return Err("Mode must be open, save or folder".into()),
    };
    let request = PickerRequest {
        mode,
        initial_directory: Some(directory.into()),
        filename: rest.first().cloned().unwrap_or_default(),
        parent_window: rest
            .get(1)
            .map(|value| value.to_str().ok_or("Parent identifier must be textual"))
            .transpose()?
            .unwrap_or_default()
            .into(),
        ..Default::default()
    };
    let response = choose(application.into(), &request)?;
    println!(
        "{}",
        serde_json::to_string(&response).map_err(|e| e.to_string())?
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("picker_client: {error}");
        std::process::exit(1);
    }
}
