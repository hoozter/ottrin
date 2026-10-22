//! Native volume enumeration and identity-based directory recovery shared by transactions.
use super::{open_directory, open_directory_by_identity};
use crate::{
    PlatformError,
    move_recovery::{WindowsStableIdentity, stable_identity_from_handle},
};
use std::{
    collections::BTreeMap, ffi::OsString, fs::File, os::windows::ffi::OsStringExt, path::PathBuf,
};
use windows_sys::Win32::{
    Foundation::{ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::*,
};
const MAX_VOLUMES: usize = 512;
fn io(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(error.to_string())
}
struct VolumeCursor(HANDLE);
impl Drop for VolumeCursor {
    fn drop(&mut self) {
        unsafe {
            FindVolumeClose(self.0);
        }
    }
}
pub(crate) struct Volumes {
    anchors: BTreeMap<u64, File>,
    enumerated: bool,
}
impl Volumes {
    pub(crate) fn new(central: &File) -> Result<Self, PlatformError> {
        let mut anchors = BTreeMap::new();
        anchors.insert(
            stable_identity_from_handle(central)
                .map_err(io)?
                .volume_serial_number(),
            central.try_clone().map_err(io)?,
        );
        Ok(Self {
            anchors,
            enumerated: false,
        })
    }
    pub(crate) fn resolve(
        &mut self,
        expected: WindowsStableIdentity,
        access: u32,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<File, PlatformError> {
        let serial = expected.volume_serial_number();
        if !self.anchors.contains_key(&serial) && !self.enumerated {
            self.enumerated = true;
            let mut name = [0u16; 1024];
            let raw = unsafe { FindFirstVolumeW(name.as_mut_ptr(), name.len() as u32) };
            if raw == INVALID_HANDLE_VALUE {
                return Err(io(std::io::Error::last_os_error()));
            }
            let _cursor = VolumeCursor(raw);
            for index in 0..MAX_VOLUMES {
                progress(0)?;
                let end = name
                    .iter()
                    .position(|ch| *ch == 0)
                    .ok_or_else(|| io("unterminated native volume name"))?;
                let path = PathBuf::from(OsString::from_wide(&name[..end]));
                // Names come from the local volume manager, never a journal path.
                if let Ok(file) = open_directory(&path, FILE_READ_ATTRIBUTES | SYNCHRONIZE) {
                    let key = stable_identity_from_handle(&file)
                        .map_err(io)?
                        .volume_serial_number();
                    self.anchors.entry(key).or_insert(file);
                }
                if unsafe { FindNextVolumeW(raw, name.as_mut_ptr(), name.len() as u32) } == 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                        return Err(io(error));
                    }
                    break;
                }
                if index + 1 == MAX_VOLUMES {
                    return Err(io("native recovery volume budget exceeded"));
                }
            }
        }
        let anchor = self
            .anchors
            .get(&serial)
            .ok_or_else(|| io("Recovery volume is unavailable"))?;
        open_directory_by_identity(anchor, expected, access).map_err(io)
    }
}
