//! Per-user static folder verbs. Commands and their ownership receipt commit in
//! one native registry transaction; an error or concurrent writer leaves neither
//! half-installed entries nor a compensating recovery path.
use ottrin_core::native_path::NativePath;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::os::windows::{
    ffi::OsStrExt, ffi::OsStringExt, io::AsRawHandle, io::FromRawHandle, io::OwnedHandle,
};
use std::path::Path;
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::{CommitTransaction, CreateTransaction},
    System::Registry::*,
};

const RECEIPT: &str = "FolderCommands";
const VERB: &str = "Ottrin.Open";
const MARKER: &str = "OttrinOwner";
const LIMIT: usize = 1024 * 1024;
#[derive(Clone, Debug, Serialize)]
pub struct FolderStatus {
    pub enabled: bool,
    pub ready: bool,
    pub detail: String,
    pub default_supported: bool,
    pub default_enabled: bool,
    pub default_ready: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    executable: NativePath,
    owner: [u8; 16],
    created_keys: Vec<String>,
    defaults: Option<BTreeMap<String, Option<Value>>>,
}
#[derive(Clone)]
struct Context {
    classes: String,
    state: String,
    volatile: bool,
}
impl Context {
    fn current() -> Self {
        Self {
            classes: r"Software\Classes".into(),
            state: r"Software\Ottrin\DesktopIntegration".into(),
            volatile: false,
        }
    }
    fn targets(&self) -> [(String, &'static str); 3] {
        [
            (format!(r"{}\Directory\shell\{VERB}", self.classes), "%1"),
            (
                format!(r"{}\Directory\Background\shell\{VERB}", self.classes),
                "%V",
            ),
            (format!(r"{}\Drive\shell\{VERB}", self.classes), "%1"),
        ]
    }
    fn allowed_created_keys(&self) -> BTreeSet<String> {
        self.targets()
            .iter()
            .map(|(path, _)| format!(r"{path}\command"))
            .chain(std::iter::once(self.state.clone()))
            .flat_map(|path| prefixes(&path))
            .filter(|path| path != "Software")
            .collect()
    }
    fn default_targets(&self) -> [String; 2] {
        [
            format!(r"{}\Directory\shell", self.classes),
            format!(r"{}\Drive\shell", self.classes),
        ]
    }
    fn read(&self, tx: &Transaction) -> Result<Option<Registration>, String> {
        let Some(key) = tx.open(&self.state)? else {
            return Ok(None);
        };
        let Some(value) = key.value(OsStr::new(RECEIPT))? else {
            return Ok(None);
        };
        if value.kind != REG_BINARY {
            return Err("Folder command receipt has an unexpected registry type; retained".into());
        }
        let registration: Registration = serde_json::from_slice(&value.bytes)
            .map_err(|e| format!("Folder command receipt retained: {e}"))?;
        registration
            .executable
            .decode()
            .map_err(|e| e.to_string())?;
        let allowed = self.allowed_created_keys();
        if registration
            .created_keys
            .iter()
            .any(|path| !allowed.contains(path))
            || registration.created_keys.len()
                != registration
                    .created_keys
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
        {
            return Err("Folder receipt contains an unrelated registry path; retained".into());
        }
        if let Some(defaults) = &registration.defaults
            && (defaults.len() != 2
                || self
                    .default_targets()
                    .iter()
                    .any(|path| !defaults.contains_key(path))
                || defaults
                    .values()
                    .flatten()
                    .any(|value| value.bytes.len() > LIMIT))
        {
            return Err("Folder receipt contains unrelated or oversized defaults; retained".into());
        }
        Ok(Some(registration))
    }
    fn matches(&self, tx: &Transaction, registration: &Registration) -> Result<bool, String> {
        let executable = registration
            .executable
            .decode()
            .map_err(|e| e.to_string())?;
        let mut complete = true;
        for (path, variable) in self.targets() {
            let Some(key) = tx.open(&path)? else {
                complete = false;
                continue;
            };
            let expected = verb_values(&executable, registration.owner)?;
            let actual = key.values()?;
            let children = key.children()?;
            if actual.get(OsStr::new(MARKER)) != expected.get(OsStr::new(MARKER))
                || actual
                    .iter()
                    .any(|(name, value)| expected.get(name) != Some(value))
                || children.iter().any(|name| name != OsStr::new("command"))
            {
                return Err(format!(
                    "Folder command changed outside Ottrin; retained: HKCU\\{path}"
                ));
            }
            complete &= actual == expected && children == vec![OsString::from("command")];
            let Some(command) = tx.open(&format!(r"{path}\command"))? else {
                complete = false;
                continue;
            };
            let expected =
                BTreeMap::from([(OsString::new(), command_value(&executable, variable)?)]);
            let actual = command.values()?;
            if actual
                .iter()
                .any(|(name, value)| expected.get(name) != Some(value))
                || !command.children()?.is_empty()
            {
                return Err(format!(
                    "Folder launch command changed outside Ottrin; retained: HKCU\\{path}\\command"
                ));
            }
            complete &= actual == expected;
        }
        Ok(complete)
    }
    fn configure(&self, tx: &Transaction, executable: &Path) -> Result<Registration, String> {
        if !executable.is_file() {
            return Err("The Ottrin application executable is unavailable".into());
        }
        quoted_executable(executable)?;
        let previous = self.read(tx)?;
        if let Some(previous) = &previous {
            self.matches(tx, previous)?;
        } else {
            for (path, _) in self.targets() {
                if tx.open(&path)?.is_some() {
                    return Err(format!(
                        "An unowned folder command already exists; retained: HKCU\\{path}"
                    ));
                }
            }
        }
        let mut owner = [0; 16];
        if let Some(previous) = &previous {
            owner = previous.owner;
        } else {
            getrandom::fill(&mut owner).map_err(|e| e.to_string())?;
        }
        let mut created = previous
            .as_ref()
            .map(|r| r.created_keys.clone())
            .unwrap_or_default();
        let defaults = previous.and_then(|r| r.defaults);
        for (path, variable) in self.targets() {
            let key = tx.ensure(&path, self.volatile, &mut created)?;
            for (name, value) in verb_values(executable, owner)? {
                key.set(&name, &value)?;
            }
            let command = tx.ensure(&format!(r"{path}\command"), self.volatile, &mut created)?;
            command.set(OsStr::new(""), &command_value(executable, variable)?)?;
        }
        let key = tx.ensure(&self.state, self.volatile, &mut created)?;
        let registration = Registration {
            executable: NativePath::encode(executable),
            owner,
            created_keys: created,
            defaults,
        };
        drop(key);
        self.store(tx, &registration)?;
        Ok(registration)
    }
    fn store(&self, tx: &Transaction, registration: &Registration) -> Result<(), String> {
        let bytes = serde_json::to_vec(registration).map_err(|e| e.to_string())?;
        if bytes.len() > LIMIT {
            return Err("Folder registration is too large; no settings changed".into());
        }
        tx.open(&self.state)?
            .ok_or("Folder registration key disappeared")?
            .set(
                OsStr::new(RECEIPT),
                &Value {
                    kind: REG_BINARY,
                    bytes,
                },
            )
    }
    fn enable(&self, executable: &Path) -> Result<String, String> {
        let tx = Transaction::new()?;
        self.configure(&tx, executable)?;
        tx.commit()?;
        notify_shell();
        Ok("Open in Ottrin enabled for folders, folder backgrounds and drives".into())
    }
    fn set_default(&self, executable: &Path, enabled: bool) -> Result<String, String> {
        let tx = Transaction::new()?;
        let mut registration = if enabled {
            self.configure(&tx, executable)?
        } else {
            let Some(registration) = self.read(&tx)? else {
                return Ok("Ottrin default folder opening is off".into());
            };
            self.matches(&tx, &registration)?;
            registration
        };
        if enabled {
            let mut defaults = BTreeMap::new();
            let installed = string_value(VERB.encode_utf16());
            for path in self.default_targets() {
                let key = tx.ensure(&path, self.volatile, &mut registration.created_keys)?;
                let current = key.value(OsStr::new(""))?;
                let previous = if current.as_ref() == Some(&installed) {
                    registration
                        .defaults
                        .as_ref()
                        .and_then(|values| values.get(&path))
                        .cloned()
                        .unwrap_or(current)
                } else {
                    current
                };
                defaults.insert(path, previous);
                key.set(OsStr::new(""), &installed)?;
            }
            registration.defaults = Some(defaults);
        } else {
            self.restore_defaults(&tx, &registration)?;
            registration.defaults = None;
        }
        self.store(&tx, &registration)?;
        tx.commit()?;
        notify_shell();
        Ok(if enabled {"Ottrin is selected for filesystem folder and drive links that use the Windows default verb. Programs explicitly requesting Explorer or another verb keep that behavior."} else {"Previous folder defaults restored where still owned by Ottrin; later choices preserved"}.into())
    }
    fn restore_defaults(
        &self,
        tx: &Transaction,
        registration: &Registration,
    ) -> Result<(), String> {
        if let Some(defaults) = &registration.defaults {
            let installed = string_value(VERB.encode_utf16());
            for (path, previous) in defaults {
                if let Some(key) = tx.open(path)?
                    && key.value(OsStr::new(""))?.as_ref() == Some(&installed)
                {
                    if let Some(previous) = previous {
                        key.set(OsStr::new(""), previous)?;
                    } else {
                        key.remove(OsStr::new(""))?;
                    }
                }
            }
        }
        Ok(())
    }
    fn disable(&self) -> Result<String, String> {
        let tx = Transaction::new()?;
        let Some(registration) = self.read(&tx)? else {
            return Ok("Ottrin folder commands are off".into());
        };
        self.matches(&tx, &registration)?;
        self.restore_defaults(&tx, &registration)?;
        for (path, _) in self.targets() {
            tx.delete(&format!(r"{path}\command"))?;
            tx.delete(&path)?;
        }
        if let Some(key) = tx.open(&self.state)? {
            key.remove(OsStr::new(RECEIPT))?;
        }
        let mut parents = registration.created_keys;
        parents.sort_by_key(|path| std::cmp::Reverse(path.matches('\\').count()));
        for path in parents {
            let empty = tx
                .open(&path)?
                .map(|key| key.is_empty())
                .transpose()?
                .unwrap_or(false);
            if empty {
                tx.delete(&path)?;
            }
        }
        tx.commit()?;
        notify_shell();
        Ok("Ottrin folder commands removed; owned defaults restored and later choices kept".into())
    }
}
pub fn enable() -> Result<String, String> {
    if super::windows_startup::is_packaged()? {
        ottrin_shell::set_enabled(true)?;
        notify_shell();
        return Ok("Open in Ottrin enabled in Windows folder menus".into());
    }
    Context::current().enable(&super::application_launch_path()?)
}
pub fn disable() -> Result<String, String> {
    if super::windows_startup::is_packaged()? {
        ottrin_shell::set_enabled(false)?;
        notify_shell();
        return Ok("Open in Ottrin hidden from Windows folder menus".into());
    }
    Context::current().disable()
}
pub fn enable_default() -> Result<String, String> {
    set_default(true)
}
pub fn disable_default() -> Result<String, String> {
    set_default(false)
}
fn set_default(enabled: bool) -> Result<String, String> {
    if super::windows_startup::is_packaged()? {
        return Err("Default folder opening is unavailable in the MSIX package.".into());
    }
    Context::current().set_default(&super::application_launch_path()?, enabled)
}
pub fn status() -> FolderStatus {
    match super::windows_startup::is_packaged() {
        Ok(true) => return packaged_status(),
        Ok(false) => {}
        Err(detail) => {
            return FolderStatus {
                enabled: true,
                ready: false,
                detail,
                default_supported: false,
                default_enabled: false,
                default_ready: false,
            };
        }
    }
    let context = Context::current();
    let check = (|| {
        let tx = Transaction::new()?;
        let Some(registration) = context.read(&tx)? else {
            return Ok::<_, String>((
                false,
                false,
                "Off — Windows folder menus keep their current actions".into(),
                false,
                false,
            ));
        };
        let complete = context.matches(&tx, &registration)?;
        let registered = registration
            .executable
            .decode()
            .map_err(|e| e.to_string())?;
        let ready =
            complete && registered.is_file() && registered == super::application_launch_path()?;
        let default_enabled = registration.defaults.is_some();
        let installed = string_value(VERB.encode_utf16());
        let default_ready = ready
            && default_enabled
            && context
                .default_targets()
                .iter()
                .map(|path| {
                    tx.open(path)?
                        .map(|key| key.value(OsStr::new("")))
                        .transpose()
                })
                .collect::<Result<Vec<_>, String>>()?
                .into_iter()
                .all(|value| value.flatten().as_ref() == Some(&installed));
        Ok((
            true,
            ready,
            if ready {
                "Enabled — Open in Ottrin is available in folder menus".into()
            } else {
                "Needs update — repair Ottrin's folder menu commands".into()
            },
            default_enabled,
            default_ready,
        ))
    })();
    match check {
        Ok((enabled, ready, detail, default_enabled, default_ready)) => FolderStatus {
            enabled,
            ready,
            detail,
            default_supported: true,
            default_enabled,
            default_ready,
        },
        Err(error) => FolderStatus {
            enabled: true,
            ready: false,
            detail: error,
            default_supported: true,
            default_enabled: false,
            default_ready: false,
        },
    }
}

fn packaged_status() -> FolderStatus {
    let state = ottrin_shell::enabled().and_then(|enabled| {
        let library = ottrin_shell::package_directory()?.join("bin/ottrin_shell.dll");
        if !library.is_file() {
            return Err(
                "Ottrin's packaged folder command is missing; reinstall this package".into(),
            );
        }
        Ok(enabled)
    });
    match state {
        Ok(enabled) => FolderStatus {
            enabled,
            ready: enabled,
            detail: if enabled {
                "Enabled — Open in Ottrin is available in folder menus"
            } else {
                "Off — Open in Ottrin is hidden from folder menus"
            }
            .into(),
            // Package-owned context commands do not change host folder defaults.
            default_supported: false,
            default_enabled: false,
            default_ready: false,
        },
        Err(detail) => FolderStatus {
            enabled: true,
            ready: false,
            detail,
            default_supported: false,
            default_enabled: false,
            default_ready: false,
        },
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Value {
    kind: u32,
    bytes: Vec<u8>,
}
fn string_value(units: impl IntoIterator<Item = u16>) -> Value {
    Value {
        kind: REG_SZ,
        bytes: units
            .into_iter()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect(),
    }
}
fn quoted_executable(path: &Path) -> Result<Vec<u16>, String> {
    let units: Vec<_> = path.as_os_str().encode_wide().collect();
    if units.is_empty() || units.iter().any(|u| matches!(*u, 0 | 34)) {
        return Err("The executable path cannot be represented in a Windows shell command".into());
    }
    Ok(std::iter::once(34).chain(units).chain(Some(34)).collect())
}
fn command_value(path: &Path, variable: &str) -> Result<Value, String> {
    // The shell substitutes directory names without escaping a trailing `\`.
    // A final dot component names the same directory and keeps a drive root's
    // backslash away from the closing argument quote. The launcher resolves it
    // through its ordinary canonical directory path, without a second parser.
    Ok(string_value(
        quoted_executable(path)?
            .into_iter()
            .chain(format!(" -- \"{variable}\\.\"").encode_utf16()),
    ))
}
fn verb_values(path: &Path, owner: [u8; 16]) -> Result<BTreeMap<OsString, Value>, String> {
    Ok(BTreeMap::from([
        (
            OsString::new(),
            string_value("Open in Ottrin".encode_utf16()),
        ),
        (
            "Icon".into(),
            string_value(
                quoted_executable(path)?
                    .into_iter()
                    .chain(",0".encode_utf16()),
            ),
        ),
        (
            "MultiSelectModel".into(),
            string_value("Single".encode_utf16()),
        ),
        (
            MARKER.into(),
            Value {
                kind: REG_BINARY,
                bytes: owner.to_vec(),
            },
        ),
    ]))
}
fn notify_shell() {
    use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
    // SAFETY: this change event has no item pointers; notify only after commit.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}
fn prefixes(path: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut value = String::new();
    for part in path.split('\\') {
        if !value.is_empty() {
            value.push('\\')
        }
        value.push_str(part);
        result.push(value.clone());
    }
    result
}
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn result(code: u32) -> Result<(), String> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(code as i32).to_string())
    }
}
fn transaction_error(operation: &str, path: Option<&str>, error: std::io::Error) -> String {
    let target = path.map_or(String::new(), |path| format!(" for HKCU\\{path}"));
    match error.raw_os_error() {
        Some(code) => format!("{operation}{target} failed (Windows error {code}): {error}"),
        None => format!("{operation}{target} failed: {error}"),
    }
}
struct Transaction(OwnedHandle);
impl Transaction {
    fn new() -> Result<Self, String> {
        // SAFETY: no pointers are supplied; this creates one non-inheritable owned transaction.
        let handle = unsafe {
            CreateTransaction(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                std::ptr::null(),
            )
        };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(transaction_error(
                "CreateTransaction",
                None,
                std::io::Error::last_os_error(),
            ));
        }
        // SAFETY: successful CreateTransaction transfers the owned handle. Closing
        // an uncommitted transaction aborts its registry changes.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
    }
    fn open(&self, path: &str) -> Result<Option<Key>, String> {
        let name = wide(OsStr::new(path));
        let mut key = std::ptr::null_mut();
        // SAFETY: terminated path/output remain live and transaction stays owned.
        let code = unsafe {
            RegOpenKeyTransactedW(
                HKEY_CURRENT_USER,
                name.as_ptr(),
                0,
                KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
                &mut key,
                self.0.as_raw_handle(),
                std::ptr::null(),
            )
        };
        if matches!(code, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
            return Ok(None);
        }
        if code != ERROR_SUCCESS {
            return Err(transaction_error(
                "RegOpenKeyTransactedW",
                Some(path),
                std::io::Error::from_raw_os_error(code as i32),
            ));
        }
        Ok(Some(Key(key)))
    }
    fn ensure(&self, path: &str, volatile: bool, created: &mut Vec<String>) -> Result<Key, String> {
        for prefix in prefixes(path) {
            if prefix == "Software" {
                if self.open(&prefix)?.is_none() {
                    return Err(
                        "The current user's Software registry container is unavailable".into(),
                    );
                }
                continue;
            }

            let name = wide(OsStr::new(&prefix));
            let mut key = std::ptr::null_mut();
            let mut disposition = 0;
            // SAFETY: buffers/outputs remain live; every key explicitly enlists
            // in this transaction, including children (which do not inherit it).
            let code = unsafe {
                RegCreateKeyTransactedW(
                    HKEY_CURRENT_USER,
                    name.as_ptr(),
                    0,
                    std::ptr::null(),
                    if volatile && prefix != "Software" {
                        REG_OPTION_VOLATILE
                    } else {
                        REG_OPTION_NON_VOLATILE
                    },
                    KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
                    std::ptr::null(),
                    &mut key,
                    &mut disposition,
                    self.0.as_raw_handle(),
                    std::ptr::null(),
                )
            };
            if code != ERROR_SUCCESS {
                return Err(transaction_error(
                    "RegCreateKeyTransactedW",
                    Some(&prefix),
                    std::io::Error::from_raw_os_error(code as i32),
                ));
            }
            drop(Key(key));
            if disposition == REG_CREATED_NEW_KEY && !created.contains(&prefix) {
                created.push(prefix);
            }
        }
        self.open(path)?
            .ok_or("Registry key disappeared in transaction".into())
    }
    fn delete(&self, path: &str) -> Result<(), String> {
        let name = wide(OsStr::new(path));
        // SAFETY: terminated path and owned transaction remain live. Only empty
        // keys are removed; unknown children are never recursively deleted.
        let code = unsafe {
            RegDeleteKeyTransactedW(
                HKEY_CURRENT_USER,
                name.as_ptr(),
                KEY_WOW64_64KEY,
                0,
                self.0.as_raw_handle(),
                std::ptr::null(),
            )
        };
        if matches!(
            code,
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND
        ) {
            Ok(())
        } else {
            Err(transaction_error(
                "RegDeleteKeyTransactedW",
                Some(path),
                std::io::Error::from_raw_os_error(code as i32),
            ))
        }
    }
    fn commit(self) -> Result<(), String> {
        // SAFETY: this transaction is owned; all key writes used its handle.
        if unsafe { CommitTransaction(self.0.as_raw_handle()) } == 0 {
            Err(transaction_error(
                "CommitTransaction",
                None,
                std::io::Error::last_os_error(),
            ))
        } else {
            Ok(())
        }
    }
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: each successful open/create transfers exactly one owned key handle.
        let code = unsafe { RegCloseKey(self.0) };
        if code != ERROR_SUCCESS {
            tracing::warn!(code, "Cannot release folder integration registry handle");
        }
    }
}
impl Key {
    fn value(&self, name: &OsStr) -> Result<Option<Value>, String> {
        let name = wide(name);
        let mut bytes = vec![0; LIMIT];
        let mut size = bytes.len() as u32;
        let mut kind = 0;
        // SAFETY: owned key and terminated name/bounded writable buffer remain live.
        let code = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut size,
            )
        };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        result(code)?;
        bytes.truncate(size as usize);
        Ok(Some(Value { kind, bytes }))
    }

    fn is_empty(&self) -> Result<bool, String> {
        let mut children = 0;
        let mut values = 0;
        // SAFETY: the key is owned; only these two scalar output counts are requested.
        result(unsafe {
            RegQueryInfoKeyW(
                self.0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                &mut children,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut values,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        })?;
        Ok(children == 0 && values == 0)
    }
    fn values(&self) -> Result<BTreeMap<OsString, Value>, String> {
        let mut values = BTreeMap::new();
        let mut bytes = vec![0; LIMIT];
        for index in 0..32 {
            let mut name = [0; 256];
            let mut length = name.len() as u32;
            let mut size = bytes.len() as u32;
            let mut kind = 0;
            // SAFETY: key and bounded writable buffers remain live; lengths match capacities.
            let code = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    std::ptr::null(),
                    &mut kind,
                    bytes.as_mut_ptr(),
                    &mut size,
                )
            };
            if code == ERROR_NO_MORE_ITEMS {
                return Ok(values);
            }
            result(code)?;
            values.insert(
                OsString::from_wide(&name[..length as usize]),
                Value {
                    kind,
                    bytes: bytes[..size as usize].to_vec(),
                },
            );
        }
        Err("Unexpectedly large registry entry retained".into())
    }
    fn children(&self) -> Result<Vec<OsString>, String> {
        let mut children = Vec::new();
        for index in 0..32 {
            let mut name = [0; 256];
            let mut length = name.len() as u32;
            // SAFETY: key and bounded writable name buffer remain live.
            let code = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if code == ERROR_NO_MORE_ITEMS {
                return Ok(children);
            }
            result(code)?;
            children.push(OsString::from_wide(&name[..length as usize]));
        }
        Err("Unexpected registry children retained".into())
    }
    fn set(&self, name: &OsStr, value: &Value) -> Result<(), String> {
        let name = wide(name);
        let length = u32::try_from(value.bytes.len()).map_err(|e| e.to_string())?;
        // SAFETY: owned key and terminated name/data remain live for this call.
        result(unsafe {
            RegSetValueExW(
                self.0,
                name.as_ptr(),
                0,
                value.kind,
                value.bytes.as_ptr(),
                length,
            )
        })
    }
    fn remove(&self, name: &OsStr) -> Result<(), String> {
        let name = wide(name);
        // SAFETY: owned key and terminated name remain live.
        let code = unsafe { RegDeleteValueW(self.0, name.as_ptr()) };
        if code == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            result(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        context: Context,
        root: String,
        token: Value,
    }
    impl Fixture {
        fn new() -> Self {
            let mut id = [0; 16];
            getrandom::fill(&mut id).unwrap();
            let root = format!(
                r"Software\Ottrin.FolderTests.{}",
                ottrin_core::native_path::encode_hex(&id)
            );
            let tx = Transaction::new().unwrap();
            assert!(tx.open(&root).unwrap().is_none());
            let token = Value {
                kind: REG_BINARY,
                bytes: id.to_vec(),
            };
            let key = tx.ensure(&root, true, &mut Vec::new()).unwrap();
            key.set(OsStr::new("FixtureOwner"), &token).unwrap();
            drop(key);
            tx.commit().unwrap();
            eprintln!("Owned volatile registry fixture: HKCU\\{root}");
            Self {
                context: Context {
                    classes: format!(r"{root}\Classes"),
                    state: format!(r"{root}\State"),
                    volatile: true,
                },
                root,
                token,
            }
        }
        fn executable(&self) -> std::path::PathBuf {
            std::env::current_exe().unwrap()
        }
        fn registration(&self) -> Registration {
            self.context
                .read(&Transaction::new().unwrap())
                .unwrap()
                .unwrap()
        }
        fn clean(&self) -> Result<(), String> {
            fn remove(tx: &Transaction, path: &str) -> Result<(), String> {
                let Some(key) = tx.open(path)? else {
                    return Ok(());
                };
                for child in key.children()? {
                    let child = child.to_str().ok_or("Unexpected fixture key encoding")?;
                    remove(tx, &format!(r"{path}\{child}"))?;
                }
                for name in key.values()?.keys() {
                    key.remove(name)?;
                }
                drop(key);
                tx.delete(path)
            }
            let tx = Transaction::new()?;
            let key = tx.open(&self.root)?.ok_or("Fixture root disappeared")?;
            if key.values()?.get(OsStr::new("FixtureOwner")) != Some(&self.token) {
                return Err("Fixture ownership changed; retained".into());
            }
            drop(key);
            // This nonce-bound fixture owns its complete subtree, including the
            // deliberate unknown-data controls below. Never visit another root.
            remove(&tx, &self.root)?;
            tx.commit()?;
            if Transaction::new()?.open(&self.root)?.is_some() {
                return Err("Fixture registry root remains".into());
            }
            Ok(())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Err(error) = self.clean() {
                eprintln!("Registry fixture retained at HKCU\\{}: {error}", self.root);
                if !std::thread::panicking() {
                    panic!("Registry fixture cleanup failed")
                }
            }
        }
    }
    #[test]
    fn default_opening_is_atomic_repairs_keep_it_and_restore_preserves_raw_values() {
        for original in [
            None,
            Some(string_value("Previous.Verb".encode_utf16())),
            Some(Value {
                kind: REG_EXPAND_SZ,
                bytes: vec![0x25, 0, 0, 0],
            }),
            Some(Value {
                kind: REG_BINARY,
                bytes: vec![0, 255, 80],
            }),
        ] {
            let fixture = Fixture::new();
            let context = &fixture.context;
            let exe = fixture.executable();
            let tx = Transaction::new().unwrap();
            let mut created = Vec::new();
            for path in context.default_targets() {
                let key = tx.ensure(&path, true, &mut created).unwrap();
                if let Some(value) = &original {
                    key.set(OsStr::new(""), value).unwrap();
                }
            }
            tx.commit().unwrap();
            context.set_default(&exe, true).unwrap();
            let owned = string_value(VERB.encode_utf16());
            for path in context.default_targets() {
                assert_eq!(
                    Transaction::new()
                        .unwrap()
                        .open(&path)
                        .unwrap()
                        .unwrap()
                        .value(OsStr::new(""))
                        .unwrap(),
                    Some(owned.clone())
                );
            }
            context.enable(&exe).unwrap();
            assert!(fixture.registration().defaults.is_some());
            context.set_default(&exe, false).unwrap();
            assert!(fixture.registration().defaults.is_none());
            for path in context.default_targets() {
                assert_eq!(
                    Transaction::new()
                        .unwrap()
                        .open(&path)
                        .unwrap()
                        .unwrap()
                        .value(OsStr::new(""))
                        .unwrap(),
                    original
                );
            }
            context.disable().unwrap();
        }
    }
    #[test]
    fn removing_defaults_restores_only_values_still_owned_and_keeps_other_choices() {
        let fixture = Fixture::new();
        let context = &fixture.context;
        let exe = fixture.executable();
        context.set_default(&exe, true).unwrap();
        let paths = context.default_targets();
        let third_party = string_value("Later.User.Choice".encode_utf16());
        let tx = Transaction::new().unwrap();
        tx.open(&paths[1])
            .unwrap()
            .unwrap()
            .set(OsStr::new(""), &third_party)
            .unwrap();
        tx.commit().unwrap();
        context.disable().unwrap();
        let tx = Transaction::new().unwrap();
        assert!(tx.open(&paths[0]).unwrap().is_none());
        assert_eq!(
            tx.open(&paths[1])
                .unwrap()
                .unwrap()
                .value(OsStr::new(""))
                .unwrap(),
            Some(third_party)
        );
    }
    #[test]
    fn a_default_receipt_cannot_restore_an_unrelated_registry_path() {
        let fixture = Fixture::new();
        let context = &fixture.context;
        let exe = fixture.executable();
        context.set_default(&exe, true).unwrap();
        let tx = Transaction::new().unwrap();
        let mut registration = context.read(&tx).unwrap().unwrap();
        registration
            .defaults
            .as_mut()
            .unwrap()
            .insert(r"Software\Unrelated".into(), None);
        context.store(&tx, &registration).unwrap();
        tx.commit().unwrap();
        assert!(context.set_default(&exe, false).is_err());
        let tx = Transaction::new().unwrap();
        assert_eq!(
            tx.open(&context.default_targets()[0])
                .unwrap()
                .unwrap()
                .value(OsStr::new(""))
                .unwrap(),
            Some(string_value(VERB.encode_utf16()))
        );
    }
    #[test]
    fn shell_directory_arguments_survive_native_parsing_including_drive_roots() {
        use windows::{Win32::UI::Shell::CommandLineToArgvW, core::PCWSTR};
        struct Arguments(*mut windows::core::PWSTR);
        impl Drop for Arguments {
            fn drop(&mut self) {
                // SAFETY: CommandLineToArgvW transfers one LocalAlloc block.
                let remaining = unsafe {
                    windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
                        self.0.cast(),
                    ))
                };
                if !remaining.0.is_null() {
                    tracing::warn!("Cannot release parsed folder command arguments");
                }
            }
        }
        let temporary = tempfile::tempdir().unwrap();
        let mut name: Vec<u16> = "space 100% & literal ".encode_utf16().collect();
        name.push(0xd800);
        let native_directory = temporary.path().join(OsString::from_wide(&name));
        std::fs::create_dir(&native_directory).unwrap();
        let executable = std::env::current_exe().unwrap();
        let drive = executable.ancestors().last().unwrap();
        for directory in [drive, native_directory.as_path()] {
            for variable in ["%1", "%V"] {
                let value = command_value(&executable, variable).unwrap();
                let mut command: Vec<u16> = value
                    .bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                let token: Vec<u16> = variable.encode_utf16().collect();
                let at = command
                    .windows(token.len())
                    .rposition(|part| part == token)
                    .unwrap();
                command.splice(at..at + token.len(), directory.as_os_str().encode_wide());
                let mut count = 0;
                // SAFETY: the registry string is terminated and count is writable.
                let raw = unsafe { CommandLineToArgvW(PCWSTR(command.as_ptr()), &mut count) };
                assert!(!raw.is_null(), "{}", std::io::Error::last_os_error());
                let args = Arguments(raw);
                assert_eq!(count, 3);
                // SAFETY: the returned block contains count terminated arguments.
                let parsed: Vec<_> = unsafe { std::slice::from_raw_parts(args.0, count as usize) }
                    .iter()
                    .map(|argument| OsString::from_wide(unsafe { argument.as_wide() }))
                    .collect();
                assert_eq!(parsed[0], executable.as_os_str());
                assert_eq!(parsed[1], OsStr::new("--"));
                assert_eq!(
                    std::fs::canonicalize(Path::new(&parsed[2])).unwrap(),
                    std::fs::canonicalize(directory).unwrap()
                );
            }
        }
    }

    #[test]
    fn folder_verbs_update_and_uninstall_leave_unrelated_registry_data() {
        let fixture = Fixture::new();
        let ctx = &fixture.context;
        let exe = fixture.executable();
        let tx = Transaction::new().unwrap();
        let other = format!(r"{}\Directory\shell\Unrelated", ctx.classes);
        let key = tx.ensure(&other, true, &mut Vec::new()).unwrap();
        let value = string_value("preserve another handler".encode_utf16());
        key.set(OsStr::new(""), &value).unwrap();
        drop(key);
        tx.commit().unwrap();
        ctx.enable(&exe).unwrap();
        let first = fixture.registration();
        assert!(ctx.matches(&Transaction::new().unwrap(), &first).unwrap());
        ctx.enable(&exe).unwrap();
        assert_eq!(fixture.registration().owner, first.owner);
        ctx.disable().unwrap();
        let tx = Transaction::new().unwrap();
        assert!(ctx.read(&tx).unwrap().is_none());
        assert_eq!(
            tx.open(&other)
                .unwrap()
                .unwrap()
                .values()
                .unwrap()
                .get(OsStr::new("")),
            Some(&value)
        );
        for (target, _) in ctx.targets() {
            assert!(tx.open(&target).unwrap().is_none());
        }
    }
    #[test]
    fn unowned_verb_refuses_initial_setup_without_partial_registration() {
        let fixture = Fixture::new();
        let ctx = &fixture.context;
        let target = ctx.targets()[1].0.clone();
        let tx = Transaction::new().unwrap();
        let key = tx.ensure(&target, true, &mut Vec::new()).unwrap();
        let value = string_value("foreign".encode_utf16());
        key.set(OsStr::new(""), &value).unwrap();
        drop(key);
        tx.commit().unwrap();
        assert!(
            ctx.enable(&fixture.executable())
                .unwrap_err()
                .contains("unowned")
        );
        let tx = Transaction::new().unwrap();
        assert!(ctx.read(&tx).unwrap().is_none());
        assert!(tx.open(&ctx.targets()[0].0).unwrap().is_none());
        assert!(tx.open(&ctx.targets()[2].0).unwrap().is_none());
        assert_eq!(
            tx.open(&target)
                .unwrap()
                .unwrap()
                .values()
                .unwrap()
                .get(OsStr::new("")),
            Some(&value)
        );
    }
    #[test]
    fn changed_command_is_preserved_by_update_and_uninstall() {
        let fixture = Fixture::new();
        let ctx = &fixture.context;
        ctx.enable(&fixture.executable()).unwrap();
        let registration = fixture.registration();
        let path = format!(r"{}\command", ctx.targets()[1].0);
        let changed = string_value("foreign command".encode_utf16());
        let tx = Transaction::new().unwrap();
        tx.open(&path)
            .unwrap()
            .unwrap()
            .set(OsStr::new(""), &changed)
            .unwrap();
        tx.commit().unwrap();
        assert!(ctx.enable(&fixture.executable()).is_err());
        assert!(ctx.disable().is_err());
        let tx = Transaction::new().unwrap();
        assert_eq!(
            tx.open(&path)
                .unwrap()
                .unwrap()
                .values()
                .unwrap()
                .get(OsStr::new("")),
            Some(&changed)
        );
        assert_eq!(ctx.read(&tx).unwrap().unwrap().owner, registration.owner);
    }
    #[test]
    fn unknown_owned_values_refuse_removal_and_deleted_verbs_can_be_repaired() {
        let fixture = Fixture::new();
        let ctx = &fixture.context;
        let exe = fixture.executable();
        ctx.enable(&exe).unwrap();
        let path = ctx.targets()[0].0.clone();
        let tx = Transaction::new().unwrap();
        tx.open(&path)
            .unwrap()
            .unwrap()
            .set(
                OsStr::new("UserAddition"),
                &string_value("keep".encode_utf16()),
            )
            .unwrap();
        tx.commit().unwrap();
        assert!(ctx.disable().is_err());
        let tx = Transaction::new().unwrap();
        tx.open(&path)
            .unwrap()
            .unwrap()
            .remove(OsStr::new("UserAddition"))
            .unwrap();
        tx.delete(&format!(r"{path}\command")).unwrap();
        tx.delete(&path).unwrap();
        tx.commit().unwrap();
        assert!(
            !ctx.matches(&Transaction::new().unwrap(), &fixture.registration())
                .unwrap()
        );
        ctx.enable(&exe).unwrap();
        assert!(
            ctx.matches(&Transaction::new().unwrap(), &fixture.registration())
                .unwrap()
        );
        ctx.disable().unwrap();
    }
    #[test]
    fn repair_restores_only_missing_data_with_an_intact_owner_marker() {
        let fixture = Fixture::new();
        let ctx = &fixture.context;
        let exe = fixture.executable();
        ctx.enable(&exe).unwrap();
        let path = ctx.targets()[0].0.clone();
        let tx = Transaction::new().unwrap();
        tx.open(&path)
            .unwrap()
            .unwrap()
            .remove(OsStr::new("Icon"))
            .unwrap();
        tx.open(&format!(r"{path}\command"))
            .unwrap()
            .unwrap()
            .remove(OsStr::new(""))
            .unwrap();
        tx.commit().unwrap();
        assert!(
            !ctx.matches(&Transaction::new().unwrap(), &fixture.registration())
                .unwrap()
        );
        ctx.enable(&exe).unwrap();
        assert!(
            ctx.matches(&Transaction::new().unwrap(), &fixture.registration())
                .unwrap()
        );
        let tx = Transaction::new().unwrap();
        tx.open(&path)
            .unwrap()
            .unwrap()
            .remove(OsStr::new(MARKER))
            .unwrap();
        tx.commit().unwrap();
        assert!(ctx.enable(&exe).is_err());
        assert!(ctx.disable().is_err());
    }

    #[test]
    fn abandoning_native_transaction_rolls_back_values_keys_and_receipt() {
        let fixture = Fixture::new();
        let ctx = &fixture.context;
        {
            let tx = Transaction::new().unwrap();
            for (target, variable) in ctx.targets() {
                let key = tx.ensure(&target, true, &mut Vec::new()).unwrap();
                for (name, value) in verb_values(&fixture.executable(), [3; 16]).unwrap() {
                    key.set(&name, &value).unwrap();
                }
                tx.ensure(&format!(r"{target}\command"), true, &mut Vec::new())
                    .unwrap()
                    .set(
                        OsStr::new(""),
                        &command_value(&fixture.executable(), variable).unwrap(),
                    )
                    .unwrap();
            }
            tx.ensure(&ctx.state, true, &mut Vec::new())
                .unwrap()
                .set(
                    OsStr::new(RECEIPT),
                    &Value {
                        kind: REG_BINARY,
                        bytes: b"unfinished".to_vec(),
                    },
                )
                .unwrap();
            // Deliberately leave all writes uncommitted, as on a setup error.
        }
        let tx = Transaction::new().unwrap();
        assert!(ctx.read(&tx).unwrap().is_none());
        for (target, _) in ctx.targets() {
            assert!(tx.open(&target).unwrap().is_none());
        }
    }
    #[test]
    fn uninstall_keeps_created_parents_with_many_unrelated_handlers() {
        let fixture = Fixture::new();
        let ctx = &fixture.context;
        ctx.enable(&fixture.executable()).unwrap();
        let parent = ctx.targets()[0].0.rsplit_once('\\').unwrap().0.to_owned();
        let value = string_value("preserve other handler".encode_utf16());
        let tx = Transaction::new().unwrap();
        for index in 0..40 {
            tx.ensure(&format!(r"{parent}\Other{index}"), true, &mut Vec::new())
                .unwrap()
                .set(OsStr::new(""), &value)
                .unwrap();
        }
        tx.commit().unwrap();
        ctx.disable().unwrap();
        let tx = Transaction::new().unwrap();
        assert!(ctx.read(&tx).unwrap().is_none());
        for index in 0..40 {
            let path = format!(r"{parent}\Other{index}");
            let key = tx.open(&path).unwrap().unwrap();
            assert_eq!(key.values().unwrap().get(OsStr::new("")), Some(&value));
            key.remove(OsStr::new("")).unwrap();
            drop(key);
            tx.delete(&path).unwrap();
        }
        tx.commit().unwrap();
    }

    #[test]
    fn tampered_receipt_cannot_widen_empty_parent_cleanup() {
        let fixture = Fixture::new();
        let ctx = &fixture.context;
        ctx.enable(&fixture.executable()).unwrap();
        let mut registration = fixture.registration();
        registration.created_keys.push(r"Software\Unrelated".into());
        let tx = Transaction::new().unwrap();
        tx.open(&ctx.state)
            .unwrap()
            .unwrap()
            .set(
                OsStr::new(RECEIPT),
                &Value {
                    kind: REG_BINARY,
                    bytes: serde_json::to_vec(&registration).unwrap(),
                },
            )
            .unwrap();
        tx.commit().unwrap();
        assert!(
            ctx.disable()
                .unwrap_err()
                .contains("unrelated registry path")
        );
        for (target, _) in ctx.targets() {
            assert!(Transaction::new().unwrap().open(&target).unwrap().is_some());
        }
    }
}
