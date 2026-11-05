use std::{
    cell::RefCell,
    ffi::{OsString, c_void},
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU32, Ordering},
};
use windows::{
    Win32::{
        Foundation::*,
        System::{Com::*, Ole::*},
        UI::Shell::*,
    },
    core::{GUID, HRESULT, IUnknown, Interface, PCWSTR, PWSTR, Result, implement},
};

// Also declared in msix-manifest.xml; the manifest contract test checks equality.
const CLASS: GUID = GUID::from_u128(0x6aee96d8_191d_476c_a773_b8679c83311c);
static OBJECTS: AtomicU32 = AtomicU32::new(0);
static LOCKS: AtomicU32 = AtomicU32::new(0);

struct Lifetime;
impl Lifetime {
    fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}
impl Drop for Lifetime {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

#[implement(IClassFactory)]
struct Factory {
    _lifetime: Lifetime,
}
impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Option<&IUnknown>,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> Result<()> {
        if out.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *out = std::ptr::null_mut();
        }
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        if iid.is_null() {
            return Err(E_POINTER.into());
        }
        let object: IExplorerCommand = FolderCommand {
            site: RefCell::new(None),
            _lifetime: Lifetime::new(),
        }
        .into();
        unsafe { object.query(iid, out).ok() }
    }
    fn LockServer(&self, lock: BOOL) -> Result<()> {
        if lock.as_bool() {
            LOCKS.fetch_add(1, Ordering::SeqCst);
        } else {
            LOCKS
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .map_err(|_| windows::core::Error::from(E_UNEXPECTED))?;
        }
        Ok(())
    }
}

#[implement(IExplorerCommand, IObjectWithSite)]
struct FolderCommand {
    site: RefCell<Option<IUnknown>>,
    _lifetime: Lifetime,
}

fn owned_string(value: &str) -> Result<PWSTR> {
    let value: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    unsafe { SHStrDupW(PCWSTR(value.as_ptr())) }
}

fn folder_path(item: &IShellItem) -> Result<PathBuf> {
    let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH)? };
    let path = PathBuf::from(OsString::from_wide(unsafe { name.as_wide() }));
    unsafe {
        CoTaskMemFree(Some(name.0.cast()));
    }
    if !path.is_absolute() || !path.is_dir() {
        return Err(E_INVALIDARG.into());
    }
    Ok(path)
}

impl FolderCommand_Impl {
    fn folders(&self, items: Option<&IShellItemArray>) -> Result<Vec<PathBuf>> {
        if let Some(items) = items {
            let count = unsafe { items.GetCount()? };
            if count != 0 {
                return (0..count)
                    .map(|i| folder_path(&unsafe { items.GetItemAt(i)? }))
                    .collect();
            }
        }
        // Explorer's folder background supplies its current view through the
        // site. Do not substitute the surrogate's working directory or Home.
        let site = self
            .site
            .try_borrow()
            .map_err(|_| windows::core::Error::from(E_UNEXPECTED))?
            .clone();
        let provider: IServiceProvider = site.as_ref().ok_or(E_FAIL)?.cast()?;
        // SID_SFolderView is the SDK alias for IID_IFolderView.
        let view: IFolderView = unsafe { provider.QueryService(&IFolderView::IID)? };
        // GetFolder documents IShellItemArray for the view's folder, rather
        // than promising that the underlying IShellFolder exposes IShellItem.
        let folder: IShellItemArray = unsafe { view.GetFolder()? };
        if unsafe { folder.GetCount()? } != 1 {
            return Err(E_INVALIDARG.into());
        }
        let item = unsafe { folder.GetItemAt(0)? };
        Ok(vec![folder_path(&item)?])
    }
}

