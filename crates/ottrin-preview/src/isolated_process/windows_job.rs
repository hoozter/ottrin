use super::*;
use std::ffi::{OsStr, OsString, c_void};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::mem::{size_of, size_of_val};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOBOBJECT_LIMIT_VIOLATION_INFORMATION_2, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, JobObjectLimitViolationInformation2,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
    GetExitCodeProcess, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::UrlCreateFromPathW;

const MAX_DIAGNOSTIC_BYTES: u64 = 1024 * 1024;
const WAIT_SLICE: Duration = Duration::from_millis(10);
const TERMINATION_WAIT_MS: u32 = 5_000;
const PROFILE_MARKER: &str = "-env:UserInstallation=file:///profile";

pub(super) fn run(
    request: &IsolatedProcessRequest,
    cancelled: &AtomicBool,
) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
    validate(request)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(IsolatedProcessFailure::Cancelled);
    }

    let arguments = translated_arguments(request)?;
    let mut logs = LogFiles::open(request)?;
    let stdin_file = OpenOptions::new()
        .read(true)
        .open("NUL")
        .map_err(|_| IsolatedProcessFailure::Unavailable)?;
    let inherited_handles = [
        raw_handle(&stdin_file),
        raw_handle(logs.stdout()),
        raw_handle(logs.stderr()),
    ];
    let job = create_job(request)?;
    let jobs = [job.raw()];
    let attributes = AttributeList::new(&inherited_handles, &jobs)?;
    let inheritance = InheritanceGuard::new(&inherited_handles)?;
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>())
        .map_err(|_| IsolatedProcessFailure::Unavailable)?;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = inherited_handles[0];
    startup.StartupInfo.hStdOutput = inherited_handles[1];
    startup.StartupInfo.hStdError = inherited_handles[2];
    startup.lpAttributeList = attributes.raw();

    let executable = nul_terminated(request.executable.as_os_str());
    let current_directory = nul_terminated(request.private_profile_dir.as_os_str());
    let mut command_line = command_line(request.executable.as_os_str(), &arguments);
    let mut process_info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: all pointers reference live, NUL-terminated or initialized buffers;
    // the attribute list whitelists standard I/O and assigns the process to `job`
    // before its first instruction executes.
    let created = unsafe {
        CreateProcessW(
            executable.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
            null(),
            current_directory.as_ptr(),
            &startup.StartupInfo,
            &mut process_info,
        )
    };
    drop(attributes);
    drop(inheritance);
    if created == 0 {
        return Err(IsolatedProcessFailure::Unavailable);
    }
    let process = OwnedHandle::new(process_info.hProcess)?;
    let thread = OwnedHandle::new(process_info.hThread)?;
    drop(thread);

    let exit_code = wait_for_job(&job, &process, request.timeout, cancelled)?;
    logs.close_writers();
    let stdout = read_bounded(&logs.stdout_path)?;
    let diagnostics = read_bounded(&logs.stderr_path)?;
    if job_limit_violated(&job) {
        return Err(IsolatedProcessFailure::LimitExceeded);
    }
    if exit_code != 0 {
        return Err(IsolatedProcessFailure::Crash);
    }
    validate_output_budget(&request.private_output_dir, request.max_output_bytes)?;
    Ok(IsolatedProcessOutput {
        stdout,
        diagnostics,
    })
}

fn validate(request: &IsolatedProcessRequest) -> Result<(), IsolatedProcessFailure> {
    if !request.executable.is_absolute()
        || request.timeout.is_zero()
        || request.max_memory_bytes == 0
        || request.max_processes == 0
        || request.max_output_bytes == 0
        || !regular_file(&request.executable)
        || !directory(&request.private_profile_dir)
        || !directory(&request.private_output_dir)
    {
        return Err(IsolatedProcessFailure::InvalidRequest);
    }
    if let Some(input) = &request.input
        && (!regular_file(&input.host_path)
            || input.sandbox_path.parent() != Some(Path::new("/input")))
    {
        return Err(IsolatedProcessFailure::InvalidRequest);
    }
    Ok(())
}

