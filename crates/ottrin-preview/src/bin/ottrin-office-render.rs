//! Installed LibreOfficeKit entrypoint. The caller supplies the AppContainer,
//! read-only input snapshot, disposable profile and process/resource limits.

fn main() {
    #[cfg(windows)]
    if let Err(error) = windows::run() {
        eprintln!("ottrin-office-render: {error}");
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    {
        eprintln!("ottrin-office-render is only supported on Windows");
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::{CStr, CString, c_char};
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
        LoadLibraryExW,
    };
    use windows_sys::Win32::UI::Shell::UrlCreateFromPathW;

    // Stable ABI: https://docs.libreoffice.org/libreofficekit.html
    // Stable LibreOfficeKit C ABI prefixes. nSize is checked before borrowing
    // either vtable; no unstable SDK members are used.
    #[repr(C)]
    struct Kit {
        class: *const KitClass,
    }
    #[repr(C)]
    struct KitClass {
        size: usize,
        destroy: Option<unsafe extern "C" fn(*mut Kit)>,
        load: Option<unsafe extern "C" fn(*mut Kit, *const c_char) -> *mut Document>,
        error: Option<unsafe extern "C" fn(*mut Kit) -> *mut c_char>,
        load_options:
            Option<unsafe extern "C" fn(*mut Kit, *const c_char, *const c_char) -> *mut Document>,
        free: Option<unsafe extern "C" fn(*mut c_char)>,
    }
    #[repr(C)]
    struct Document {
        class: *const DocumentClass,
    }
    #[repr(C)]
    struct DocumentClass {
        size: usize,
        destroy: Option<unsafe extern "C" fn(*mut Document)>,
        save: Option<
            unsafe extern "C" fn(*mut Document, *const c_char, *const c_char, *const c_char) -> i32,
        >,
    }

    pub fn run() -> Result<(), String> {
        let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
        match arguments.as_slice() {
            [flag, executable] if flag == "--version" => {
                let executable = Path::new(executable);
                if !executable.is_absolute()
                    || !executable.parent().is_some_and(|directory| directory.join("mergedlo.dll").is_file())
                {
                    return Err("installed LibreOffice library is missing".into());
                }
                println!("LibreOffice {}", version(executable)?);
                Ok(())
            }
            [program, profile, input, output] => {
                let profile = profile.to_str().and_then(|value| value.strip_prefix("-env:UserInstallation="))
                    .ok_or("missing disposable profile URL")?;
                convert(Path::new(program), profile, Path::new(input), Path::new(output))
            }
            _ => Err("expected --version <installed-executable>, or <program-directory> <profile-argument> <input> <output-directory>".into()),
        }
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    fn version(executable: &Path) -> Result<String, String> {
        let path = wide(executable);
        let mut ignored = 0;
        // SAFETY: path is NUL-terminated and the output points to a live DWORD.
        let bytes = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut ignored) };
        if bytes == 0 || bytes > 1024 * 1024 {
            return Err("installed LibreOffice version resource is unavailable".into());
        }
        let mut buffer = vec![0u8; bytes as usize];
        let mut value = null_mut();
        let mut length = 0;
        // SAFETY: the buffer is the size requested by the API; the returned
        // value borrows this buffer and is checked before an unaligned read.
        unsafe {
            if GetFileVersionInfoW(path.as_ptr(), 0, bytes, buffer.as_mut_ptr().cast()) == 0
                || VerQueryValueW(
                    buffer.as_ptr().cast(),
                    [92u16, 0].as_ptr(),
                    &mut value,
                    &mut length,
                ) == 0
                || value.is_null()
                || length < size_of::<VS_FIXEDFILEINFO>() as u32
            {
                return Err("installed LibreOffice version resource is invalid".into());
            }
            let info = value.cast::<VS_FIXEDFILEINFO>().read_unaligned();
            if info.dwSignature != 0xfeef04bd {
                return Err("installed LibreOffice version signature is invalid".into());
            }
            Ok(format!(
                "{}.{}.{}.{}",
                info.dwFileVersionMS >> 16,
                info.dwFileVersionMS & 65535,
                info.dwFileVersionLS >> 16,
                info.dwFileVersionLS & 65535
            ))
        }
    }

    fn url(path: &Path) -> Result<CString, String> {
        if !path.is_absolute() {
            return Err("conversion paths must be absolute".into());
        }
        let path = wide(path);
        let mut result = vec![0u16; 32768];
        let mut length = result.len() as u32;
        // SAFETY: both path and output are live, appropriately sized buffers.
        if unsafe { UrlCreateFromPathW(path.as_ptr(), result.as_mut_ptr(), &mut length, 0) } < 0 {
            return Err("file URL could not be encoded".into());
        }
        result.truncate(length as usize);
        if result.last() == Some(&0) {
            result.pop();
        }
        CString::new(String::from_utf16(&result).map_err(|_| "file URL is not valid Unicode")?)
            .map_err(|_| "file URL contains NUL".into())
    }

    fn convert(program: &Path, profile: &str, input: &Path, output: &Path) -> Result<(), String> {
        if !program.is_absolute() || !output.is_dir() {
            return Err("installed program and private output directories are required".into());
        }
        let library = wide(&program.join("mergedlo.dll"));
        let program = CString::new(
            program
                .to_str()
                .ok_or("program path is not valid Unicode")?,
        )
        .map_err(|_| "program path contains NUL")?;
        let profile = CString::new(profile).map_err(|_| "profile URL contains NUL")?;
        let input = url(input)?;
        let output = url(&output.join("source.pdf"))?;
        // SAFETY: the caller admitted this installed provider; the absolute DLL
        // path and standard system/provider-directory search exclude the private
        // working directory. Keep the module loaded for this helper's lifetime:
        // LibreOffice owns global callbacks until process exit.
        unsafe {
            let module = LoadLibraryExW(
                library.as_ptr(),
                null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            );
            if module.is_null() {
                return Err(format!(
                    "installed LibreOffice library could not load: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let symbol = GetProcAddress(module, c"libreofficekit_hook_2".as_ptr().cast())
                .ok_or("installed LibreOfficeKit entrypoint is unavailable")?;
            let hook: unsafe extern "C" fn(*const c_char, *const c_char) -> *mut Kit =
                std::mem::transmute(symbol);
            let kit = hook(program.as_ptr(), profile.as_ptr());
            if kit.is_null()
                || (*kit).class.is_null()
                || (*kit).class.cast::<usize>().read() < size_of::<KitClass>()
            {
                return Err("installed LibreOfficeKit initialization failed".into());
            }
            let class = &*(*kit).class;
            let destroy = class
                .destroy
                .ok_or("LibreOfficeKit destroy API is unavailable")?;
            let result = (|| {
                let load = class
                    .load_options
                    .ok_or("LibreOfficeKit load API is unavailable")?;
                // These are actual LOK options, not UNO property names. Batch
                // cancels dialogs; macro execution is explicitly disabled.
                let document = load(
                    kit,
                    input.as_ptr(),
                    c"Batch=true,EnableMacrosExecution=false,MacroSecurityLevel=3".as_ptr(),
                );
                if document.is_null() {
                    return Err(last_error(kit, class, "document could not load"));
                }
                if (*document).class.is_null()
                    || (*document).class.cast::<usize>().read() < size_of::<DocumentClass>()
                {
                    return Err("LibreOfficeKit document ABI is unsupported".into());
                }
                let dc = &*(*document).class;
                let close = dc
                    .destroy
                    .ok_or("LibreOfficeKit document destroy API is unavailable")?;
                let saved = dc
                    .save
                    .map(|save| save(document, output.as_ptr(), c"pdf".as_ptr(), null()))
                    .unwrap_or(0);
                close(document);
                if saved == 0 {
                    Err(last_error(kit, class, "PDF export failed"))
                } else {
                    Ok(())
                }
            })();
            destroy(kit);
            result
        }
    }

    unsafe fn last_error(kit: *mut Kit, class: &KitClass, fallback: &str) -> String {
        if let (Some(error), Some(free)) = (class.error, class.free) {
            // SAFETY: the checked stable SDK vtable returns a NUL-terminated
            // error owned by that SDK; release it using the same SDK's allocator.
            unsafe {
                let pointer = error(kit);
                if !pointer.is_null() {
                    let text = CStr::from_ptr(pointer)
                        .to_string_lossy()
                        .chars()
                        .take(4096)
                        .collect::<String>();
                    free(pointer);
                    if !text.is_empty() {
                        return text;
                    }
                }
            }
        }
        fallback.into()
    }
}
