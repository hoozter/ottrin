//! Explicit two-principal fixture. Bootstrap/ready files only coordinate this
//! disposable guest; authority-bearing requests still use the real bound pipe.
use super::*;
use serde_json::{Value, json};
use std::io::Seek;
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE,
    Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
    DACL_SECURITY_INFORMATION, GetAce, GetLengthSid, GetSecurityDescriptorControl,
    ImpersonateLoggedOnUser, OWNER_SECURITY_INFORMATION, RevertToSelf, SE_DACL_PROTECTED,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

fn root() -> PathBuf {
    let root = PathBuf::from(std::env::var_os("OTTRIN_CALLER_PROBE_ROOT").unwrap());
    assert!(root.is_absolute() && root.join("results").is_dir());
    root
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
fn save(path: &Path, value: &Value) {
    let temporary = path.with_extension("preparing");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    file.sync_all().unwrap();
    drop(file);
    assert!(!path.exists());
    std::fs::rename(temporary, path).unwrap();
}
fn wait(path: &Path) -> Value {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "Owned companion did not become ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    read(path)
}
fn file_owner(file: &std::fs::File) -> Vec<u8> {
    let (mut owner, mut descriptor) = (ptr::null_mut(), ptr::null_mut());
    assert_eq!(
        unsafe {
            GetSecurityInfo(
                file.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        },
        0
    );
    let _allocation = unsafe { crate::windows_fs::LocalSecurity::from_local_alloc(descriptor) };
    let sid =
        unsafe { std::slice::from_raw_parts(owner.cast::<u8>(), GetLengthSid(owner) as usize) }
            .to_vec();
    assert!(crate::windows_fs::valid_sid(&sid));
    sid
}

fn token_state() -> (u32, i32, Vec<u8>) {
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_STATISTICS, TokenPrivileges, TokenStatistics,
    };
    let token = crate::windows_fs::current_token().unwrap();
    let mut statistics = TOKEN_STATISTICS::default();
    let mut length = 0;
    assert_ne!(
        unsafe {
            GetTokenInformation(
                token.as_raw_handle().cast(),
                TokenStatistics,
                ptr::from_mut(&mut statistics).cast(),
                std::mem::size_of_val(&statistics) as u32,
                &mut length,
            )
        },
        0
    );
    unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenPrivileges,
            ptr::null_mut(),
            0,
            &mut length,
        );
    }
    assert!((4..=65536).contains(&length));
    let mut storage = vec![0u64; (length as usize).div_ceil(8)];
    let capacity = storage.len() * 8;
    assert_ne!(
        unsafe {
            GetTokenInformation(
                token.as_raw_handle().cast(),
                TokenPrivileges,
                storage.as_mut_ptr().cast(),
                capacity as u32,
                &mut length,
            )
        },
        0
    );
    assert!(length as usize <= capacity);
    let privileges =
        unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), length as usize) }
            .to_vec();
    (
        statistics.TokenId.LowPart,
        statistics.TokenId.HighPart,
        privileges,
    )
}

fn system_parent_policy(file: &std::fs::File) -> Vec<u8> {
    let (mut dacl, mut descriptor) = (ptr::null_mut(), ptr::null_mut());
    assert_eq!(
        unsafe {
            GetSecurityInfo(
                file.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        },
        0
    );
    let _allocation = unsafe { crate::windows_fs::LocalSecurity::from_local_alloc(descriptor) };
    let (mut control, mut revision) = (0, 0);
    assert_ne!(
        unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
        0
    );
    assert_ne!(control & SE_DACL_PROTECTED, 0);
    assert!(!dacl.is_null());
    assert_eq!(unsafe { (*dacl).AceCount }, 2);
    let mut principals = std::collections::BTreeSet::new();
    for index in 0..2 {
        let mut raw = ptr::null_mut();
        assert_ne!(unsafe { GetAce(dacl, index, &mut raw) }, 0);
        let ace = unsafe { &*raw.cast::<ACCESS_ALLOWED_ACE>() };
        assert_eq!(ace.Header.AceType, 0); // ACCESS_ALLOWED_ACE_TYPE.
        assert_eq!(ace.Header.AceFlags, 3); // Only object/container inheritance.
        assert_eq!(
            ace.Mask,
            windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS
        );
        let sid = ptr::addr_of!(ace.SidStart)
            .cast_mut()
            .cast::<std::ffi::c_void>();
        principals.insert(
            unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), GetLengthSid(sid) as usize) }
                .to_vec(),
        );
    }
    let system = vec![1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
    let administrators = vec![1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0];
    assert_eq!(
        principals,
        [system.clone(), administrators.clone()]
            .into_iter()
            .collect()
    );
    let owner = file_owner(file);
    assert!(owner == system || owner == administrators);
    owner
}