fn translated_arguments(
    request: &IsolatedProcessRequest,
) -> Result<Vec<OsString>, IsolatedProcessFailure> {
    let mut saw_input = request.input.is_none();
    let profile_argument = profile_argument(&request.private_profile_dir)?;
    let mut translated = Vec::with_capacity(request.arguments.len());
    for argument in &request.arguments {
        if argument == OsStr::new(PROFILE_MARKER) {
            translated.push(profile_argument.clone());
        } else if argument == OsStr::new("/output") {
            translated.push(request.private_output_dir.as_os_str().to_owned());
        } else if let Some(input) = &request.input
            && argument == input.sandbox_path.as_os_str()
        {
            translated.push(input.host_path.as_os_str().to_owned());
            saw_input = true;
        } else {
            translated.push(argument.clone());
        }
    }
    if !saw_input {
        return Err(IsolatedProcessFailure::InvalidRequest);
    }
    Ok(translated)
}

fn profile_argument(profile: &Path) -> Result<OsString, IsolatedProcessFailure> {
    let path = nul_terminated(profile.as_os_str());
    let mut url = vec![0_u16; 32_768];
    let mut length =
        u32::try_from(url.len()).map_err(|_| IsolatedProcessFailure::InvalidRequest)?;
    // SAFETY: `path` and `url` are valid NUL-terminated/input and writable/output buffers.
    let result = unsafe { UrlCreateFromPathW(path.as_ptr(), url.as_mut_ptr(), &mut length, 0) };
    if result < 0 {
        return Err(IsolatedProcessFailure::InvalidRequest);
    }
    url.truncate(length as usize);
    if url.last() == Some(&0) {
        url.pop();
    }
    let mut argument: Vec<u16> = OsStr::new("-env:UserInstallation=").encode_wide().collect();
    argument.extend(url);
    Ok(OsString::from_wide(&argument))
}

fn create_job(request: &IsolatedProcessRequest) -> Result<OwnedHandle, IsolatedProcessFailure> {
    // SAFETY: null attributes/name request a private unnamed Job Object.
    let raw = unsafe { CreateJobObjectW(null(), null()) };
    let job = OwnedHandle::new(raw).map_err(|_| IsolatedProcessFailure::Unavailable)?;
    let memory_limit = usize::try_from(request.max_memory_bytes)
        .map_err(|_| IsolatedProcessFailure::InvalidRequest)?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_JOB_MEMORY
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
    limits.BasicLimitInformation.ActiveProcessLimit =
        u32::try_from(request.max_processes).map_err(|_| IsolatedProcessFailure::InvalidRequest)?;
    limits.ProcessMemoryLimit = memory_limit;
    limits.JobMemoryLimit = memory_limit;
    // SAFETY: `limits` has the exact layout and lifetime required by this information class.
    let set = unsafe {
        SetInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                .map_err(|_| IsolatedProcessFailure::Unavailable)?,
        )
    };
    if set == 0 {
        return Err(IsolatedProcessFailure::Unavailable);
    }
    Ok(job)
}

