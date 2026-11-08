//! One invocation, one local endpoint. No request/response files or shell command.
//! The caller holds the launched process handle and authenticates its pipe PID
//! before sending authority-bearing request bytes. Kernel handles own teardown.
use crate::windows_pipe::{LocalPipeListener, LocalPipeStream};
use crate::{PlatformError, helper_protocol};
use ottrin_core::{PrivilegedRequest, PrivilegedResponse, PrivilegedStatus};
use serde::{Serialize, de::DeserializeOwned};
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    ERROR_CANCELLED, FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY};
use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessId, GetProcessTimes, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::{
    FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC,
    SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, SHGetKnownFolderPath, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

mod copy_stream;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const PREFIX: &str = r"\\.\pipe\ottrin.priv.";

fn endpoint(nonce: &str) -> io::Result<String> {
    if nonce.len() != 32 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid helper pipe nonce",
        ));
    }
    Ok(format!("{PREFIX}{nonce}"))
}

fn verify_pid(actual: u32, expected: u32) -> io::Result<()> {
    if actual == expected && expected != 0 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Administrator pipe peer is not the expected process",
        ))
    }
}

fn write_json(stream: &mut LocalPipeStream, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    let size = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    stream.write_all(&size.to_be_bytes())?;
    stream.write_all(&bytes)
}

fn read_json<T: DeserializeOwned>(stream: &mut impl Read) -> io::Result<T> {
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    let mut frame = stream.take(u64::from(u32::from_be_bytes(header)));
    // Parse incrementally instead of allocating an untrusted advertised length.
    let value =
        serde_json::from_reader(std::io::BufReader::new(&mut frame)).map_err(io::Error::other)?;
    if frame.limit() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Incomplete helper frame",
        ));
    }
    Ok(value)
}

fn process_start_time(process: HANDLE) -> io::Result<u64> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: process is a borrowed live handle and all four outputs remain valid.
    if unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let value = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    if value == 0 {
        return Err(io::Error::other("Application process has no creation time"));
    }
    Ok(value)
}

fn process_token(process: &OwnedHandle, access: u32) -> io::Result<OwnedHandle> {
    let mut token = ptr::null_mut();
    // SAFETY: the retained process is queryable; success transfers one token handle.
    if unsafe { OpenProcessToken(process.as_raw_handle(), access, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(token) })
}

/// Query Windows for the explicit token's registered folder, without creating
/// it or consulting the helper's environment. The returned path is a locator,
/// not permission to adopt a recovery registry or replay serialized authority.
fn local_app_data(token: &OwnedHandle) -> io::Result<PathBuf> {
    struct Buffer(*mut u16);
    impl Drop for Buffer {
        fn drop(&mut self) {
            // SAFETY: SHGetKnownFolderPath requires this even on failure; null is OK.
            unsafe { CoTaskMemFree(self.0.cast()) };
        }
    }
    let mut buffer = Buffer(ptr::null_mut());
    // SAFETY: the live primary token has the documented access rights; Windows
    // owns a terminated UTF-16 allocation which Buffer releases on every path.
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            KF_FLAG_DONT_VERIFY as u32,
            token.as_raw_handle(),
            &mut buffer.0,
        )
    };
    if result < 0 {
        return Err(io::Error::other(format!(
            "Resolve original caller Local AppData: HRESULT {result:#x}"
        )));
    }
    if buffer.0.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows returned no caller profile path",
        ));
    }
    let mut length = 0;
    // SAFETY: successful SHGetKnownFolderPath returns a terminated allocation.
    while unsafe { *buffer.0.add(length) } != 0 {
        length += 1;
    }
    let path = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(buffer.0, length)
    }));
    if !path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows returned an ambiguous caller profile path",
        ));
    }
    Ok(path)
}

/// Retain the original process object and its primary token. The creation time
/// binds the launch-time identity even if the numeric PID was reused before the
/// helper opened it. TokenUser is independent of either side's default file owner.
struct CallerProcess {
    process: OwnedHandle,
    token: OwnedHandle,
    identity: CallerIdentity,
}

#[derive(PartialEq, Eq)]
struct CallerIdentity {
    user: Vec<u8>,
    owner: Vec<u8>,
    group: Vec<u8>,
}
impl CallerIdentity {
    fn capture(token: &OwnedHandle) -> io::Result<Self> {
        Ok(Self {
            user: crate::windows_fs::user_for_token(token).map_err(io::Error::other)?,
            owner: crate::windows_fs::owner_for_token(token).map_err(io::Error::other)?,
            group: crate::windows_fs::group_for_token(token).map_err(io::Error::other)?,
        })
    }
}

impl CallerProcess {
    fn capture(pid: u32, expected_start: u64) -> io::Result<Self> {
        if pid == 0 || expected_start == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid application process identity",
            ));
        }
        // SAFETY: no pointers are supplied; success transfers a non-inheritable handle.
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let process = unsafe { OwnedHandle::from_raw_handle(raw) };
        if process_start_time(process.as_raw_handle())? != expected_start {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Application process creation time does not match its launch identity",
            ));
        }
        let token = process_token(&process, TOKEN_QUERY)?;
        let identity = CallerIdentity::capture(&token)?;
        let caller = Self {
            process,
            token,
            identity,
        };
        caller.validate()?;
        Ok(caller)
    }
    fn local_app_data(&self) -> io::Result<PathBuf> {
        self.validate()?;
        // Additional token rights are requested only for profile lookup. Normal
        // helper admission still requires query access alone. No impersonation,
        // registry-hive loading, environment edits or directory creation occurs.
        let token = process_token(
            &self.process,
            TOKEN_QUERY | TOKEN_IMPERSONATE | TOKEN_DUPLICATE,
        )?;
        if CallerIdentity::capture(&token)? != self.identity {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Caller token user, default owner or primary group changed before profile lookup",
            ));
        }
        let path = local_app_data(&token)?;
        self.validate()?;
        Ok(path)
    }
    fn validate(&self) -> io::Result<()> {
        // SAFETY: capture retained the exact process with SYNCHRONIZE rights.
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => {}
            WAIT_OBJECT_0 => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Original application process has exited",
                ));
            }
            _ => return Err(io::Error::last_os_error()),
        }
        if CallerIdentity::capture(&self.token)? != self.identity
            || CallerIdentity::capture(&process_token(&self.process, TOKEN_QUERY)?)?
                != self.identity
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Original application token user, default owner or primary group changed",
            ));
        }
        Ok(())
    }
}

