//! The bounded index-daemon wire contract, independent of socket/pipe transport.
use crate::{SearchIndexDiagnostics, SearchQueryResult};
use ottrin_core::{SearchConfig, SearchQuery};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub(super) const PROTOCOL_VERSION: u16 = 1;
pub(super) const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum DaemonRequest {
    Ping,
    Status,
    Query { query: SearchQuery },
    UpdateConfig { config: SearchConfig },
    Reconcile { operation_id: u64 },
    Rebuild { operation_id: u64 },
    ClearStorage,
    Shutdown,
    PrepareUpgrade,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RequestFrame {
    pub(super) version: u16,
    pub(super) request: DaemonRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ResponseFrame {
    pub(super) version: u16,
    pub(super) response: DaemonResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum DaemonResponse {
    Pong,
    Status {
        diagnostics: Box<SearchIndexDiagnostics>,
    },
    Query {
        result: SearchQueryResult,
    },
    Complete,
    OperationComplete {
        operation_id: u64,
    },
    StorageCleared {
        bytes: u64,
    },
    Error {
        code: String,
        message: String,
    },
}

pub(super) fn write_frame<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<(), String> {
    let payload = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(format!("daemon frame exceeds {MAX_FRAME_BYTES} byte limit"));
    }
    let length = u32::try_from(payload.len()).map_err(|_| "daemon frame is too large")?;
    writer
        .write_all(&length.to_be_bytes())
        .and_then(|()| writer.write_all(&payload))
        .and_then(|()| writer.flush())
        .map_err(|error| error.to_string())
}

pub(super) fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T, String> {
    let mut length = [0_u8; 4];
    reader
        .read_exact(&mut length)
        .map_err(|error| error.to_string())?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(format!("daemon frame exceeds {MAX_FRAME_BYTES} byte limit"));
    }
    let mut payload = vec![0_u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&payload).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn daemon_protocol_round_trips_one_bounded_frame() {
        let expected = RequestFrame {
            version: PROTOCOL_VERSION,
            request: DaemonRequest::Ping,
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &expected).expect("write request frame");

        let actual: RequestFrame = read_frame(&mut Cursor::new(bytes)).expect("read request frame");
        assert_eq!(actual, expected);
    }

    #[test]
    fn daemon_protocol_binds_completion_to_the_requested_operation() {
        let expected = RequestFrame {
            version: PROTOCOL_VERSION,
            request: DaemonRequest::Rebuild { operation_id: 41 },
        };
        let mut request_bytes = Vec::new();
        write_frame(&mut request_bytes, &expected).expect("write operation request");
        let actual: RequestFrame =
            read_frame(&mut Cursor::new(request_bytes)).expect("read operation request");
        assert_eq!(actual, expected);

        let response = ResponseFrame {
            version: PROTOCOL_VERSION,
            response: DaemonResponse::OperationComplete { operation_id: 41 },
        };
        let mut response_bytes = Vec::new();
        write_frame(&mut response_bytes, &response).expect("write operation response");
        let actual: ResponseFrame =
            read_frame(&mut Cursor::new(response_bytes)).expect("read operation response");
        assert!(matches!(
            actual.response,
            DaemonResponse::OperationComplete { operation_id: 41 }
        ));
    }

    #[test]
    fn daemon_protocol_rejects_an_oversized_frame_before_reading_its_payload() {
        let mut bytes = Cursor::new(((MAX_FRAME_BYTES as u32) + 1).to_be_bytes().to_vec());
        let error = read_frame::<RequestFrame>(&mut bytes).expect_err("oversized frame must fail");
        assert!(error.contains("exceeds"), "unexpected error: {error}");
        assert_eq!(bytes.position(), 4, "payload must not be read or allocated");
    }
    #[test]
    fn fragmented_transport_preserves_unicode_response() {
        struct Fragmented(Cursor<Vec<u8>>);
        impl Read for Fragmented {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let count = buffer.len().min(3);
                self.0.read(&mut buffer[..count])
            }
        }
        impl Write for Fragmented {
            fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                self.0.write(&buffer[..buffer.len().min(2)])
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.0.flush()
            }
        }
        let mut stream = Fragmented(Cursor::new(Vec::new()));
        write_frame(
            &mut stream,
            &ResponseFrame {
                version: PROTOCOL_VERSION,
                response: DaemonResponse::Error {
                    code: "operation_failed".into(),
                    message: "Åtkomst nekad — 文件".into(),
                },
            },
        )
        .unwrap();
        stream.0.set_position(0);
        let actual: ResponseFrame = read_frame(&mut stream).unwrap();
        assert!(
            matches!(actual.response, DaemonResponse::Error {code, message} if code == "operation_failed" && message == "Åtkomst nekad — 文件")
        );
    }

    #[test]
    fn truncated_and_unknown_requests_are_rejected() {
        for body in [
            b"{\"version\":1}".as_slice(),
            b"{\"version\":1,\"request\":{\"type\":\"unknown\"}}".as_slice(),
        ] {
            let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
            bytes.extend_from_slice(body);
            assert!(read_frame::<RequestFrame>(&mut Cursor::new(bytes)).is_err());
        }
        let mut bytes = 30_u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        assert!(read_frame::<RequestFrame>(&mut Cursor::new(bytes)).is_err());
    }

    #[test]
    fn oversized_response_is_rejected_without_writing_a_header() {
        let mut bytes = Vec::new();
        let error = write_frame(
            &mut bytes,
            &ResponseFrame {
                version: PROTOCOL_VERSION,
                response: DaemonResponse::Error {
                    code: "operation_failed".into(),
                    message: "x".repeat(MAX_FRAME_BYTES),
                },
            },
        )
        .unwrap_err();
        assert!(error.contains("exceeds"));
        assert!(bytes.is_empty());
    }
}