fn wait_for_job(
    job: &OwnedHandle,
    process: &OwnedHandle,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<u32, IsolatedProcessFailure> {
    let deadline = Instant::now() + timeout;
    let mut exit_code = None;
    loop {
        if cancelled.load(Ordering::Acquire) {
            terminate_job(job, process);
            return Err(IsolatedProcessFailure::Cancelled);
        }
        if Instant::now() >= deadline {
            terminate_job(job, process);
            return Err(IsolatedProcessFailure::Timeout);
        }
        if exit_code.is_none() {
            // SAFETY: `process` remains valid for the duration of the wait.
            match unsafe { WaitForSingleObject(process.raw(), 0) } {
                WAIT_OBJECT_0 => {
                    let mut code = 0;
                    // SAFETY: the process is signalled and `code` is writable.
                    if unsafe { GetExitCodeProcess(process.raw(), &mut code) } == 0 {
                        terminate_job(job, process);
                        return Err(IsolatedProcessFailure::Crash);
                    }
                    exit_code = Some(code);
                }
                WAIT_TIMEOUT => {}
                WAIT_FAILED => {
                    terminate_job(job, process);
                    return Err(IsolatedProcessFailure::Crash);
                }
                _ => {
                    terminate_job(job, process);
                    return Err(IsolatedProcessFailure::Crash);
                }
            }
        }
        if let Some(exit_code) = exit_code
            && active_processes(job)? == 0
        {
            return Ok(exit_code);
        }
        std::thread::sleep(WAIT_SLICE);
    }
}

fn active_processes(job: &OwnedHandle) -> Result<u32, IsolatedProcessFailure> {
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    // SAFETY: `accounting` has the exact layout and lifetime required by this information class.
    let queried = unsafe {
        QueryInformationJobObject(
            job.raw(),
            JobObjectBasicAccountingInformation,
            (&raw mut accounting).cast(),
            u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>())
                .map_err(|_| IsolatedProcessFailure::Crash)?,
            null_mut(),
        )
    };
    if queried == 0 {
        return Err(IsolatedProcessFailure::Crash);
    }
    Ok(accounting.ActiveProcesses)
}

fn job_limit_violated(job: &OwnedHandle) -> bool {
    let mut violation = JOBOBJECT_LIMIT_VIOLATION_INFORMATION_2::default();
    // SAFETY: `violation` has the exact layout and lifetime required by this information class.
    let queried = unsafe {
        QueryInformationJobObject(
            job.raw(),
            JobObjectLimitViolationInformation2,
            (&raw mut violation).cast(),
            u32::try_from(size_of::<JOBOBJECT_LIMIT_VIOLATION_INFORMATION_2>()).unwrap_or(0),
            null_mut(),
        )
    };
    queried != 0
        && violation.ViolationLimitFlags
            & (JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                | JOB_OBJECT_LIMIT_JOB_MEMORY
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY)
            != 0
}

fn terminate_job(job: &OwnedHandle, process: &OwnedHandle) {
    // SAFETY: both handles remain valid; closing `job` later repeats the kill-on-close guarantee.
    unsafe {
        let _ = TerminateJobObject(job.raw(), 1);
        let _ = WaitForSingleObject(process.raw(), TERMINATION_WAIT_MS);
    }
}

fn command_line(executable: &OsStr, arguments: &[OsString]) -> Vec<u16> {
    let mut command = Vec::new();
    append_quoted_argument(&mut command, executable);
    for argument in arguments {
        command.push(b' ' as u16);
        append_quoted_argument(&mut command, argument);
    }
    command.push(0);
    command
}

fn append_quoted_argument(command: &mut Vec<u16>, argument: &OsStr) {
    let units: Vec<u16> = argument.encode_wide().collect();
    let quote = units.is_empty()
        || units
            .iter()
            .any(|unit| *unit <= b' ' as u16 || *unit == b'"' as u16);
    if !quote {
        command.extend(units);
        return;
    }
    command.push(b'"' as u16);
    let mut backslashes = 0;
    for unit in units {
        if unit == b'\\' as u16 {
            backslashes += 1;
        } else if unit == b'"' as u16 {
            command.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2 + 1));
            command.push(unit);
            backslashes = 0;
        } else {
            command.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
            command.push(unit);
            backslashes = 0;
        }
    }
    command.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
    command.push(b'"' as u16);
}

fn nul_terminated(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn raw_handle(file: &File) -> HANDLE {
    file.as_raw_handle()
}

fn regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

fn directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false)
}

