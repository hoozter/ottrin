//! Run Kit conversion on the Windows backend's owning message thread. Calling
//! from the initialization caller can deadlock SetWindowPos against SolarMutex.
use std::{
    ffi::c_void,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    ptr::{null, null_mut},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicPtr, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::{HANDLE, HMODULE, HWND, LPARAM, WAIT_OBJECT_0, WPARAM},
    System::{
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleHandleExW, GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32,
            LoadLibraryExW,
        },
        Threading::{
            CreateEventW, GetCurrentProcessId, GetCurrentThreadId, INFINITE, SetEvent,
            WaitForSingleObject,
        },
    },
    UI::WindowsAndMessaging::{
        GWLP_WNDPROC, HHOOK, HOOKPROC, HWND_MESSAGE, MSG, PM_REMOVE, WH_GETMESSAGE, WM_NULL,
        WNDENUMPROC,
    },
};

// The hook belongs to one thread in this helper process, never a desktop/global
// hook. Serialize its scoped context through unhook, including concurrent callers.
static DISPATCH: Mutex<()> = Mutex::new(());
static ACTIVE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static MESSAGE: OnceLock<u32> = OnceLock::new();

// Keep --version usable in noninteractive AppContainer sessions. Importing
// user32 at executable startup would initialize a desktop before main runs.
#[derive(Clone, Copy)]
struct Ui {
    window_thread: unsafe extern "system" fn(HWND, *mut u32) -> u32,
    window_procedure: unsafe extern "system" fn(HWND, i32) -> isize,
    enum_windows: unsafe extern "system" fn(WNDENUMPROC, LPARAM) -> i32,
    find_window: unsafe extern "system" fn(HWND, HWND, *const u16, *const u16) -> HWND,
    next_hook: unsafe extern "system" fn(HHOOK, i32, WPARAM, LPARAM) -> isize,
    register_message: unsafe extern "system" fn(*const u16) -> u32,
    set_hook: unsafe extern "system" fn(i32, HOOKPROC, HMODULE, u32) -> HHOOK,
    post_message: unsafe extern "system" fn(u32, u32, WPARAM, LPARAM) -> i32,
    remove_hook: unsafe extern "system" fn(HHOOK) -> i32,
}
static UI: OnceLock<Result<Ui, String>> = OnceLock::new();

unsafe fn symbol<T: Copy>(module: HMODULE, name: &std::ffi::CStr) -> Result<T, String> {
    // SAFETY: each caller supplies the documented system ABI for this symbol.
    unsafe {
        let pointer = GetProcAddress(module, name.as_ptr().cast()).ok_or_else(|| {
            format!(
                "Windows backend API {} is unavailable",
                name.to_string_lossy()
            )
        })?;
        assert_eq!(size_of::<T>(), size_of_val(&pointer));
        Ok(std::mem::transmute_copy(&pointer))
    }
}

