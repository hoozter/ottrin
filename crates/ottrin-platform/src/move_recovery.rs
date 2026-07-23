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

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JournalPhase {
    Prepared,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinuxFileType {
    Regular,
    Directory,
    Symlink,
    Other,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinuxStableIdentity {
    device: u64,
    inode: u64,
    file_type: LinuxFileType,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PreparedRecord {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
    phase: JournalPhase,
    source_parent: NativePath,
    original_name: NativePath,
    source_parent_identity: LinuxStableIdentity,
    source_entry_identity: LinuxStableIdentity,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPreparedRecord {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
    phase: JournalPhase,
    source_parent: NativePath,
    original_name: NativePath,
    source_parent_identity: LinuxStableIdentity,
    source_entry_identity: LinuxStableIdentity,
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
    #[error("invalid prepared source path")]
    InvalidPreparedSourcePath,
    #[error("invalid Linux stable identity")]
    InvalidLinuxIdentity,
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
        self.replace_named("journal", header)
    }

    pub fn persist_discovery_header(&self, header: &JournalHeader) -> Result<(), JournalError> {
        self.replace_named(&format!("{}.journal", header.transaction_id), header)
    }

    pub fn persist_prepared(&self, record: &PreparedRecord) -> Result<(), JournalError> {
        self.replace_named(&format!("{}.journal", record.transaction_id), record)
    }

    pub fn persist_local_prepared(&self, record: &PreparedRecord) -> Result<(), JournalError> {
        self.replace_named("journal", record)
    }

    fn replace_named<T: Serialize>(&self, name: &str, record: &T) -> Result<(), JournalError> {
        use rustix::fs::{Mode, OFlags, fsync, openat, renameat};
        use std::fs::File;
        use std::io::Write;

        let encoded = serde_json::to_vec(record)
            .map_err(|error| JournalError::InvalidJson(error.to_string()))?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|error| JournalError::Io(error.to_string()))?;
        let next_name = format!("{name}.next-{}", encode_hex(&random));
        let next = openat(
            &self.directory,
            next_name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|error| JournalError::Io(error.to_string()))?;
        let mut next = File::from(next);
        next.write_all(&encoded)
            .and_then(|()| next.sync_all())
            .map_err(|error| JournalError::Io(error.to_string()))?;
        renameat(&self.directory, next_name.as_str(), &self.directory, name)
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

#[cfg(target_os = "linux")]
impl LinuxStableIdentity {
    pub fn new(device: u64, inode: u64, file_type: LinuxFileType) -> Self {
        Self {
            device,
            inode,
            file_type,
        }
    }
}

#[cfg(target_os = "linux")]
impl PreparedRecord {
    pub fn new_linux(
        transaction_id: [u8; 16],
        source_parent: &Path,
        original_name: &Path,
        source_parent_identity: LinuxStableIdentity,
        source_entry_identity: LinuxStableIdentity,
    ) -> Result<Self, JournalError> {
        Self::try_from(RawPreparedRecord {
            schema_major: JOURNAL_SCHEMA_MAJOR,
            schema_minor: JOURNAL_SCHEMA_MINOR,
            transaction_id: encode_hex(&transaction_id),
            phase: JournalPhase::Prepared,
            source_parent: NativePath::encode(source_parent),
            original_name: NativePath::encode(original_name),
            source_parent_identity,
            source_entry_identity,
        })
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, JournalError> {
        let raw: RawPreparedRecord = serde_json::from_slice(bytes)
            .map_err(|error| JournalError::InvalidJson(error.to_string()))?;
        Self::try_from(raw)
    }
}

#[cfg(target_os = "linux")]
impl TryFrom<RawPreparedRecord> for PreparedRecord {
    type Error = JournalError;

    fn try_from(raw: RawPreparedRecord) -> Result<Self, Self::Error> {
        JournalHeader::try_from(RawJournalHeader {
            schema_major: raw.schema_major,
            schema_minor: raw.schema_minor,
            transaction_id: raw.transaction_id.clone(),
        })?;
        let source_parent = raw.source_parent.decode()?;
        let original_name = raw.original_name.decode()?;
        let mut original_components = original_name.components();
        if !source_parent.is_absolute()
            || !matches!(
                original_components.next(),
                Some(std::path::Component::Normal(_))
            )
            || original_components.next().is_some()
        {
            return Err(JournalError::InvalidPreparedSourcePath);
        }
        if raw.source_parent_identity.file_type != LinuxFileType::Directory {
            return Err(JournalError::InvalidLinuxIdentity);
        }
        Ok(Self {
            schema_major: raw.schema_major,
            schema_minor: raw.schema_minor,
            transaction_id: raw.transaction_id,
            phase: raw.phase,
            source_parent: raw.source_parent,
            original_name: raw.original_name,
            source_parent_identity: raw.source_parent_identity,
            source_entry_identity: raw.source_entry_identity,
        })
    }
}

#[cfg(target_os = "linux")]
impl<'de> Deserialize<'de> for PreparedRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawPreparedRecord::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
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

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_persists_lossless_linux_source_identity() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let source_parent = PathBuf::from(OsString::from_vec(b"/source\x80".to_vec()));
        let original_name = PathBuf::from(OsString::from_vec(b"item\xff".to_vec()));
        let record = PreparedRecord::new_linux(
            [0x42; 16],
            &source_parent,
            &original_name,
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
        )
        .unwrap();

        let encoded = serde_json::to_vec(&record).unwrap();
        let parsed = PreparedRecord::parse(&encoded).unwrap();
        assert_eq!(parsed, record);
        assert_eq!(parsed.phase, JournalPhase::Prepared);
        assert_eq!(parsed.source_parent.decode().unwrap(), source_parent);
        assert_eq!(parsed.original_name.decode().unwrap(), original_name);
        assert_eq!(parsed.source_parent_identity.device, 11);
        assert_eq!(parsed.source_parent_identity.inode, 22);
        assert_eq!(parsed.source_entry_identity.inode, 33);

        let invalid = String::from_utf8(encoded)
            .unwrap()
            .replace("\"file_type\":\"directory\"", "\"file_type\":\"regular\"");
        assert!(serde_json::from_str::<PreparedRecord>(&invalid).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_rejects_root_as_original_name() {
        let result = PreparedRecord::new_linux(
            [0x43; 16],
            Path::new("/source"),
            Path::new("/"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
        );

        assert_eq!(result, Err(JournalError::InvalidPreparedSourcePath));
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
    fn transaction_headers_create_independent_central_discovery_records() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-central-discovery-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();

        let directory = LinuxJournalDirectory::open(&root).unwrap();
        let first = JournalHeader::new([0x12; 16]);
        let second = JournalHeader::new([0x34; 16]);
        directory.persist_discovery_header(&first).unwrap();
        directory.persist_discovery_header(&second).unwrap();

        for (name, expected) in [
            ("12121212121212121212121212121212.journal", first),
            ("34343434343434343434343434343434.journal", second),
        ] {
            let persisted = std::fs::read(root.join(name)).unwrap();
            assert_eq!(JournalHeader::parse(&persisted).unwrap(), expected);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_is_persisted_as_complete_central_discovery_record() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-prepared-discovery-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();

        let record = PreparedRecord::new_linux(
            [0x56; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 11, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 13, LinuxFileType::Regular),
        )
        .unwrap();
        let directory = LinuxJournalDirectory::open(&root).unwrap();
        directory.persist_prepared(&record).unwrap();

        let expected_path = root.join("56565656565656565656565656565656.journal");
        let persisted = std::fs::read(expected_path).unwrap();
        assert_eq!(PreparedRecord::parse(&persisted).unwrap(), record);
        assert!(JournalHeader::parse(&persisted).is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_is_persisted_to_anchored_source_local_journal() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-local-prepared-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let original = root.join("source-transaction");
        let moved = root.join("moved-source-transaction");
        std::fs::create_dir_all(&original).unwrap();

        let directory = LinuxJournalDirectory::open(&original).unwrap();
        std::fs::rename(&original, &moved).unwrap();
        std::fs::create_dir(&original).unwrap();
        let record = PreparedRecord::new_linux(
            [0x78; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 17, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 19, LinuxFileType::Regular),
        )
        .unwrap();

        directory.persist_local_prepared(&record).unwrap();

        let persisted = std::fs::read(moved.join("journal")).unwrap();
        assert_eq!(PreparedRecord::parse(&persisted).unwrap(), record);
        assert!(JournalHeader::parse(&persisted).is_err());
        assert!(!original.join("journal").exists());
        assert_eq!(std::fs::read_dir(&moved).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stale_fixed_next_entry_does_not_block_journal_replacement() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-journal-stale-next-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let stale = b"interrupted journal update";
        std::fs::write(root.join("journal.next"), stale).unwrap();

        let directory = LinuxJournalDirectory::open(&root).unwrap();
        let header = JournalHeader::new([0x6b; 16]);
        directory.replace(&header).unwrap();

        let persisted = std::fs::read(root.join("journal")).unwrap();
        assert_eq!(JournalHeader::parse(&persisted).unwrap(), header);
        assert_eq!(std::fs::read(root.join("journal.next")).unwrap(), stale);
        std::fs::remove_dir_all(root).unwrap();
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
