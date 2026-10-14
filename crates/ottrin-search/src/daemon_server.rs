use crate::daemon_protocol::{
    DaemonRequest, DaemonResponse, PROTOCOL_VERSION, RequestFrame, ResponseFrame, read_frame,
    write_frame,
};
use crate::{SearchHandle, SearchService};
use std::io::{Read, Write};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn serve_frame(
    stream: &mut (impl Read + Write),
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
        dispatch_request(frame.request, service, search, shutdown)
    };
    write_frame(
        stream,
        &ResponseFrame {
            version: PROTOCOL_VERSION,
            response,
        },
    )
}

fn dispatch_request(
    request: DaemonRequest,
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
            return DaemonResponse::Query {
                result: search.query_with_diagnostics(query),
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