fn ui() -> Result<&'static Ui, String> {
    UI.get_or_init(|| {
        // SAFETY: load only the OS system directory DLL. Keep it loaded for the
        // helper lifetime so hook callbacks never outlive their function table.
        unsafe {
            let module = LoadLibraryExW(w!("user32.dll"), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
            if module.is_null() {
                return Err(format!(
                    "Windows backend API initialization failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Ui {
                window_thread: symbol(module, c"GetWindowThreadProcessId")?,
                window_procedure: symbol(module, c"GetWindowLongPtrW")?,
                enum_windows: symbol(module, c"EnumWindows")?,
                find_window: symbol(module, c"FindWindowExW")?,
                next_hook: symbol(module, c"CallNextHookEx")?,
                register_message: symbol(module, c"RegisterWindowMessageW")?,
                set_hook: symbol(module, c"SetWindowsHookExW")?,
                post_message: symbol(module, c"PostThreadMessageW")?,
                remove_hook: symbol(module, c"UnhookWindowsHookEx")?,
            })
        }
    })
    .as_ref()
    .map_err(Clone::clone)
}

struct Discovery {
    ui: Ui,
    module: HMODULE,
    thread: u32,
    ambiguous: bool,
}
unsafe extern "system" fn identify(window: HWND, context: LPARAM) -> i32 {
    // SAFETY: enumeration borrows this stack context synchronously. Only windows
    // in this process whose procedure belongs to the admitted backend qualify.
    unsafe {
        let discovery = &mut *(context as *mut Discovery);
        let mut process = 0;
        let thread = (discovery.ui.window_thread)(window, &mut process);
        if process == GetCurrentProcessId() {
            let procedure = (discovery.ui.window_procedure)(window, GWLP_WNDPROC);
            let mut module = null_mut();
            if procedure != 0
                && GetModuleHandleExW(
                    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                        | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                    procedure as *const u16,
                    &mut module,
                ) != 0
                && module == discovery.module
            {
                discovery.ambiguous |= discovery.thread != 0 && discovery.thread != thread;
                discovery.thread = thread;
            }
        }
    }
    1
}

fn owner_thread(program: &Path, ui: &Ui) -> Result<u32, String> {
    let path: Vec<_> = program
        .join("vclplug_winlo.dll")
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: paths and synchronous enumeration buffers remain live. This only
    // observes the already loaded provider; it does not load another backend.
    unsafe {
        let module = GetModuleHandleW(path.as_ptr());
        if module.is_null() {
            return Err("installed Windows conversion backend is not loaded".into());
        }
        let mut discovery = Discovery {
            ui: *ui,
            module,
            thread: 0,
            ambiguous: false,
        };
        let context = &mut discovery as *mut Discovery as LPARAM;
        if (ui.enum_windows)(Some(identify), context) == 0 {
            return Err(format!(
                "backend window enumeration failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut window = null_mut();
        loop {
            window = (ui.find_window)(HWND_MESSAGE, window, null(), null());
            if window.is_null() {
                break;
            }
            identify(window, context);
        }
        if discovery.thread == 0 || discovery.ambiguous {
            return Err("installed Windows conversion backend has no unique owner thread".into());
        }
        Ok(discovery.thread)
    }
}

struct TaskState<F, T> {
    job: Option<F>,
    result: Option<Result<T, String>>,
}

struct Task<F, T> {
    state: Mutex<TaskState<F, T>>,
}

fn fatal_wait(message: &str) -> ! {
    // A queued callback may still borrow the caller's scoped data. Never return
    // and free it after a synchronization failure; the parent owns process-tree
    // termination, diagnostics and profile cleanup for this isolated helper.
    eprintln!("ottrin-office-render: {message}");
    std::process::abort()
}

#[derive(Clone, Copy)]
struct Invocation {
    context: *mut c_void,
    execute: unsafe fn(*mut c_void),
    event: HANDLE,
}

unsafe fn complete<F: FnOnce() -> T + Send, T: Send>(context: *mut c_void) {
    // SAFETY: run pairs this trampoline with exactly its scoped Task<F, T>.
    let task = unsafe { &*(context as *const Task<F, T>) };
    let job = task
        .state
        .lock()
        .unwrap_or_else(|_| fatal_wait("backend task lock failed"))
        .job
        .take();
    let result = job
        .ok_or_else(|| "backend task was already consumed".to_string())
        .and_then(|job| {
            catch_unwind(AssertUnwindSafe(job))
                .map_err(|_| "Windows backend conversion panicked".to_string())
        });
    task.state
        .lock()
        .unwrap_or_else(|_| fatal_wait("backend result lock failed"))
        .result = Some(result);
}

unsafe extern "system" fn invoke(code: i32, reason: WPARAM, data: LPARAM) -> isize {
    // SAFETY: Windows supplies MSG for WH_GETMESSAGE. Before accessing context,
    // match both the registered message and the exact active invocation pointer.
    unsafe {
        if code >= 0 && reason == PM_REMOVE as usize {
            let message = &mut *(data as *mut MSG);
            let active = ACTIVE.load(Ordering::Acquire);
            if !active.is_null()
                && Some(&message.message) == MESSAGE.get()
                && message.wParam == active as usize
            {
                message.message = WM_NULL;
                let Invocation {
                    context,
                    execute,
                    event,
                } = *(active as *const Invocation);
                // No context borrow survives this disarm. Nested/later messages
                // cannot execute a second task or access it after completion.
                ACTIVE.store(null_mut(), Ordering::Release);
                execute(context);
                if SetEvent(event) == 0 {
                    fatal_wait("backend completion signal failed");
                }
                return 0;
            }
        }
        let ui = UI
            .get()
            .and_then(|result| result.as_ref().ok())
            .unwrap_or_else(|| fatal_wait("backend callback API unavailable"));
        (ui.next_hook)(null_mut(), code, reason, data)
    }
}

pub(super) fn run<F: FnOnce() -> T + Send, T: Send>(program: &Path, job: F) -> Result<T, String> {
    let _dispatch = DISPATCH
        .lock()
        .map_err(|_| "Windows backend dispatch lock failed")?;
    let ui = ui()?;
    let thread = owner_thread(program, ui)?;
    // SAFETY: these are thread-safe Win32 synchronization APIs. The scoped task
    // remains live until completion; failures after posting terminate the helper.
    unsafe {
        if thread == GetCurrentThreadId() {
            return Ok(job());
        }
        let message =
            *MESSAGE.get_or_init(|| (ui.register_message)(w!("OttrinOfficeBackendConversion")));
        if message == 0 {
            return Err("backend message registration failed".into());
        }
        let event: HANDLE = CreateEventW(null(), 1, 0, null());
        if event.is_null() {
            return Err("backend completion event creation failed".into());
        }
        let event = OwnedHandle::from_raw_handle(event);
        let task = Task {
            state: Mutex::new(TaskState {
                job: Some(job),
                result: None,
            }),
        };
        let invocation = Invocation {
            context: &task as *const Task<F, T> as *mut c_void,
            execute: complete::<F, T>,
            event: event.as_raw_handle(),
        };
        let pointer = &invocation as *const Invocation as *mut c_void;
        ACTIVE.store(pointer, Ordering::Release);
        let hook = (ui.set_hook)(WH_GETMESSAGE, Some(invoke), null_mut(), thread);
        if hook.is_null() {
            ACTIVE.store(null_mut(), Ordering::Release);
            return Err(format!(
                "backend thread dispatch unavailable: {}",
                std::io::Error::last_os_error()
            ));
        }
        if (ui.post_message)(thread, message, pointer as usize, 0) == 0 {
            ACTIVE.store(null_mut(), Ordering::Release);
            let error = std::io::Error::last_os_error();
            if (ui.remove_hook)(hook) == 0 {
                fatal_wait("backend hook removal failed after message failure");
            }
            return Err(format!("backend message failed: {error}"));
        }
        if WaitForSingleObject(event.as_raw_handle(), INFINITE) != WAIT_OBJECT_0 {
            fatal_wait("backend completion wait failed");
        }
        if (ui.remove_hook)(hook) == 0 {
            fatal_wait("backend hook removal failed after completion");
        }
        task.state
            .into_inner()
            .map_err(|_| "backend result lock failed")?
            .result
            .ok_or_else(|| "backend returned no result".to_string())?
    }
}

use windows_sys::w;
