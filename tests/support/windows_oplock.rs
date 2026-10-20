use std::{
    fs::{File, OpenOptions},
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_IO_PENDING, ERROR_NOT_FOUND, ERROR_OPERATION_ABORTED},
    Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_OVERLAPPED, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    },
    System::{
        IO::{CancelIoEx, DeviceIoControl, GetOverlappedResult, OVERLAPPED},
        Ioctl::FSCTL_REQUEST_OPLOCK_LEVEL_1,
        Threading::{CreateEventW, WaitForSingleObject},
    },
};

// The heap-owned OVERLAPPED and event live until the request has completed.
// Closing the file implicitly acknowledges the break, including failure teardown.
pub(crate) struct Oplock {
    file: Option<File>,
    event: OwnedHandle,
    overlapped: Box<OVERLAPPED>,
    pending: bool,
}
impl Oplock {
    pub(crate) fn request(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OVERLAPPED | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if event.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let event = unsafe { OwnedHandle::from_raw_handle(event.cast()) };
        let mut result = Self {
            file: Some(file),
            overlapped: Box::new(OVERLAPPED {
                hEvent: event.as_raw_handle().cast(),
                ..Default::default()
            }),
            event,
            pending: false,
        };
        let mut bytes = 0;
        let ok = unsafe {
            DeviceIoControl(
                result.file.as_ref().unwrap().as_raw_handle().cast(),
                FSCTL_REQUEST_OPLOCK_LEVEL_1,
                ptr::null(),
                0,
                ptr::null_mut(),
                0,
                &mut bytes,
                result.overlapped.as_mut(),
            )
        };
        let error = std::io::Error::last_os_error();
        if ok != 0 {
            return Err(std::io::Error::other(
                "Oplock request completed without a pending lease",
            ));
        }
        if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            return Err(error);
        }
        result.pending = true;
        Ok(result)
    }
    pub(crate) fn wait_break(&self) -> u32 {
        unsafe { WaitForSingleObject(self.event.as_raw_handle().cast(), 3000) }
    }
    pub(crate) fn close(&mut self) -> std::io::Result<()> {
        let mut result = Ok(());
        if let Some(file) = &self.file
            && self.pending
        {
            if unsafe { CancelIoEx(file.as_raw_handle().cast(), self.overlapped.as_ref()) } == 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_NOT_FOUND as i32) {
                    result = Err(error);
                }
            }
            let mut bytes = 0;
            if unsafe {
                GetOverlappedResult(
                    file.as_raw_handle().cast(),
                    self.overlapped.as_mut(),
                    &mut bytes,
                    1,
                )
            } == 0
            {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_OPERATION_ABORTED as i32) {
                    result = Err(error);
                }
            }
            self.pending = false;
        }
        drop(self.file.take());
        result
    }
}
impl Drop for Oplock {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            eprintln!("Owned oplock teardown failed: {error}");
        }
    }
}