impl IExplorerCommand_Impl for FolderCommand_Impl {
    fn GetTitle(&self, _: Option<&IShellItemArray>) -> Result<PWSTR> {
        owned_string("Open in Ottrin")
    }
    fn GetIcon(&self, _: Option<&IShellItemArray>) -> Result<PWSTR> {
        let executable = crate::package_directory()
            .map_err(|_| windows::core::Error::from(E_FAIL))?
            .join("bin/ottrin.exe");
        owned_string(&format!("{},0", executable.display()))
    }
    fn GetToolTip(&self, _: Option<&IShellItemArray>) -> Result<PWSTR> {
        owned_string("Open this folder in Ottrin")
    }
    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(CLASS)
    }
    fn GetState(&self, _: Option<&IShellItemArray>, _: BOOL) -> Result<u32> {
        // One bounded local registry read. No directory/network inspection or
        // process launch while Explorer constructs its menu.
        match crate::enabled() {
            Ok(true) => Ok(ECS_ENABLED.0 as u32),
            Ok(false) => Ok(ECS_HIDDEN.0 as u32),
            Err(_) => Err(E_FAIL.into()),
        }
    }
    fn Invoke(&self, items: Option<&IShellItemArray>, _: Option<&IBindCtx>) -> Result<()> {
        if !crate::enabled().map_err(|_| windows::core::Error::from(E_FAIL))? {
            return Err(E_ACCESSDENIED.into());
        }
        let folders = self.folders(items)?;
        let executable = crate::package_directory()
            .map_err(|_| windows::core::Error::from(E_FAIL))?
            .join("bin/ottrin.exe");
        // Resolve and validate every selection before launching any window.
        // Rust passes paths as Windows argv; no command shell or string parser.
        for folder in folders {
            Command::new(&executable)
                .arg("--")
                .arg(folder)
                .spawn()
                .map_err(|_| windows::core::Error::from(E_FAIL))?;
        }
        Ok(())
    }
    fn GetFlags(&self) -> Result<u32> {
        Ok(ECF_DEFAULT.0 as u32)
    }
    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        Err(E_NOTIMPL.into())
    }
}

impl IObjectWithSite_Impl for FolderCommand_Impl {
    fn SetSite(&self, site: Option<&IUnknown>) -> Result<()> {
        let replacement = site.cloned();
        let previous = {
            let mut current = self
                .site
                .try_borrow_mut()
                .map_err(|_| windows::core::Error::from(E_UNEXPECTED))?;
            std::mem::replace(&mut *current, replacement)
        };
        drop(previous);
        Ok(())
    }
    fn GetSite(&self, iid: *const GUID, out: *mut *mut c_void) -> Result<()> {
        if out.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *out = std::ptr::null_mut();
        }
        if iid.is_null() {
            return Err(E_POINTER.into());
        }
        let site = self
            .site
            .try_borrow()
            .map_err(|_| windows::core::Error::from(E_UNEXPECTED))?
            .clone();
        unsafe { site.as_ref().ok_or(E_FAIL)?.query(iid, out).ok() }
    }
}

/// Standard COM activation entry point. The caller supplies writable COM output
/// storage and valid CLSID/IID pointers, as required by the Windows loader ABI.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    class: *const GUID,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe {
        *out = std::ptr::null_mut();
    }
    if class.is_null() || iid.is_null() {
        return E_POINTER;
    }
    if unsafe { *class } != CLASS {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = Factory {
        _lifetime: Lifetime::new(),
    }
    .into();
    unsafe { factory.query(iid, out) }
}