fn private_log(path: &Path) -> Result<File, IsolatedProcessFailure> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| IsolatedProcessFailure::InvalidRequest)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, IsolatedProcessFailure> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| IsolatedProcessFailure::Crash)?
        .take(MAX_DIAGNOSTIC_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| IsolatedProcessFailure::Crash)?;
    if bytes.len() as u64 > MAX_DIAGNOSTIC_BYTES {
        return Err(IsolatedProcessFailure::LimitExceeded);
    }
    Ok(bytes)
}

fn validate_output_budget(directory: &Path, max: u64) -> Result<(), IsolatedProcessFailure> {
    let mut total = 0_u64;
    let entries = std::fs::read_dir(directory).map_err(|_| IsolatedProcessFailure::Crash)?;
    for entry in entries {
        let entry = entry.map_err(|_| IsolatedProcessFailure::Crash)?;
        let file_type = entry
            .file_type()
            .map_err(|_| IsolatedProcessFailure::Crash)?;
        if file_type.is_symlink() {
            return Err(IsolatedProcessFailure::LimitExceeded);
        }
        if file_type.is_file() {
            let length = entry
                .metadata()
                .map_err(|_| IsolatedProcessFailure::Crash)?
                .len();
            total = total
                .checked_add(length)
                .ok_or(IsolatedProcessFailure::LimitExceeded)?;
            if total > max {
                return Err(IsolatedProcessFailure::LimitExceeded);
            }
        }
    }
    Ok(())
}

struct LogFiles {
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    stdout: Option<File>,
    stderr: Option<File>,
}

impl LogFiles {
    fn open(request: &IsolatedProcessRequest) -> Result<Self, IsolatedProcessFailure> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let stdout_path = request.private_profile_dir.join(format!(
            ".ottrin-process-stdout-{}-{unique}.log",
            std::process::id()
        ));
        let stderr_path = request.private_profile_dir.join(format!(
            ".ottrin-process-stderr-{}-{unique}.log",
            std::process::id()
        ));
        let stdout = private_log(&stdout_path)?;
        let stderr = match private_log(&stderr_path) {
            Ok(file) => file,
            Err(error) => {
                drop(stdout);
                let _ = std::fs::remove_file(&stdout_path);
                return Err(error);
            }
        };
        Ok(Self {
            stdout_path,
            stderr_path,
            stdout: Some(stdout),
            stderr: Some(stderr),
        })
    }

    fn stdout(&self) -> &File {
        self.stdout.as_ref().expect("writer is open")
    }

    fn stderr(&self) -> &File {
        self.stderr.as_ref().expect("writer is open")
    }

    fn close_writers(&mut self) {
        drop(self.stdout.take());
        drop(self.stderr.take());
    }
}

impl Drop for LogFiles {
    fn drop(&mut self) {
        self.close_writers();
        let _ = std::fs::remove_file(&self.stdout_path);
        let _ = std::fs::remove_file(&self.stderr_path);
    }
}

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn new(raw: HANDLE) -> Result<Self, IsolatedProcessFailure> {
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            Err(IsolatedProcessFailure::Unavailable)
        } else {
            Ok(Self(raw))
        }
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: `OwnedHandle` is the sole owner and closes exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct InheritanceGuard {
    handles: Vec<HANDLE>,
}

impl InheritanceGuard {
    fn new(handles: &[HANDLE]) -> Result<Self, IsolatedProcessFailure> {
        let mut enabled = Vec::with_capacity(handles.len());
        for handle in handles {
            // SAFETY: each raw handle comes from a live `File` kept through process creation.
            if unsafe { SetHandleInformation(*handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) }
                == 0
            {
                for prior in &enabled {
                    // SAFETY: rollback touches only handles enabled by this constructor.
                    unsafe {
                        let _ = SetHandleInformation(*prior, HANDLE_FLAG_INHERIT, 0);
                    }
                }
                return Err(IsolatedProcessFailure::Unavailable);
            }
            enabled.push(*handle);
        }
        Ok(Self { handles: enabled })
    }
}