/// Authenticated original-caller context pins the profile and distinct native
/// token principals. Private entry creation is a primitive for the helper's
/// transaction engine; it does not admit recovery history or authorize Copy,
/// Undo or cleanup without that engine's namespace/journal validation.
pub struct CallerCopyContext {
    caller: Arc<CallerProcess>,
    profile: crate::copy_transaction::CallerProfile,
}
impl CallerCopyContext {
    fn capture(caller: Arc<CallerProcess>) -> io::Result<Self> {
        let path = caller.local_app_data()?;
        let profile = crate::copy_transaction::CallerProfile::capture(
            &path,
            &caller.identity.user,
            &caller.identity.owner,
        )
        .map_err(io::Error::other)?;
        let context = Self { caller, profile };
        context.validate()?;
        Ok(context)
    }
    pub fn validate(&self) -> io::Result<()> {
        self.caller.validate()?;
        self.profile.validate().map_err(io::Error::other)?;
        self.caller.validate()
    }
    /// Canonical caller profile registry. New provisioning requires a nonempty
    /// source selection and refuses native overlap before any directory write.
    /// Existing registry discovery passes an empty selection with initialize=false.
    pub fn copy_registry(
        self: &Arc<Self>,
        sources: &[PathBuf],
        initialize: bool,
    ) -> io::Result<Option<crate::WindowsCopyRegistry>> {
        crate::copy_transaction::WindowsCopyRegistry::for_caller_profile(
            Arc::clone(self),
            sources,
            initialize,
        )
        .map_err(io::Error::other)
    }
    /// Execute one authenticated Copy through the shared conflict/staging engine.
    /// The returned receipt is admitted using this original caller, never the
    /// helper's ambient profile. This does not enable advertised administrator Copy.
    pub fn copy_entry_with_progress(
        self: &Arc<Self>,
        source: &Path,
        destination: &Path,
        conflict: crate::ConflictAction,
        max_entries: usize,
        progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Option<crate::PathMoveUndoReceipt>, PlatformError> {
        crate::copy_transaction::copy_for_caller(
            Arc::clone(self),
            source,
            destination,
            conflict,
            max_entries,
            progress,
        )
    }
    pub(crate) fn registry_parent(
        &self,
        initialize: bool,
    ) -> io::Result<Option<crate::copy_transaction::CallerRegistryParent>> {
        self.validate()?;
        let parent = self
            .profile
            .registry_parent(
                &self.caller.identity.user,
                &self.caller.identity.owner,
                initialize,
                |parent, name| {
                    self.create_private_entry(
                        parent,
                        name,
                        true,
                        windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
                            | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE
                            | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE,
                    )
                },
            )
            .map_err(io::Error::other)?;
        self.validate()?;
        Ok(parent)
    }
    pub(crate) fn validate_registry_parent(
        &self,
        parent: &crate::copy_transaction::CallerRegistryParent,
    ) -> io::Result<()> {
        self.validate()?;
        parent.validate(&self.profile).map_err(io::Error::other)?;
        self.validate()
    }
    /// Open the fixed Copy registry under an independently admitted parent.
    /// The returned holder retains this authenticated context for every native
    /// validation/reopen. This does not select the profile ancestry or admit
    /// transaction journals; the engine must establish those before mutation.
    pub fn copy_registry_under(
        self: &Arc<Self>,
        parent: std::fs::File,
        initialize: bool,
    ) -> io::Result<Option<crate::WindowsCopyRegistry>> {
        self.validate()?;
        crate::copy_transaction::WindowsCopyRegistry::for_caller(
            parent,
            Arc::clone(self),
            initialize,
        )
        .map_err(io::Error::other)
    }
    pub fn local_app_data(&self) -> io::Result<&Path> {
        self.validate()?;
        Ok(self.profile.path())
    }
    pub fn user_sid(&self) -> io::Result<&[u8]> {
        self.validate()?;
        Ok(&self.caller.identity.user)
    }
    pub fn default_file_owner_sid(&self) -> io::Result<&[u8]> {
        self.validate()?;
        Ok(&self.caller.identity.owner)
    }
    pub fn default_file_group_sid(&self) -> io::Result<&[u8]> {
        self.validate()?;
        Ok(&self.caller.identity.group)
    }
    /// Create one caller-owned private entry relative to an already admitted
    /// parent handle. The caller supplies neither a SID nor saved permissions.
    /// The transaction engine must journal its intent before invoking this.
    pub fn create_private_entry(
        &self,
        parent: &std::fs::File,
        name: &OsStr,
        directory: bool,
        sharing: u32,
    ) -> io::Result<std::fs::File> {
        let policy = self.private_creation_policy(directory)?;
        #[cfg(test)]
        crate::copy_transaction::caller_private_create_boundary(false)?;
        let file = policy.create(parent, name, sharing)?;
        self.validate()?;
        #[cfg(test)]
        crate::copy_transaction::caller_private_create_boundary(true)?;
        Ok(file)
    }
    pub(crate) fn create_temporary_control(
        &self,
        parent: &std::fs::File,
        name: &OsStr,
    ) -> io::Result<std::fs::File> {
        let policy = self.private_creation_policy(false)?;
        let file = policy.create_temporary(parent, name)?;
        self.validate()?;
        Ok(file)
    }
    pub(crate) fn create_private_candidate(
        &self,
        parent: &std::fs::File,
        name: &OsStr,
        directory: bool,
        sharing: u32,
    ) -> io::Result<crate::windows_fs::PerObjectFile> {
        let policy = self.private_creation_policy(directory)?;
        let file = policy.create_candidate(parent, name, sharing)?;
        self.validate()?;
        Ok(file)
    }
    fn private_creation_policy(
        &self,
        directory: bool,
    ) -> io::Result<crate::copy_transaction::CallerCreationSecurity> {
        let token = self.snapshot_token()?;
        let policy = crate::copy_transaction::CallerCreationSecurity::capture(
            &token,
            &self.caller.identity.owner,
            &self.caller.identity.group,
            directory,
        )
        .map_err(io::Error::other)?;
        self.validate()?;
        Ok(policy)
    }
    /// Apply a required naming policy to the engine's newly created, empty
    /// private directory. The engine must establish create-new ownership and
    /// journal intent; this method does not admit arbitrary retained candidates.
    pub fn set_private_directory_case_policy(
        &self,
        directory: &std::fs::File,
        expected: crate::move_recovery::WindowsStableIdentity,
        case_sensitive: bool,
    ) -> io::Result<()> {
        self.with_caller_token(|| {
            crate::copy_transaction::set_caller_private_directory_case_policy(
                directory,
                expected,
                &self.caller.identity.owner,
                &self.caller.identity.group,
                case_sensitive,
            )
        })
    }
    /// Duplicate the kernel-bound original token and check the immutable
    /// admitted principals. Derivation uses this snapshot, never helper policy.
    pub(crate) fn snapshot_token(&self) -> io::Result<OwnedHandle> {
        self.validate()?;
        let source = process_token(&self.caller.process, TOKEN_QUERY | TOKEN_DUPLICATE)?;
        let mut raw = ptr::null_mut();
        if unsafe {
            windows_sys::Win32::Security::DuplicateTokenEx(
                source.as_raw_handle().cast(),
                TOKEN_QUERY | TOKEN_IMPERSONATE,
                ptr::null(),
                windows_sys::Win32::Security::SecurityImpersonation,
                windows_sys::Win32::Security::TokenImpersonation,
                &mut raw,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let token = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        if CallerIdentity::capture(&token)? != self.caller.identity {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Caller token changed before Copy policy snapshot",
            ));
        }
        self.validate()?;
        Ok(token)
    }
    /// A required native operation may need the original effective subject.
    /// Restore the exact prior thread token on every exit.
    fn with_caller_token<T>(&self, operation: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        let token = self.snapshot_token()?;
        let result = crate::windows_fs::with_thread_token(&token, operation);
        self.validate()?;
        result
    }
    /// Reopen one private entry whose native identity the transaction engine
    /// independently admitted. A serialized identity alone is not admission.
    /// This checks caller/profile liveness, identity/type and private owner/group/
    /// DACL policy; registry anchors, journal contents, links and streams still
    /// require the engine's normal validation before any mutation or cleanup.
    pub fn open_private_entry(
        &self,
        parent: &std::fs::File,
        name: &OsStr,
        expected: crate::move_recovery::WindowsStableIdentity,
        writable: bool,
        sharing: u32,
    ) -> io::Result<std::fs::File> {
        self.validate()?;
        let file = crate::copy_transaction::open_caller_private_entry(
            parent,
            name,
            expected,
            &self.caller.identity.owner,
            &self.caller.identity.group,
            writable,
            sharing,
        )?;
        self.validate()?;
        Ok(file)
    }
}

/// Helper-side connection. Bind the kernel application identity before reading
/// request bytes; no caller SID, profile or elevated environment is trusted.
pub struct HelperChannel {
    stream: LocalPipeStream,
    caller: Arc<CallerProcess>,
}
impl HelperChannel {
    pub fn connect(nonce: &str, parent_pid: u32, parent_start: u64) -> io::Result<Self> {
        let name = endpoint(nonce)?;
        let caller = Arc::new(CallerProcess::capture(parent_pid, parent_start)?);
        let stream = LocalPipeStream::connect(&name, CONNECT_TIMEOUT, |pid| {
            verify_pid(pid, parent_pid)?;
            caller.validate()
        })?;
        Ok(Self { stream, caller })
    }
    pub fn read_request(&mut self) -> Result<PrivilegedRequest, String> {
        self.caller.validate().map_err(|error| error.to_string())?;
        let request: PrivilegedRequest =
            read_json(&mut self.stream).map_err(|error| error.to_string())?;
        self.caller.validate().map_err(|error| error.to_string())?;
        request.validate_protocol().map_err(str::to_owned)?;
        Ok(request)
    }
    /// Route Copy commands only through the kernel-authenticated original caller.
    /// Capturing once per request keeps the same authority for the entire batch.
    /// Feature negotiation remains disabled until package/UAC acceptance passes.
    pub fn execute_copy_command(
        &self,
        command: &ottrin_core::PrivilegedCommand,
    ) -> Option<PrivilegedResponse> {
        self.execute_copy_command_with_progress(command, &mut |_| Ok(()))
    }
    /// Forward progress/cancellation into both admission and filesystem work.
    /// The callback does not supply authority; one fresh kernel-authenticated
    /// caller context still owns this entire request.
    pub fn execute_copy_command_with_progress(
        &self,
        command: &ottrin_core::PrivilegedCommand,
        progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
    ) -> Option<PrivilegedResponse> {
        if !command.requires_copy_recovery() {
            return None;
        }
        Some(execute_copy_for_caller(&self.caller, command, progress))
    }
    pub fn caller_copy_context(&self) -> io::Result<CallerCopyContext> {
        CallerCopyContext::capture(Arc::clone(&self.caller))
    }
    /// Serve one Copy operation's progress and terminal response on this same
    /// authenticated connection. A Stop byte never supplies caller authority.
    pub fn serve_copy_command(
        &mut self,
        command: &ottrin_core::PrivilegedCommand,
    ) -> Option<io::Result<()>> {
        if !command.requires_copy_recovery() {
            return None;
        }
        let caller = Arc::clone(&self.caller);
        Some(copy_stream::serve(&mut self.stream, |progress| {
            execute_copy_for_caller(&caller, command, progress)
        }))
    }
    pub fn write_response(&mut self, response: &PrivilegedResponse) -> io::Result<()> {
        write_json(&mut self.stream, response)
    }
}

fn execute_copy_for_caller(
    caller: &Arc<CallerProcess>,
    command: &ottrin_core::PrivilegedCommand,
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
) -> PrivilegedResponse {
    match CallerCopyContext::capture(Arc::clone(caller)) {
        Ok(caller) => crate::privileged_copy::execute(Arc::new(caller), command, progress),
        Err(error) => PrivilegedResponse {
            status: PrivilegedStatus::Failed,
            message: Some(error.to_string()),
            payload: None,
        },
    }
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut value = value.encode_wide().collect::<Vec<_>>();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in helper launch argument",
        ));
    }
    value.push(0);
    Ok(value)
}

