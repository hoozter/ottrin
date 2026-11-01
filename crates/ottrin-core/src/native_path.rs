//! Lossless native filesystem paths shared by IPC, preferences and recovery records.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Human-readable presentation only. Never parse this label back into an operation path.
pub fn display_label(path: &Path) -> String {
    let text = path.display().to_string();
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if let Some(Component::Prefix(prefix)) = path.components().next() {
            match prefix.kind() {
                Prefix::VerbatimDisk(_) => {
                    return text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned();
                }
                Prefix::VerbatimUNC(_, _) => {
                    if let Some(tail) = text.strip_prefix(r"\\?\UNC\") {
                        return format!(r"\\{tail}");
                    }
                }
                // Volume GUIDs and device namespaces have no equivalent short label.
                _ => {}
            }
        }
    }
    text
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum NativePathError {
    #[error("path encoding does not match this platform")]
    WrongPlatformEncoding,
    #[error("invalid native path units")]
    InvalidNativePathUnits,
}

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

    pub fn decode(&self) -> Result<PathBuf, NativePathError> {
        #[cfg(unix)]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            if self.encoding != NativePathEncoding::UnixBytes {
                return Err(NativePathError::WrongPlatformEncoding);
            }
            let bytes = decode_hex(&self.units_hex)?;
            if bytes.contains(&0) {
                return Err(NativePathError::InvalidNativePathUnits);
            }
            Ok(PathBuf::from(OsString::from_vec(bytes)))
        }
        #[cfg(windows)]
        {
            use std::ffi::OsString;
            use std::os::windows::ffi::OsStringExt;
            if self.encoding != NativePathEncoding::WindowsUtf16Le {
                return Err(NativePathError::WrongPlatformEncoding);
            }
            if !self.units_hex.len().is_multiple_of(4) {
                return Err(NativePathError::InvalidNativePathUnits);
            }
            let units = self
                .units_hex
                .as_bytes()
                .chunks_exact(4)
                .map(decode_u16_hex)
                .collect::<Result<Vec<_>, _>>()?;
            if units.contains(&0) {
                return Err(NativePathError::InvalidNativePathUnits);
            }
            Ok(PathBuf::from(OsString::from_wide(&units)))
        }
        #[cfg(not(any(unix, windows)))]
        {
            if self.encoding != NativePathEncoding::UnixBytes {
                return Err(NativePathError::WrongPlatformEncoding);
            }
            let bytes = decode_hex(&self.units_hex)?;
            if bytes.contains(&0) {
                return Err(NativePathError::InvalidNativePathUnits);
            }
            let text =
                String::from_utf8(bytes).map_err(|_| NativePathError::InvalidNativePathUnits)?;
            Ok(PathBuf::from(text))
        }
    }
}

/// Encode native units and transaction identifiers in canonical lowercase hex.
pub fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[usize::from(byte >> 4)] as char);
        encoded.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

/// Decode canonical lowercase byte pairs; reject partial or malformed units.
pub fn decode_hex(encoded: &str) -> Result<Vec<u8>, NativePathError> {
    if !encoded.len().is_multiple_of(2) {
        return Err(NativePathError::InvalidNativePathUnits);
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
fn decode_u16_hex(digits: &[u8]) -> Result<u16, NativePathError> {
    let mut value = 0_u16;
    for digit in digits {
        value = (value << 4) | u16::from(decode_nibble(*digit)?);
    }
    Ok(value)
}

fn decode_nibble(digit: u8) -> Result<u8, NativePathError> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        _ => Err(NativePathError::InvalidNativePathUnits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_codec_preserves_existing_persisted_representation_and_rejects_nul() {
        #[cfg(unix)]
        let (path, expected) = (
            Path::new("/a"),
            r#"{"encoding":"unix-bytes","units_hex":"2f61"}"#,
        );
        #[cfg(windows)]
        let (path, expected) = (
            Path::new(r"C:\a"),
            r#"{"encoding":"windows-utf16-le","units_hex":"0043003a005c0061"}"#,
        );
        #[cfg(not(any(unix, windows)))]
        let (path, expected) = (
            Path::new("/a"),
            r#"{"encoding":"unix-bytes","units_hex":"2f61"}"#,
        );
        assert_eq!(
            serde_json::to_string(&NativePath::encode(path)).unwrap(),
            expected
        );
        assert_eq!(
            serde_json::from_str::<NativePath>(expected)
                .unwrap()
                .decode()
                .unwrap(),
            path
        );
        assert!(NativePath::encode(Path::new("bad\0path")).decode().is_err());
        assert_eq!(
            NativePath::encode(Path::new("")).decode().unwrap(),
            Path::new("")
        );
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
            assert_eq!(
                encoded.decode(),
                Err(NativePathError::InvalidNativePathUnits)
            );
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
        assert_eq!(
            encoded.decode(),
            Err(NativePathError::WrongPlatformEncoding)
        );
    }
}

/// Optional paths in cooperating-application requests.
pub mod optional {
    use super::*;
    use serde::{Deserializer, Serializer};
    pub fn serialize<S: Serializer>(
        path: &Option<PathBuf>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        path.as_deref()
            .map(NativePath::encode)
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<PathBuf>, D::Error> {
        Option::<NativePath>::deserialize(deserializer)?
            .map(|path| path.decode().map_err(serde::de::Error::custom))
            .transpose()
    }
}

/// Selected native paths in cooperating-application responses.
pub mod list {
    use super::*;
    use serde::{Deserializer, Serializer};
    pub fn serialize<S: Serializer>(paths: &[PathBuf], serializer: S) -> Result<S::Ok, S::Error> {
        paths
            .iter()
            .map(|path| NativePath::encode(path))
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<PathBuf>, D::Error> {
        Vec::<NativePath>::deserialize(deserializer)?
            .into_iter()
            .map(|path| path.decode().map_err(serde::de::Error::custom))
            .collect()
    }
}

/// Native filenames use the same platform-unit wire representation as paths.
/// Filename shape and request limits are checked by the picker contract.
pub mod os_string {
    use super::*;
    use serde::{Deserializer, Serializer};
    pub fn serialize<S: Serializer>(
        name: &std::ffi::OsString,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        NativePath::encode(Path::new(name)).serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<std::ffi::OsString, D::Error> {
        NativePath::deserialize(deserializer)?
            .decode()
            .map(PathBuf::into_os_string)
            .map_err(serde::de::Error::custom)
    }
}