impl Drop for InheritanceGuard {
    fn drop(&mut self) {
        for handle in &self.handles {
            // SAFETY: the parent still owns every listed `File` handle.
            unsafe {
                let _ = SetHandleInformation(*handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

struct AttributeList<'a> {
    storage: Vec<usize>,
    raw: LPPROC_THREAD_ATTRIBUTE_LIST,
    handles: &'a [HANDLE],
    jobs: &'a [HANDLE],
}

impl<'a> AttributeList<'a> {
    fn new(handles: &'a [HANDLE], jobs: &'a [HANDLE]) -> Result<Self, IsolatedProcessFailure> {
        let mut bytes = 0;
        // SAFETY: the documented sizing call writes only the required byte count.
        unsafe {
            let _ = InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(IsolatedProcessFailure::Unavailable);
        }
        let words = bytes.div_ceil(size_of::<usize>());
        let mut storage = vec![0_usize; words];
        let raw = storage.as_mut_ptr().cast::<c_void>();
        // SAFETY: `storage` is aligned and sized from the API's sizing call.
        if unsafe { InitializeProcThreadAttributeList(raw, 2, 0, &mut bytes) } == 0 {
            return Err(IsolatedProcessFailure::Unavailable);
        }
        let list = Self {
            storage,
            raw,
            handles,
            jobs,
        };
        // SAFETY: UpdateProcThreadAttribute borrows value buffers until the list
        // is deleted. The references in `list` keep both caller-owned arrays
        // alive through CreateProcessW and DeleteProcThreadAttributeList.
        if unsafe {
            UpdateProcThreadAttribute(
                list.raw,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                list.handles.as_ptr().cast(),
                size_of_val(list.handles),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(IsolatedProcessFailure::Unavailable);
        }
        // SAFETY: the Job Object value buffer has the same borrowed lifetime
        // as the inherited-handle buffer above.
        if unsafe {
            UpdateProcThreadAttribute(
                list.raw,
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                list.jobs.as_ptr().cast(),
                size_of_val(list.jobs),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(IsolatedProcessFailure::Unavailable);
        }
        Ok(list)
    }

    fn raw(&self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        debug_assert!(!self.storage.is_empty());
        self.raw
    }
}

impl Drop for AttributeList<'_> {
    fn drop(&mut self) {
        // SAFETY: the list was initialized exactly once and storage is still live.
        unsafe {
            DeleteProcThreadAttributeList(self.raw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_string(command: Vec<u16>) -> String {
        String::from_utf16(&command[..command.len() - 1]).expect("valid fixture command line")
    }

    #[test]
    fn command_line_quotes_spaces_quotes_and_trailing_backslashes() {
        let command = command_line(
            OsStr::new(r"C:\Program Files\LibreOffice\program\soffice.exe"),
            &[
                OsString::from("--headless"),
                OsString::from(r#"C:\input files\fixture"quoted".docx"#),
                OsString::from("C:\\output dir\\"),
            ],
        );

        assert_eq!(
            as_string(command),
            r#""C:\Program Files\LibreOffice\program\soffice.exe" --headless "C:\input files\fixture\"quoted\".docx" "C:\output dir\\""#
        );
    }

    #[test]
    fn logical_input_must_be_present_in_the_child_arguments() {
        let request = IsolatedProcessRequest {
            executable: PathBuf::from(r"C:\LibreOffice\soffice.exe"),
            arguments: vec![OsString::from("--version")],
            input: Some(crate::office_provider::IsolatedProcessInput {
                host_path: PathBuf::from(r"C:\input\source.docx"),
                sandbox_path: PathBuf::from("/input/source.docx"),
            }),
            private_profile_dir: PathBuf::from(r"C:\profile"),
            private_output_dir: PathBuf::from(r"C:\output"),
            timeout: Duration::from_secs(1),
            max_memory_bytes: 1,
            max_processes: 1,
            max_output_bytes: 1,
        };

        assert_eq!(
            translated_arguments(&request),
            Err(IsolatedProcessFailure::InvalidRequest)
        );
    }
}