struct ComApartment(bool);
impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances only this thread's successful initialization.
            unsafe { CoUninitialize() };
        }
    }
}

fn launch(helper: &Path, nonce: &str) -> io::Result<OwnedHandle> {
    // SAFETY: initialize COM on the invoking thread; an existing apartment is OK.
    let initialized = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    // RPC_E_CHANGED_MODE: the caller already owns a different COM apartment.
    if initialized < 0 && initialized != 0x80010106u32 as i32 {
        return Err(io::Error::other(format!(
            "Initialize UAC launcher: HRESULT {initialized:#x}"
        )));
    }
    let _apartment = ComApartment(initialized >= 0);
    let helper = wide(helper.as_os_str())?;
    let verb = wide(OsStr::new("runas"))?;
    // Only validated hexadecimal and decimal values enter lpParameters. The
    // executable path is a separate native UTF-16 field, including spaces.
    endpoint(nonce)?;
    let args = wide(OsStr::new(&format!(
        "--local-pipe {nonce} --parent-pid {} --parent-start {}",
        std::process::id(),
        process_start_time(unsafe { GetCurrentProcess() })?
    )))?;
    // SAFETY: zero initializes optional ShellExecute fields; pointers stay live.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = helper.as_ptr();
    info.lpParameters = args.as_ptr();
    info.nShow = SW_HIDE;
    // SAFETY: all string fields are terminated; Windows supplies the process handle.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.hProcess.is_null() {
        return Err(io::Error::other(
            "Windows returned no administrator helper process handle",
        ));
    }
    // SAFETY: ShellExecute transferred this non-null handle to the caller.
    Ok(unsafe { OwnedHandle::from_raw_handle(info.hProcess) })
}

