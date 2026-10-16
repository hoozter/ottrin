use crate::daemon_protocol::{
    DaemonRequest, DaemonResponse, PROTOCOL_VERSION, RequestFrame, ResponseFrame, read_frame,
    write_frame,
};
use crate::daemon_transport::{DaemonStream, QueryFinished};
use crate::{SearchHandle, SearchService};
use std::io::Read;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn serve_frame(
    stream: &mut DaemonStream,
    service: &Arc<Mutex<SearchService>>,
    search: &SearchHandle,
    shutdown: &AtomicBool,
) -> Result<(), String> {
    let frame: RequestFrame = read_frame(stream)?;
    let response = if frame.version != PROTOCOL_VERSION {
        DaemonResponse::Error {
            code: "protocol_mismatch".to_string(),
            message: format!(
                "client protocol {}, server protocol {PROTOCOL_VERSION}",
                frame.version
            ),
        }
    } else {
        dispatch_request(frame.request, stream, service, search, shutdown)
    };
    write_frame(
        stream,
        &ResponseFrame {
            version: PROTOCOL_VERSION,
            response,
        },
    )
}

/// A query has exactly the lifetime of its requesting connection. No registry,
/// detached job or cancellation RPC survives an abandoned client.
pub(super) fn with_connected_query(
    stream: &DaemonStream,
    shutdown: &AtomicBool,
    run: impl FnOnce(&AtomicBool) -> crate::SearchQueryResult,
) -> Result<crate::SearchQueryResult, String> {
    let mut monitor = stream.try_clone().map_err(|error| error.to_string())?;
    crate::daemon_transport::monitor_timeout(&mut monitor, std::time::Duration::from_millis(50))
        .map_err(|error| error.to_string())?;
    let cancelled = AtomicBool::new(false);
    let done = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let watcher = std::thread::Builder::new()
            .name("ottrin-query-connection".into())
            .spawn_scoped(scope, || {
                let mut byte = [0];
                while !done.load(Ordering::Acquire) {
                    if shutdown.load(Ordering::Acquire) {
                        cancelled.store(true, Ordering::Release);
                        break;
                    }
                    match monitor.read(&mut byte) {
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::TimedOut
                                    | std::io::ErrorKind::WouldBlock
                                    | std::io::ErrorKind::Interrupted
                            ) => {}
                        // After a complete request frame, clients only read a reply.
                        // EOF, extra bytes or a broken connection end that query.
                        _ => {
                            cancelled.store(true, Ordering::Release);
                            break;
                        }
                    }
                }
            })
            .map_err(|error| format!("Cannot start query monitor: {error}"))?;
        let finished = QueryFinished(&done);
        let result = run(&cancelled);
        drop(finished);
        watcher
            .join()
            .map_err(|_| "Search connection monitor failed".to_string())?;
        Ok(result)
    })
}

fn dispatch_request(
    request: DaemonRequest,
    stream: &mut DaemonStream,
    service: &Arc<Mutex<SearchService>>,
    search: &SearchHandle,
    shutdown: &AtomicBool,
) -> DaemonResponse {
    let result = match request {
        DaemonRequest::Ping => return DaemonResponse::Pong,
        DaemonRequest::Status => {
            return DaemonResponse::Status {
                diagnostics: Box::new(search.diagnostics()),
            };
        }
        DaemonRequest::Query { query } => {
            return match with_connected_query(stream, shutdown, |cancelled| {
                search.query_cancellable_with_diagnostics(query, cancelled)
            }) {
                Ok(result) => DaemonResponse::Query { result },
                Err(message) => DaemonResponse::Error {
                    code: "query_failed".into(),
                    message,
                },
            };
        }
        DaemonRequest::UpdateConfig { config } => {
            service
                .lock()
                .expect("search service")
                .update_config(config);
            Ok(())
        }
        DaemonRequest::Reconcile { operation_id } => {
            return complete_operation(
                operation_id,
                service
                    .lock()
                    .expect("search service")
                    .begin_index_reconcile(),
            );
        }
        DaemonRequest::Rebuild { operation_id } => {
            return complete_operation(
                operation_id,
                service
                    .lock()
                    .expect("search service")
                    .begin_rebuild_index(),
            );
        }
        DaemonRequest::ClearStorage => {
            return match service.lock().expect("search service").clear_storage() {
                Ok(bytes) => DaemonResponse::StorageCleared { bytes },
                Err(message) => DaemonResponse::Error {
                    code: "operation_failed".to_string(),
                    message,
                },
            };
        }
        DaemonRequest::Shutdown => {
            shutdown.store(true, Ordering::Release);
            Ok(())
        }
        DaemonRequest::PrepareUpgrade => {
            shutdown.store(true, Ordering::Release);
            Ok(())
        }
    };
    match result {
        Ok(()) => DaemonResponse::Complete,
        Err(message) => DaemonResponse::Error {
            code: "operation_failed".to_string(),
            message,
        },
    }
}

fn complete_operation(
    operation_id: u64,
    started: Result<std::sync::mpsc::Receiver<Result<(), String>>, String>,
) -> DaemonResponse {
    let result = started.and_then(|completion| {
        completion
            .recv()
            .map_err(|_| "Search worker stopped before operation completed".to_string())?
    });
    match result {
        Ok(()) => DaemonResponse::OperationComplete { operation_id },
        Err(message) => DaemonResponse::Error {
            code: "operation_failed".to_string(),
            message,
        },
    }
}