#[test]
#[ignore = "requires an owned ordinary client and SYSTEM companion in an isolated guest"]
fn client() {
    let root = root();
    let results = root.join("results");
    let protected = root.join("system-only");
    assert_eq!(
        std::fs::read_dir(&protected).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    let source = root.join("probe-data/caller-source.txt");
    std::fs::write(&source, b"unchanged caller source").unwrap();
    let nonce = nonce();
    let mut listener = LocalPipeListener::bind(&endpoint(&nonce).unwrap(), ACL).unwrap();
    let token = crate::windows_fs::current_token().unwrap();
    let user = crate::windows_fs::user_for_token(&token).unwrap();
    let owner = crate::windows_fs::owner_for_token(&token).unwrap();
    let group = crate::windows_fs::group_for_token(&token).unwrap();
    let original_token_state = token_state();
    let denied = crate::windows_fs::with_file_privileges(|| Ok(())).unwrap_err();
    assert_eq!(denied.raw_os_error(), Some(1300)); // ERROR_NOT_ALL_ASSIGNED.
    assert_eq!(token_state(), original_token_state);
    let profile = {
        let start = process_start_time(unsafe { GetCurrentProcess() }).unwrap();
        CallerProcess::capture(std::process::id(), start)
            .unwrap()
            .local_app_data()
            .unwrap()
    };
    save(
        &results.join("CALLER_READY.json"),
        &json!({
            "nonce":nonce, "pid":std::process::id(),
            "start":process_start_time(unsafe { GetCurrentProcess() }).unwrap(),
        }),
    );
    let helper = wait(&results.join("SYSTEM_PROCESS.json"));
    let helper_pid = helper["pid"].as_u64().unwrap().try_into().unwrap();
    let mut pipe = listener
        .accept(Duration::from_secs(30), |peer| verify_pid(peer, helper_pid))
        .unwrap()
        .unwrap();
    write_json(
        &mut pipe,
        &PrivilegedRequest {
            protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
            command: PrivilegedCommand::ListDirectory {
                path: protected,
                show_hidden: false,
            },
            context: PrivilegedContext {
                reason: Some("owned caller-owner mechanism probe".into()),
                cwd: None,
            },
        },
    )
    .unwrap();
    let response: PrivilegedResponse = read_json(&mut pipe).unwrap();
    assert_eq!(response.status, PrivilegedStatus::Success);
    assert!(response.payload.is_none());
    let observation: Value = serde_json::from_str(response.message.as_ref().unwrap()).unwrap();
    assert_eq!(observation["caller_user"], json!(user));
    assert_eq!(observation["caller_owner"], json!(owner));
    assert_eq!(observation["caller_group"], json!(group));
    assert_eq!(observation["caller_profile"], json!(profile));
    assert_ne!(observation["helper_user"], json!(user));
    assert_eq!(
        observation["helper_user"],
        json!([1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0])
    );
    assert_eq!(std::fs::read(&source).unwrap(), b"unchanged caller source");
    assert!(observation["thread_identity_restored"].as_bool().unwrap());
    assert!(observation["profile_still_valid"].as_bool().unwrap());
    assert!(
        observation["control_creation_owner_matches_caller"]
            .as_bool()
            .unwrap()
    );
    assert!(
        observation["atomic_private_creation_owner_matches_caller"]
            .as_bool()
            .unwrap()
    );
    assert!(
        observation["existing_private_entry_refused_unchanged"]
            .as_bool()
            .unwrap()
    );
    assert!(
        observation["ordinary_helper_reopen_refused"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        std::fs::read(root.join("system-only/atomic-owner Ω/file.txt")).unwrap(),
        b"original caller owns protected private file"
    );
    assert_eq!(
        std::fs::read(root.join("probe-data/caller-control Ω/handle-proof.txt")).unwrap(),
        b"elevated IO through caller-owned handle"
    );
    save(&results.join("OWNER_OBSERVATION.json"), &observation);
    println!("Native mechanism observation recorded; this does not enable privileged Copy");
}

#[test]
#[ignore = "SYSTEM-side companion for the owned native caller-owner mechanism probe"]
fn system_helper() {
    let root = root();
    let ready = read(&root.join("results/CALLER_READY.json"));
    let mut channel = HelperChannel::connect(
        ready["nonce"].as_str().unwrap(),
        ready["pid"].as_u64().unwrap().try_into().unwrap(),
        ready["start"].as_u64().unwrap(),
    )
    .unwrap();
    let request = channel.read_request().unwrap();
    assert_eq!(
        request.context.reason.as_deref(),
        Some("owned caller-owner mechanism probe")
    );
    let PrivilegedCommand::ListDirectory {
        path,
        show_hidden: false,
    } = request.command
    else {
        panic!("Wrong owned probe request")
    };
    assert_eq!(path, root.join("system-only"));
    let context = channel.caller_copy_context().unwrap();
    let helper_token = crate::windows_fs::current_token().unwrap();
    let helper_user = crate::windows_fs::user_for_token(&helper_token).unwrap();
    let helper_owner = crate::windows_fs::owner_for_token(&helper_token).unwrap();
    assert_eq!(helper_user, [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0]);
    let caller_owner = context.default_file_owner_sid().unwrap().to_vec();
    assert_ne!(helper_user, context.user_sid().unwrap());
    let parent =
        crate::windows_fs::open_directory(&path, FILE_GENERIC_READ | FILE_GENERIC_WRITE).unwrap();
    // Directory ownership belongs to its creating context; it need not equal
    // this helper process's default owner. Verify the actual protected policy.
    let parent_owner = system_parent_policy(&parent);
    let original_helper_state = token_state();
    let control_parent = crate::windows_fs::open_directory(
        &root.join("probe-data"),
        FILE_GENERIC_READ | FILE_GENERIC_WRITE,
    )
    .unwrap();
    let token = process_token(
        &channel.caller.process,
        TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE,
    )
    .unwrap();
    assert!(CallerIdentity::capture(&token).unwrap() == channel.caller.identity);
    assert!(
        crate::copy_transaction::CallerCreationSecurity::capture(
            &token,
            &helper_owner,
            context.default_file_group_sid().unwrap(),
            false
        )
        .is_err()
    );
    assert_ne!(context.default_file_group_sid().unwrap(), helper_owner);
    assert!(
        crate::copy_transaction::CallerCreationSecurity::capture(
            &token,
            &caller_owner,
            &helper_owner,
            false
        )
        .is_err()
    );
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            // Never continue a privileged fixture with an unknown thread identity.
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
        }
    }
    assert_ne!(
        unsafe { ImpersonateLoggedOnUser(token.as_raw_handle().cast()) },
        0
    );
    let revert = Revert;
    assert_eq!(crate::windows_fs::current_owner().unwrap(), caller_owner);
    // Positive control on the same volume: the canonical creator must work
    // under the original token where that user actually has directory access.
    let control = crate::windows_fs::create_private(
        &control_parent,
        OsStr::new("caller-control Ω"),
        true,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
    .unwrap();
    assert_eq!(file_owner(&control), caller_owner);
    let mut control_file = crate::windows_fs::create_private(
        &control,
        OsStr::new("handle-proof.txt"),
        false,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
    .unwrap();
    assert_eq!(file_owner(&control_file), caller_owner);
    let created = crate::windows_fs::create_private(
        &parent,
        OsStr::new("caller-owned Ω"),
        true,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    );
    let creation = match created {
        Ok(file) => {
            assert_eq!(file_owner(&file), caller_owner);
            json!({"created":true,"owner_matches_caller":true})
        }
        Err(error) => {
            // These are mechanism observations, not successful Copy acceptance.
            assert!(
                matches!(error.raw_os_error(), Some(5 | 1314)),
                "Unexpected native error: {error}"
            );
            json!({"created":false,"native_error":error.raw_os_error(),"message":error.to_string()})
        }
    };
    drop(revert);
    assert_eq!(crate::windows_fs::current_owner().unwrap(), helper_owner);
    // Already granted handles retain their authority after restoring the
    // helper identity; neither owner nor DACL is changed to make this work.
    control_file
        .write_all(b"elevated IO through caller-owned handle")
        .unwrap();
    control_file.sync_all().unwrap();
    assert_eq!(file_owner(&control_file), caller_owner);
    assert_eq!(system_parent_policy(&parent), parent_owner);
    let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    let atomic = context
        .create_private_entry(&parent, OsStr::new("atomic-owner Ω"), true, sharing)
        .unwrap();
    assert_eq!(token_state(), original_helper_state);
    assert_eq!(file_owner(&atomic), caller_owner);
    let mut atomic_file = context
        .create_private_entry(&atomic, OsStr::new("file.txt"), false, sharing)
        .unwrap();
    assert_eq!(file_owner(&atomic_file), caller_owner);
    atomic_file
        .write_all(b"original caller owns protected private file")
        .unwrap();
    atomic_file.sync_all().unwrap();
    let id = crate::move_recovery::stable_identity_from_handle(&atomic_file).unwrap();
    let directory_id = crate::move_recovery::stable_identity_from_handle(&atomic).unwrap();
    let reopened_directory = context
        .open_private_entry(
            &parent,
            OsStr::new("atomic-owner Ω"),
            directory_id,
            true,
            sharing,
        )
        .unwrap();
    assert_eq!(
        crate::move_recovery::stable_identity_from_handle(&reopened_directory).unwrap(),
        directory_id
    );
    let mut readonly = context
        .open_private_entry(
            &reopened_directory,
            OsStr::new("file.txt"),
            id,
            false,
            sharing,
        )
        .unwrap();
    let mut reopened_bytes = Vec::new();
    readonly.read_to_end(&mut reopened_bytes).unwrap();
    assert_eq!(
        reopened_bytes,
        b"original caller owns protected private file"
    );
    assert_eq!(
        readonly.write_all(b"must not write").unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    let mut reopened_writer = context
        .open_private_entry(&atomic, OsStr::new("file.txt"), id, true, sharing)
        .unwrap();
    reopened_writer
        .write_all(b"original caller owns protected private file")
        .unwrap();
    reopened_writer.sync_all().unwrap();
    let replacement = context
        .create_private_entry(&atomic, OsStr::new("other.txt"), false, sharing)
        .unwrap();
    assert_ne!(
        crate::move_recovery::stable_identity_from_handle(&replacement).unwrap(),
        id
    );
    assert_eq!(
        context
            .open_private_entry(&atomic, OsStr::new("other.txt"), id, true, sharing)
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        context
            .open_private_entry(&atomic, OsStr::new("missing.txt"), id, true, sharing)
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        context
            .open_private_entry(&atomic_file, OsStr::new("file.txt"), id, true, sharing)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    let source_file = std::fs::File::open(root.join("probe-data/caller-source.txt")).unwrap();
    let source_id = crate::move_recovery::stable_identity_from_handle(&source_file).unwrap();
    assert!(
        context
            .open_private_entry(
                &control_parent,
                OsStr::new("caller-source.txt"),
                source_id,
                false,
                sharing
            )
            .is_err()
    );
    let changed = context
        .create_private_entry(&atomic, OsStr::new("changed-policy.txt"), false, sharing)
        .unwrap();
    let changed_id = crate::move_recovery::stable_identity_from_handle(&changed).unwrap();
    let policy =
        crate::windows_fs::LocalSecurity::from_sddl("D:P(A;;FA;;;OW)(A;;FR;;;WD)").unwrap();
    let (mut present, mut defaulted, mut dacl) = (0, 0, ptr::null_mut());
    assert_ne!(
        unsafe {
            windows_sys::Win32::Security::GetSecurityDescriptorDacl(
                policy.as_raw(),
                &mut present,
                &mut dacl,
                &mut defaulted,
            )
        },
        0
    );
    assert_eq!(
        unsafe {
            windows_sys::Win32::Security::Authorization::SetSecurityInfo(
                changed.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION
                    | windows_sys::Win32::Security::PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null(),
            )
        },
        0
    );
    let changed_security = crate::copy_transaction::owner_group_dacl_digest(&changed).unwrap();
    assert!(
        context
            .open_private_entry(
                &atomic,
                OsStr::new("changed-policy.txt"),
                changed_id,
                true,
                sharing
            )
            .is_err()
    );
    assert_eq!(
        crate::copy_transaction::owner_group_dacl_digest(&changed).unwrap(),
        changed_security
    );
    assert_eq!(token_state(), original_helper_state);
    crate::windows_fs::with_file_privileges(|| {
        let outer_state = token_state();
        assert_eq!(
            std::thread::spawn(token_state).join().unwrap(),
            original_helper_state
        );
        let nested =
            context.create_private_entry(&atomic, OsStr::new("nested.txt"), false, sharing)?;
        assert_eq!(file_owner(&nested), caller_owner);
        assert_eq!(token_state(), outer_state);
        assert_eq!(
            context
                .create_private_entry(&atomic, OsStr::new("nested.txt"), false, sharing)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(token_state(), outer_state);
        Ok(())
    })
    .unwrap();
    assert_eq!(token_state(), original_helper_state);
    let foreign_path = root.join("system-only/existing-system-owned.txt");
    std::fs::write(&foreign_path, b"existing foreign data stays intact").unwrap();
    let foreign = std::fs::File::open(&foreign_path).unwrap();
    let foreign_id = crate::move_recovery::stable_identity_from_handle(&foreign).unwrap();
    let foreign_security = crate::copy_transaction::owner_group_dacl_digest(&foreign).unwrap();
    assert_ne!(file_owner(&foreign), caller_owner);
    assert!(
        context
            .open_private_entry(
                &parent,
                OsStr::new("existing-system-owned.txt"),
                foreign_id,
                true,
                sharing
            )
            .is_err()
    );
    assert_eq!(
        context
            .create_private_entry(
                &parent,
                OsStr::new("existing-system-owned.txt"),
                false,
                sharing
            )
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(
        crate::move_recovery::stable_identity_from_handle(&foreign).unwrap(),
        foreign_id
    );
    assert_eq!(
        crate::copy_transaction::owner_group_dacl_digest(&foreign).unwrap(),
        foreign_security
    );
    assert_eq!(
        std::fs::read(&foreign_path).unwrap(),
        b"existing foreign data stays intact"
    );
    assert_eq!(token_state(), original_helper_state);
    assert_eq!(
        context
            .create_private_entry(&atomic, OsStr::new("file.txt"), false, sharing)
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(
        crate::move_recovery::stable_identity_from_handle(&atomic_file).unwrap(),
        id
    );
    atomic_file.rewind().unwrap();
    let mut bytes = Vec::new();
    atomic_file.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"original caller owns protected private file");
    assert_eq!(file_owner(&atomic_file), caller_owner);
    assert_eq!(token_state(), original_helper_state);
    assert_eq!(
        std::fs::read(root.join("system-only/atomic-owner Ω/file.txt"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(crate::windows_fs::current_owner().unwrap(), helper_owner);
    assert_eq!(
        crate::windows_fs::user_for_token(&crate::windows_fs::current_token().unwrap()).unwrap(),
        helper_user
    );
    assert_eq!(system_parent_policy(&parent), parent_owner);
    context.validate().unwrap();
    channel.write_response(&PrivilegedResponse {
        status:PrivilegedStatus::Success, payload:None,
        message:Some(json!({
            "caller_user":context.user_sid().unwrap(),"caller_owner":caller_owner,
            "caller_group":context.default_file_group_sid().unwrap(),
            "caller_profile":context.local_app_data().unwrap(),"helper_user":helper_user,
            "protected_parent_owner":parent_owner,"protected_parent_dacl_verified":true,
            "control_creation_owner_matches_caller":true,
            "atomic_private_creation_owner_matches_caller":true,
            "existing_private_entry_refused_unchanged":true,
            "ordinary_helper_reopen_refused":true,
            "exact_thread_token_and_privileges_restored":true,
            "nested_privileged_thread_restored":true,
            "foreign_owner_entry_refused_without_data_or_security_change":true,
            "derived_policy_refuses_owner_or_group_outside_admitted_identity":true,
            "identity_bound_private_reopen":true,
            "read_only_reopen_denies_writes":true,
            "foreign_nonprivate_changed_policy_reopen_refused":true,
            "reopen_refuses_replacement_or_missing":true,
            "creation_with_original_token_and_elevated_parent_handle":creation,
            "thread_identity_restored":true,"profile_still_valid":true,
            "scope":"Distinct SYSTEM helper mechanism observation; no alternate-administrator UAC or enabled Copy acceptance",
        }).to_string()),
    }).unwrap();
}