struct CopyInvocation {
    stream: Option<LocalPipeStream>,
    process: Option<OwnedHandle>,
}

impl CopyInvocation {
    fn finish(&mut self) -> io::Result<()> {
        // EOF reaches the helper's independent cancellation reader before we
        // wait, including when the application callback is unwinding.
        drop(self.stream.take());
        if let Some(process) = self.process.take() {
            // SAFETY: ShellExecute transferred this retained synchronize handle;
            // it stays live through the wait, preventing PID reuse.
            if unsafe { WaitForSingleObject(process.as_raw_handle(), u32::MAX) } != WAIT_OBJECT_0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

impl Drop for CopyInvocation {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            tracing::error!(%error, "Could not observe administrator Copy helper exit");
        }
    }
}

pub(crate) fn execute(
    helper: &str,
    copy_recovery: bool,
    request: &PrivilegedRequest,
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
) -> Result<PrivilegedResponse, PlatformError> {
    progress(0)?;
    let helper = helper_protocol::check(helper, copy_recovery)
        .map_err(PlatformError::PrivilegedHelperUnavailable)?;
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|error| PlatformError::Io(error.to_string()))?;
    let nonce = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    // Owner + elevated administrators (including over-the-shoulder UAC) + SYSTEM.
    // The ACL is only an admission boundary: the actual launched PID is mandatory.
    let mut listener = LocalPipeListener::bind(
        &endpoint(&nonce).map_err(io_error)?,
        "D:P(A;;GA;;;OW)(A;;GA;;;BA)(A;;GA;;;SY)",
    )
    .map_err(io_error)?;
    let process = match launch(&helper, &nonce) {
        Ok(process) => process,
        Err(error) if error.raw_os_error() == Some(ERROR_CANCELLED as i32) => {
            return Ok(PrivilegedResponse {
                status: PrivilegedStatus::Denied,
                message: Some("Administrator permission request was cancelled.".into()),
                payload: None,
            });
        }
        Err(error) => return Err(io_error(error)),
    };
    // Holding this process handle prevents PID reuse until peer authentication.
    let pid = unsafe { GetProcessId(process.as_raw_handle()) };
    if pid == 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let mut stream = loop {
        if let Some(stream) = listener
            .accept(Duration::from_millis(100), |peer| verify_pid(peer, pid))
            .map_err(io_error)?
        {
            break stream;
        }
        // SAFETY: the retained process handle is valid and this is a nonblocking poll.
        match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => {
                return Err(io_error(io::Error::other(
                    "Administrator helper exited before connecting",
                )));
            }
            WAIT_TIMEOUT => {}
            _ => return Err(io_error(io::Error::last_os_error())),
        }
        if Instant::now() >= deadline {
            return Err(io_error(io::Error::new(
                io::ErrorKind::TimedOut,
                "Administrator helper did not connect",
            )));
        }
    };
    // File operations can legitimately take a long time. A disconnected/crashed
    // helper closes the pipe immediately; elapsed operation time is not a failure.
    if request.command.requires_copy_recovery() {
        let mut invocation = CopyInvocation {
            stream: Some(stream),
            process: Some(process),
        };
        let response =
            copy_stream::exchange(invocation.stream.as_mut().unwrap(), request, progress);
        return match (response, invocation.finish()) {
            (outcome, Ok(())) => outcome,
            (Ok(mut response), Err(error)) => {
                copy_stream::response_error(&mut response, error);
                Ok(response)
            }
            (Err(operation), Err(exit)) => Err(PlatformError::Io(format!(
                "{operation}; helper exit: {exit}"
            ))),
        };
    }
    write_json(&mut stream, request).map_err(io_error)?;
    stream.set_io_timeout(Duration::MAX);
    read_json(&mut stream).map_err(|error| PlatformError::Io(format!("Administrator helper response failed: {error}. The operation may have changed files; review its destination and recovery history before retrying.")))
}

