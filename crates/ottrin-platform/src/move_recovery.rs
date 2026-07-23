use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[cfg(target_os = "linux")]
use rustix::fd::OwnedFd;

pub const JOURNAL_SCHEMA_MAJOR: u16 = 1;
pub const JOURNAL_SCHEMA_MINOR: u16 = 0;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativePathEncoding {
    UnixBytes,
    WindowsUtf16Le,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePath {
    encoding: NativePathEncoding,
    units_hex: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct JournalHeader {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawJournalHeader {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum JournalError {
    #[error("journal JSON is invalid: {0}")]
    InvalidJson(String),
    #[error("unsupported journal schema {major}.{minor}")]
    UnsupportedSchema { major: u16, minor: u16 },
    #[error("invalid transaction ID")]
    InvalidTransactionId,
    #[error("journal path encoding does not match this platform")]
    WrongPlatformEncoding,
    #[error("invalid native path units")]
    InvalidNativePathUnits,
    #[error("journal I/O failed: {0}")]
    Io(String),
}

#[cfg(target_os = "linux")]
pub struct LinuxJournalDirectory {
    directory: OwnedFd,
}

#[cfg(target_os = "linux")]
impl LinuxJournalDirectory {
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        use rustix::fs::{CWD, Mode, OFlags, openat};

        let directory = openat(
            CWD,
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| JournalError::Io(error.to_string()))?;
        Ok(Self { directory })
    }

    pub fn replace(&self, header: &JournalHeader) -> Result<(), JournalError> {
        use rustix::fs::{Mode, OFlags, fsync, openat, renameat};
        use std::fs::File;
        use std::io::Write;

        let encoded = serde_json::to_vec(header)
            .map_err(|error| JournalError::InvalidJson(error.to_string()))?;
        let next = openat(
            &self.directory,
            "journal.next",
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|error| JournalError::Io(error.to_string()))?;
        let mut next = File::from(next);
        next.write_all(&encoded)
            .and_then(|()| next.sync_all())
            .map_err(|error| JournalError::Io(error.to_string()))?;
        renameat(&self.directory, "journal.next", &self.directory, "journal")
            .map_err(|error| JournalError::Io(error.to_string()))?;
        fsync(&self.directory).map_err(|error| JournalError::Io(error.to_string()))
    }
}

impl NativePath {
    pub fn encode(path: &Path) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self {
                encoding: NativePathEncoding::UnixBytes,
                units_hex: encode_hex(path.as_os_str().as_bytes()),
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let units_hex = path
                .as_os_str()
                .encode_wide()
                .map(|unit| format!("{unit:04x}"))
                .collect();
            Self {
                encoding: NativePathEncoding::WindowsUtf16Le,
                units_hex,
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self {
                encoding: NativePathEncoding::UnixBytes,
                units_hex: encode_hex(path.to_string_lossy().as_bytes()),
            }
        }
    }

    pub fn decode(&self) -> Result<PathBuf, JournalError> {
        #[cfg(unix)]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            if self.encoding != NativePathEncoding::UnixBytes {
                return Err(JournalError::WrongPlatformEncoding);
            }
            Ok(PathBuf::from(OsString::from_vec(decode_hex(
                &self.units_hex,
            )?)))
        }
        #[cfg(windows)]
        {
            use std::ffi::OsString;
            use std::os::windows::ffi::OsStringExt;
            if self.encoding != NativePathEncoding::WindowsUtf16Le {
                return Err(JournalError::WrongPlatformEncoding);
            }
            if !self.units_hex.len().is_multiple_of(4) {
                return Err(JournalError::InvalidNativePathUnits);
            }
            let units = self
                .units_hex
                .as_bytes()
                .chunks_exact(4)
                .map(decode_u16_hex)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(PathBuf::from(OsString::from_wide(&units)))
        }
        #[cfg(not(any(unix, windows)))]
        {
            if self.encoding != NativePathEncoding::UnixBytes {
                return Err(JournalError::WrongPlatformEncoding);
            }
            let bytes = decode_hex(&self.units_hex)?;
            let text =
                String::from_utf8(bytes).map_err(|_| JournalError::InvalidNativePathUnits)?;
            Ok(PathBuf::from(text))
        }
    }
}

impl JournalHeader {
    pub fn new(transaction_id: [u8; 16]) -> Self {
        Self {
            schema_major: JOURNAL_SCHEMA_MAJOR,
            schema_minor: JOURNAL_SCHEMA_MINOR,
            transaction_id: encode_hex(&transaction_id),
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, JournalError> {
        let raw: RawJournalHeader = serde_json::from_slice(bytes)
            .map_err(|error| JournalError::InvalidJson(error.to_string()))?;
        Self::try_from(raw)
    }
}

impl TryFrom<RawJournalHeader> for JournalHeader {
    type Error = JournalError;

    fn try_from(raw: RawJournalHeader) -> Result<Self, Self::Error> {
        if raw.schema_major != JOURNAL_SCHEMA_MAJOR || raw.schema_minor != JOURNAL_SCHEMA_MINOR {
            return Err(JournalError::UnsupportedSchema {
                major: raw.schema_major,
                minor: raw.schema_minor,
            });
        }
        if raw.transaction_id.len() != 32 || decode_hex(&raw.transaction_id).is_err() {
            return Err(JournalError::InvalidTransactionId);
        }
        Ok(Self {
            schema_major: raw.schema_major,
            schema_minor: raw.schema_minor,
            transaction_id: raw.transaction_id,
        })
    }
}

impl<'de> Deserialize<'de> for JournalHeader {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawJournalHeader::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[usize::from(byte >> 4)] as char);
        encoded.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

fn decode_hex(encoded: &str) -> Result<Vec<u8>, JournalError> {
    if !encoded.len().is_multiple_of(2) {
        return Err(JournalError::InvalidNativePathUnits);
    }
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|digits| {
            let high = decode_nibble(digits[0])?;
            let low = decode_nibble(digits[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

#[cfg(windows)]
fn decode_u16_hex(digits: &[u8]) -> Result<u16, JournalError> {
    let mut value = 0_u16;
    for digit in digits {
        value = (value << 4) | u16::from(decode_nibble(*digit)?);
    }
    Ok(value)
}

fn decode_nibble(digit: u8) -> Result<u8, JournalError> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        _ => Err(JournalError::InvalidNativePathUnits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn native_path_round_trips_non_utf8_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from(OsString::from_vec(b"folder/item\x80.txt".to_vec()));
        let encoded = NativePath::encode(&path);
        assert_eq!(encoded.decode().unwrap(), path);
    }

    #[test]
    fn header_round_trips_and_uses_fixed_width_lowercase_id() {
        let header = JournalHeader::new([0xab; 16]);
        let json = serde_json::to_vec(&header).unwrap();
        let parsed = JournalHeader::parse(&json).unwrap();
        assert_eq!(parsed, header);
        assert_eq!(parsed.transaction_id, "abababababababababababababababab");
    }

    #[test]
    fn unknown_header_fields_are_rejected() {
        let json = br#"{
            "schema_major": 1,
            "schema_minor": 0,
            "transaction_id": "00000000000000000000000000000000",
            "unexpected": true
        }"#;
        assert!(matches!(
            JournalHeader::parse(json),
            Err(JournalError::InvalidJson(_))
        ));
    }

    #[test]
    fn unsupported_major_version_is_rejected() {
        let json = br#"{
            "schema_major": 2,
            "schema_minor": 0,
            "transaction_id": "00000000000000000000000000000000"
        }"#;
        assert_eq!(
            JournalHeader::parse(json),
            Err(JournalError::UnsupportedSchema { major: 2, minor: 0 })
        );
    }

    #[test]
    fn direct_serde_deserialization_enforces_header_invariants() {
        for json in [
            br#"{"schema_major":2,"schema_minor":0,"transaction_id":"00000000000000000000000000000000"}"#.as_slice(),
            br#"{"schema_major":1,"schema_minor":0,"transaction_id":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}"#.as_slice(),
        ] {
            assert!(serde_json::from_slice::<JournalHeader>(json).is_err());
        }
    }

    #[test]
    fn malformed_transaction_id_is_rejected() {
        for transaction_id in [
            "abc",
            "0000000000000000000000000000000g",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            let json = format!(
                r#"{{"schema_major":1,"schema_minor":0,"transaction_id":"{transaction_id}"}}"#
            );
            assert_eq!(
                JournalHeader::parse(json.as_bytes()),
                Err(JournalError::InvalidTransactionId)
            );
        }
    }

    #[test]
    fn malformed_native_path_units_are_rejected() {
        for units_hex in ["0", "0x"] {
            let encoded = NativePath {
                #[cfg(unix)]
                encoding: NativePathEncoding::UnixBytes,
                #[cfg(windows)]
                encoding: NativePathEncoding::WindowsUtf16Le,
                #[cfg(not(any(unix, windows)))]
                encoding: NativePathEncoding::UnixBytes,
                units_hex: units_hex.into(),
            };
            assert_eq!(encoded.decode(), Err(JournalError::InvalidNativePathUnits));
        }
    }

    #[test]
    fn foreign_platform_path_encoding_is_rejected() {
        let encoded = NativePath {
            #[cfg(unix)]
            encoding: NativePathEncoding::WindowsUtf16Le,
            #[cfg(windows)]
            encoding: NativePathEncoding::UnixBytes,
            #[cfg(not(any(unix, windows)))]
            encoding: NativePathEncoding::WindowsUtf16Le,
            units_hex: String::new(),
        };
        assert_eq!(encoded.decode(), Err(JournalError::WrongPlatformEncoding));
    }

    #[test]
    fn unknown_native_path_fields_are_rejected() {
        let json = br#"{
            "encoding": "unix-bytes",
            "units_hex": "00",
            "unexpected": true
        }"#;
        assert!(serde_json::from_slice::<NativePath>(json).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn open_journal_directory_cannot_be_retargeted_by_path_replacement() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-journal-anchor-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let original = root.join("journal-directory");
        let moved = root.join("moved-journal-directory");
        std::fs::create_dir_all(&original).unwrap();

        let directory = LinuxJournalDirectory::open(&original).unwrap();
        std::fs::rename(&original, &moved).unwrap();
        std::fs::create_dir(&original).unwrap();

        let header = JournalHeader::new([0x5a; 16]);
        directory.replace(&header).unwrap();

        let persisted = std::fs::read(moved.join("journal")).unwrap();
        assert_eq!(JournalHeader::parse(&persisted).unwrap(), header);
        assert!(!original.join("journal").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