#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECTS.load(Ordering::SeqCst) == 0 && LOCKS.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest_class_and_folder_contexts_match_server() {
        let manifest = include_str!("../../../packaging/windows/msix-manifest.xml");
        assert!(manifest.contains(&format!("{:?}", CLASS).to_ascii_lowercase()));
        let verbs: Vec<_> = manifest
            .split("<desktop5:Verb ")
            .skip(1)
            .chain(manifest.split("<desktop10:Verb ").skip(1))
            .collect();
        assert_eq!(verbs.len(), 3);
        for verb in verbs {
            let id = verb
                .split_once("Id=\"")
                .unwrap()
                .1
                .split('"')
                .next()
                .unwrap();
            assert!(
                !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_alphanumeric()),
                "Packaged Explorer verb identifiers must satisfy the native SDK schema: {id}"
            );
        }
        // Drive is a desktop10 context; desktop5 rejects it in the native SDK.
        assert!(manifest.contains(
            "xmlns:desktop10=\"http://schemas.microsoft.com/appx/manifest/desktop/windows10/10\""
        ));
        assert!(manifest.contains("<desktop10:ItemType Type=\"Drive\"><desktop10:Verb "));
        for context in ["Directory", "Directory\\Background"] {
            assert!(manifest.contains(&format!("Type=\"{context}\"")));
        }
    }
    fn factory_rejects_unknown_class_and_null_output() {
        let mut output = std::ptr::dangling_mut::<c_void>();
        unsafe {
            assert_eq!(
                DllGetClassObject(&GUID::zeroed(), &IClassFactory::IID, &mut output),
                CLASS_E_CLASSNOTAVAILABLE
            );
            assert!(output.is_null());
            assert_eq!(
                DllGetClassObject(&CLASS, &IClassFactory::IID, std::ptr::null_mut()),
                E_POINTER
            );
        }
    }

    #[test]
    fn packaged_shell_com_contract() {
        manifest_class_and_folder_contexts_match_server();
        factory_rejects_unknown_class_and_null_output();
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
            struct Apartment;
            impl Drop for Apartment {
                fn drop(&mut self) {
                    unsafe {
                        CoUninitialize();
                    }
                }
            }
            let _apartment = Apartment;
            let fixture = tempfile::tempdir().unwrap();
            let folder = fixture.path().join("-- folder with spaces Ω𐐷");
            std::fs::create_dir(&folder).unwrap();
            use std::os::windows::ffi::OsStrExt;
            let name: Vec<u16> = folder.as_os_str().encode_wide().chain(Some(0)).collect();
            let item: IShellItem =
                SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None).unwrap();
            let selected = folder_path(&item).unwrap();
            assert_eq!(selected.file_name(), folder.file_name());
            assert_eq!(
                std::fs::canonicalize(selected).unwrap(),
                std::fs::canonicalize(&folder).unwrap()
            );
            let file = fixture.path().join("not a folder.txt");
            std::fs::write(&file, "owned fixture").unwrap();
            let name: Vec<u16> = file.as_os_str().encode_wide().chain(Some(0)).collect();
            let item: IShellItem =
                SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None).unwrap();
            assert!(folder_path(&item).is_err());
            let mut raw = std::ptr::null_mut();
            assert_eq!(
                DllGetClassObject(&CLASS, &IClassFactory::IID, &mut raw),
                S_OK
            );
            let factory = IClassFactory::from_raw(raw);
            assert_eq!(DllCanUnloadNow(), S_FALSE);
            let command: IExplorerCommand = factory.CreateInstance(None).unwrap();
            assert_eq!(command.GetCanonicalName().unwrap(), CLASS);
            assert_eq!(command.GetFlags().unwrap(), ECF_DEFAULT.0 as u32);
            let title = command.GetTitle(None).unwrap();
            assert_eq!(title.to_string().unwrap(), "Open in Ottrin");
            CoTaskMemFree(Some(title.0.cast()));
            let site: IObjectWithSite = command.cast().unwrap();
            site.SetSite(&factory).unwrap();
            let original: IClassFactory = site.GetSite().unwrap();
            assert_eq!(original, factory);
            site.SetSite(None).unwrap();
            assert!(site.GetSite::<IUnknown>().is_err());
            // A surplus unlock is a caller error rather than wrapping an
            // unsigned lock count and permanently pinning the DLL.
            assert!(factory.LockServer(false).is_err());
            factory.LockServer(true).unwrap();
            factory.LockServer(false).unwrap();
        }
    }
}