fn io_error(error: io::Error) -> PlatformError {
    PlatformError::Io(format!("Administrator helper connection failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottrin_core::{PrivilegedCommand, PrivilegedContext, PrivilegedPayload};
    mod caller_owner;

    const ACL: &str = "D:P(A;;GA;;;OW)(A;;GA;;;BA)(A;;GA;;;SY)";

    fn nonce() -> String {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).unwrap();
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn windows_helper_pipe_binds_creation_time_before_connecting() {
        let nonce = nonce();
        let mut listener = LocalPipeListener::bind(&endpoint(&nonce).unwrap(), ACL).unwrap();
        let started = process_start_time(unsafe { GetCurrentProcess() }).unwrap();
        let error = match HelperChannel::connect(&nonce, std::process::id(), started + 1) {
            Err(error) => error,
            _ => panic!("Wrong process generation admitted"),
        };
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(
            listener
                .accept(Duration::from_millis(20), |_| Ok(()))
                .unwrap()
                .is_none()
        );
        let mut channel = HelperChannel::connect(&nonce, std::process::id(), started).unwrap();
        let mut server = listener
            .accept(Duration::from_secs(1), |pid| {
                verify_pid(pid, std::process::id())
            })
            .unwrap()
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let request = PrivilegedRequest {
            protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
            command: PrivilegedCommand::ListDirectory {
                path: directory.path().into(),
                show_hidden: false,
            },
            context: PrivilegedContext {
                reason: None,
                cwd: None,
            },
        };
        write_json(&mut server, &request).unwrap();
        assert_eq!(
            serde_json::to_value(channel.read_request().unwrap()).unwrap(),
            serde_json::to_value(request).unwrap()
        );
    }

    #[test]
    fn windows_helper_pipe_retains_original_process_and_refuses_its_exit() {
        let command = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/cmd.exe");
        let mut child = Child(
            std::process::Command::new(command)
                .args(["/d", "/q", "/c", "set /p OTTRIN_QA_WAIT="])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let started = process_start_time(child.0.as_raw_handle()).unwrap();
        let caller = CallerProcess::capture(child.0.id(), started).unwrap();
        caller.validate().unwrap();
        let user = crate::windows_fs::user_for_token(&caller.token).unwrap();
        assert!(crate::windows_fs::valid_sid(&user));
        // Existing file-owner admission still reads its own TokenOwner contract.
        assert!(crate::windows_fs::valid_sid(
            &crate::windows_fs::current_owner().unwrap()
        ));
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert_eq!(
            process_start_time(caller.process.as_raw_handle()).unwrap(),
            started
        );
        assert_eq!(
            crate::windows_fs::user_for_token(&caller.token).unwrap(),
            user
        );
        assert_eq!(
            caller.validate().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    #[ignore = "Owned subprocess entry; exercised by caller_profile_uses_kernel_token_not_environment"]
    fn caller_profile_worker() {
        let parent = std::env::var("OTTRIN_PROFILE_PARENT")
            .unwrap()
            .parse()
            .unwrap();
        let start = std::env::var("OTTRIN_PROFILE_START")
            .unwrap()
            .parse()
            .unwrap();
        let mut channel = HelperChannel::connect(
            &std::env::var("OTTRIN_PROFILE_NONCE").unwrap(),
            parent,
            start,
        )
        .unwrap();
        channel.read_request().unwrap();
        let context = channel.caller_copy_context().unwrap();
        let path = context.local_app_data().unwrap();
        write_json(&mut channel.stream, &path).unwrap();
    }

    #[test]
    fn caller_profile_native_pin_refuses_replacement_and_permission_changes() {
        let started = process_start_time(unsafe { GetCurrentProcess() }).unwrap();
        let caller = Arc::new(CallerProcess::capture(std::process::id(), started).unwrap());
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("profile Ω");
        std::fs::create_dir(&path).unwrap();
        // Only this private fixture substitutes a path. Production construction
        // always resolves the authenticated token's registered known folder.
        let context = CallerCopyContext {
            profile: crate::copy_transaction::CallerProfile::capture(
                &path,
                &caller.identity.user,
                &caller.identity.owner,
            )
            .unwrap(),
            caller,
        };
        context.validate().unwrap();
        std::fs::write(path.join("untouched.txt"), b"caller data").unwrap();
        context.validate().unwrap(); // Normal profile content is not frozen.
        let displaced = root.path().join("original");
        std::fs::rename(&path, &displaced).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(context.local_app_data().is_err());
        assert_eq!(
            std::fs::read(displaced.join("untouched.txt")).unwrap(),
            b"caller data"
        );
        std::fs::remove_dir(&path).unwrap();
        std::fs::rename(&displaced, &path).unwrap();
        context.validate().unwrap();

        // Change only this owned fixture's real DACL, retaining its owner and
        // contents. Use a live native descriptor, never serialized ACL bytes.
        use windows_sys::Win32::Security::{Authorization::*, *};
        let policy = crate::windows_fs::LocalSecurity::private().unwrap();
        let (mut present, mut defaulted, mut dacl) = (0, 0, ptr::null_mut());
        assert_ne!(
            unsafe {
                GetSecurityDescriptorDacl(policy.as_raw(), &mut present, &mut dacl, &mut defaulted)
            },
            0
        );
        assert_ne!(present, 0);
        let file = crate::windows_fs::open_directory(
            &path,
            windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED,
        )
        .unwrap();
        assert_eq!(
            unsafe {
                SetSecurityInfo(
                    file.as_raw_handle().cast(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    dacl,
                    ptr::null_mut(),
                )
            },
            0
        );
        assert!(context.validate().is_err());
        assert_eq!(
            std::fs::read(path.join("untouched.txt")).unwrap(),
            b"caller data"
        );
    }

    #[test]
    fn caller_profile_uses_kernel_token_not_environment() {
        let started = process_start_time(unsafe { GetCurrentProcess() }).unwrap();
        let current = CallerProcess::capture(std::process::id(), started).unwrap();
        let token = process_token(
            &current.process,
            TOKEN_QUERY | TOKEN_IMPERSONATE | TOKEN_DUPLICATE,
        )
        .unwrap();
        let expected = local_app_data(&token).unwrap();
        let root = tempfile::tempdir().unwrap();
        let false_profile = root.path().join("not the profile Ω");
        let nonce = nonce();
        let mut listener = LocalPipeListener::bind(&endpoint(&nonce).unwrap(), ACL).unwrap();
        let started = process_start_time(unsafe { GetCurrentProcess() }).unwrap();
        let mut worker = Child(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "windows_privileged_transport::tests::caller_profile_worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("OTTRIN_PROFILE_PARENT", std::process::id().to_string())
                .env("OTTRIN_PROFILE_START", started.to_string())
                .env("OTTRIN_PROFILE_NONCE", &nonce)
                .env("LOCALAPPDATA", &false_profile)
                .env("USERPROFILE", &false_profile)
                .spawn()
                .unwrap(),
        );
        let mut server = listener
            .accept(Duration::from_secs(10), |pid| {
                verify_pid(pid, worker.0.id())
            })
            .unwrap()
            .unwrap();
        let request = PrivilegedRequest {
            protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
            command: PrivilegedCommand::ListDirectory {
                path: root.path().into(),
                show_hidden: false,
            },
            context: PrivilegedContext {
                reason: None,
                cwd: None,
            },
        };
        write_json(&mut server, &request).unwrap();
        assert_eq!(read_json::<PathBuf>(&mut server).unwrap(), expected);
        assert!(worker.0.wait().unwrap().success());
        assert!(
            !false_profile.exists(),
            "Profile resolution must not create a redirected directory"
        );

        // The original process may also have a false environment. Only its
        // retained primary token selects the profile, and exit revokes lookup.
        let command =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe");
        let mut original = Child(
            std::process::Command::new(command)
                .args(["/d", "/q", "/c", "set /p OTTRIN_QA_WAIT="])
                .env("LOCALAPPDATA", &false_profile)
                .env("USERPROFILE", &false_profile)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let started = process_start_time(original.0.as_raw_handle()).unwrap();
        let caller = Arc::new(CallerProcess::capture(original.0.id(), started).unwrap());
        let context = CallerCopyContext::capture(Arc::clone(&caller)).unwrap();
        assert_eq!(context.local_app_data().unwrap(), expected);
        assert_eq!(
            context.user_sid().unwrap(),
            crate::windows_fs::user_for_token(&caller.token).unwrap()
        );
        assert_eq!(
            context.default_file_owner_sid().unwrap(),
            crate::windows_fs::owner_for_token(&caller.token).unwrap()
        );
        assert!(!false_profile.exists());
        original.0.kill().unwrap();
        original.0.wait().unwrap();
        assert_eq!(
            context.local_app_data().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        println!(
            "OTTRIN_CALLER_PROFILE={}",
            serde_json::to_string(&expected).unwrap()
        );
    }

    #[test]
    fn windows_helper_pipe_rejects_wrong_peer_before_receiving_authority() {
        let name = endpoint(&nonce()).unwrap();
        let mut listener = LocalPipeListener::bind(&name, ACL).unwrap();
        assert!(LocalPipeListener::bind(&name, ACL).is_err());
        let mut client = LocalPipeStream::connect(&name, Duration::from_secs(1), |pid| {
            verify_pid(pid, std::process::id())
        })
        .unwrap();
        let error = match listener.accept(Duration::from_secs(1), |pid| verify_pid(pid, 0)) {
            Err(error) => error,
            _ => panic!("Unexpected peer admitted"),
        };
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        let mut byte = [0];
        assert_eq!(client.read(&mut byte).unwrap(), 0);
        drop(client);
        drop(listener);
        assert!(LocalPipeStream::connect(&name, Duration::from_millis(20), |_| Ok(())).is_err());
    }

    #[test]
    fn windows_helper_pipe_truncated_frame_is_not_a_request() {
        let name = endpoint(&nonce()).unwrap();
        let mut listener = LocalPipeListener::bind(&name, ACL).unwrap();
        let mut client =
            LocalPipeStream::connect(&name, Duration::from_secs(1), |_| Ok(())).unwrap();
        let mut server = listener
            .accept(Duration::from_secs(1), |_| Ok(()))
            .unwrap()
            .unwrap();
        client.write_all(&100u32.to_be_bytes()).unwrap();
        client.write_all(b"{}").unwrap();
        drop(client);
        assert!(read_json::<PrivilegedRequest>(&mut server).is_err());
    }

    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                self.0.kill().expect("Stop owned read-only helper fixture");
            }
            self.0.wait().expect("Reap owned helper fixture");
        }
    }

    #[test]
    #[ignore = "requires the owned native helper executable in a Unicode/spaced package path"]
    fn windows_helper_pipe_native_helper_round_trip_and_parent_rejection() {
        let helper = std::env::var_os("OTTRIN_QA_PRIV_HELPER").expect("Owned helper fixture path");
        assert!(helper.to_string_lossy().contains(' '));
        assert!(!helper.to_string_lossy().is_ascii());
        let version = ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION.to_string();
        for (arguments, expected) in [
            (vec!["--check-protocol", &version, "--bound-local-pipe"], 0),
            (vec!["--check-protocol", &version, "--local-pipe"], 69),
            (vec!["--stdio-json"], 2),
            (
                vec![
                    "--local-pipe",
                    "0123456789abcdef0123456789abcdef",
                    "--parent-pid",
                    "1",
                ],
                2,
            ),
        ] {
            let output = std::process::Command::new(&helper)
                .args(&arguments)
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(expected),
                "{arguments:?}: {output:?}"
            );
        }
        let dir = tempfile::tempdir().unwrap();
        let item = dir.path().join("visible Å.txt");
        std::fs::write(&item, b"owner data").unwrap();
        for (wrong_parent, wrong_start) in [(false, false), (true, false), (false, true)] {
            let nonce = nonce();
            let name = endpoint(&nonce).unwrap();
            let mut listener = LocalPipeListener::bind(&name, ACL).unwrap();
            let mut child = Child(
                std::process::Command::new(&helper)
                    .args([
                        "--local-pipe",
                        &nonce,
                        "--parent-pid",
                        &if wrong_parent {
                            std::process::id() + 1
                        } else {
                            std::process::id()
                        }
                        .to_string(),
                        "--parent-start",
                        &(process_start_time(unsafe { GetCurrentProcess() }).unwrap()
                            + u64::from(wrong_start))
                        .to_string(),
                    ])
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(10);
            let stream = loop {
                if let Some(stream) = listener
                    .accept(Duration::from_millis(100), |pid| {
                        verify_pid(pid, child.0.id())
                    })
                    .unwrap()
                {
                    break Some(stream);
                }
                assert!(Instant::now() < deadline, "Owned helper did not connect");
                if let Some(status) = child.0.try_wait().unwrap() {
                    // A rejecting client may close before ConnectNamedPipe
                    // observes it. That is refusal, not a failed connection test.
                    assert!(wrong_parent || wrong_start);
                    assert_eq!(status.code(), Some(1));
                    break None;
                }
            };
            if let Some(mut stream) = stream {
                if wrong_parent || wrong_start {
                    let mut byte = [0];
                    assert_eq!(stream.read(&mut byte).unwrap(), 0);
                } else {
                    let request = PrivilegedRequest {
                        protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
                        command: PrivilegedCommand::ListDirectory {
                            path: dir.path().into(),
                            show_hidden: false,
                        },
                        context: PrivilegedContext {
                            reason: None,
                            cwd: None,
                        },
                    };
                    write_json(&mut stream, &request).unwrap();
                    let response: PrivilegedResponse = read_json(&mut stream).unwrap();
                    assert_eq!(response.status, PrivilegedStatus::Success);
                    let Some(PrivilegedPayload::Entries(entries)) = response.payload else {
                        panic!("Missing directory entries")
                    };
                    assert_eq!(entries.len(), 1);
                    assert_eq!(entries[0].path, item);
                }
                drop(stream);
            }
            drop(listener);
            while child.0.try_wait().unwrap().is_none() {
                assert!(
                    Instant::now() < deadline,
                    "Helper retained a pipe after response"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                LocalPipeStream::connect(&name, Duration::from_millis(20), |_| Ok(())).is_err()
            );
            assert_eq!(std::fs::read(&item).unwrap(), b"owner data");
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }

    #[test]
    #[ignore = "requires an owned interactive Windows desktop and real UAC consent/cancellation"]
    fn windows_helper_pipe_native_uac_round_trip() {
        let helper = std::env::var("OTTRIN_QA_PRIV_HELPER").unwrap();
        let directory =
            std::path::PathBuf::from(std::env::var_os("OTTRIN_QA_ADMIN_DIRECTORY").unwrap());
        assert_eq!(
            std::fs::read_dir(&directory).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        let request = PrivilegedRequest {
            protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
            command: PrivilegedCommand::ListDirectory {
                path: directory,
                show_hidden: false,
            },
            context: PrivilegedContext {
                reason: Some("Read the owned native UAC fixture".into()),
                cwd: None,
            },
        };
        let response = execute(&helper, false, &request, &mut |_| Ok(())).unwrap();
        match std::env::var("OTTRIN_QA_UAC_EXPECT").unwrap().as_str() {
            "cancel" => {
                assert_eq!(response.status, PrivilegedStatus::Denied);
                assert!(response.message.as_deref().unwrap().contains("cancelled"));
                assert!(response.payload.is_none());
            }
            "approve" => {
                assert_eq!(response.status, PrivilegedStatus::Success);
                let Some(PrivilegedPayload::Entries(entries)) = response.payload else {
                    panic!("Missing elevated listing")
                };
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].name, "protected-proof.txt");
            }
            _ => panic!("Unrecognized native acceptance expectation"),
        }
    }

    #[test]
    #[ignore = "requires an owned ordinary desktop, a protected destination and real UAC"]
    fn windows_helper_pipe_native_uac_copy() {
        use ottrin_core::{ConflictAction, FileCommand};

        let helper = std::env::var("OTTRIN_QA_PRIV_HELPER").unwrap();
        let destination = PathBuf::from(std::env::var_os("OTTRIN_QA_ADMIN_DIRECTORY").unwrap());
        let source = PathBuf::from(std::env::var_os("OTTRIN_QA_COPY_SOURCE").unwrap());
        let original = std::fs::read(&source).unwrap();
        assert!(!original.is_empty());
        assert_eq!(
            std::fs::read_dir(&destination).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        let request = PrivilegedRequest {
            protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
            command: PrivilegedCommand::File(FileCommand::Copy {
                sources: vec![source.clone()],
                destination,
                conflict: ConflictAction::Skip,
            }),
            context: PrivilegedContext {
                reason: Some("Copy the owned native UAC fixture".into()),
                cwd: None,
            },
        };
        let mut progress = Vec::new();
        // Exercise the production helper launch and authenticated stream while
        // the public Copy capability remains gated on native acceptance.
        let response = execute(&helper, false, &request, &mut |bytes| {
            progress.push(bytes);
            Ok(())
        })
        .unwrap();
        match std::env::var("OTTRIN_QA_UAC_EXPECT").unwrap().as_str() {
            "cancel" => {
                assert_eq!(response.status, PrivilegedStatus::Denied);
                assert!(response.message.as_deref().unwrap().contains("cancelled"));
                assert!(response.payload.is_none());
                assert_eq!(progress, [0]);
            }
            "approve" => {
                assert_eq!(
                    response.status,
                    PrivilegedStatus::Success,
                    "{:?}",
                    response.message
                );
                let Some(PrivilegedPayload::Copy(receipt)) = response.payload else {
                    panic!("Missing elevated Copy receipt");
                };
                assert_eq!(receipt.completed_sources, 1);
                assert_eq!(receipt.transactions.len(), 1);
                assert_eq!(receipt.paths.len(), 1);
                assert_eq!(receipt.paths[0].1.file_name(), source.file_name());
                assert!(progress.iter().any(|&bytes| bytes >= original.len() as u64));
                if let Some(path) = std::env::var_os("OTTRIN_QA_COPY_RECEIPT") {
                    let file = std::fs::File::create_new(path).unwrap();
                    serde_json::to_writer(file, &receipt).unwrap();
                }
            }
            _ => panic!("Unrecognized native acceptance expectation"),
        }
        assert_eq!(std::fs::read(source).unwrap(), original);
    }

    #[test]
    #[ignore = "requires a retained owned UAC Copy and a fresh interactive client"]
    fn windows_helper_pipe_native_uac_copy_recovery() {
        use ottrin_core::{PrivilegedCopyHistoryState, PrivilegedCopyReceipt};

        let helper = std::env::var("OTTRIN_QA_PRIV_HELPER").unwrap();
        let receipt_path = std::env::var_os("OTTRIN_QA_COPY_RECEIPT").unwrap();
        let receipt: PrivilegedCopyReceipt =
            serde_json::from_reader(std::fs::File::open(receipt_path).unwrap()).unwrap();
        assert_eq!(receipt.completed_sources, 1);
        assert_eq!(receipt.transactions.len(), 1);
        let phase = std::env::var("OTTRIN_QA_COPY_RECOVERY_PHASE").unwrap();
        let command = match phase.as_str() {
            "history" | "undone" => PrivilegedCommand::DiscoverCopyHistory,
            "undo" => PrivilegedCommand::UndoCopy {
                transactions: receipt.transactions.clone(),
            },
            _ => panic!("Unrecognized native recovery phase"),
        };
        let request = PrivilegedRequest {
            protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
            command,
            context: PrivilegedContext {
                reason: Some("Verify recovery of the owned native UAC Copy".into()),
                cwd: None,
            },
        };
        let response = execute(&helper, false, &request, &mut |_| Ok(())).unwrap();
        assert_eq!(
            response.status,
            PrivilegedStatus::Success,
            "{:?}",
            response.message
        );
        if phase != "undo" {
            let Some(PrivilegedPayload::CopyHistory(entries)) = response.payload else {
                panic!("Missing elevated Copy history");
            };
            let entry = entries
                .iter()
                .find(|entry| entry.reference.transactions == receipt.transactions)
                .expect("Fresh helper must find the original Copy receipt");
            assert_eq!(entry.reference.paths, receipt.paths);
            // Discovery describes a retained operation, without reconstructing
            // the original batch's completion count.
            assert_eq!(entry.reference.completed_sources, 0);
            let expected = if phase == "history" {
                PrivilegedCopyHistoryState::UndoAvailable
            } else {
                PrivilegedCopyHistoryState::Undone
            };
            assert_eq!(entry.recovery, expected);
        }
    }
}
