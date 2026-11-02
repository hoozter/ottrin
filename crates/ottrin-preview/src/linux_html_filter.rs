//! A baseline inherited by the browser before it can start renderer processes.
//! Chromium's additional process-specific sandbox remains enabled. This layer
//! denies kernel inspection/injection and optional kernel interfaces that static
//! HTML previews do not need; it is not a replacement for Chromium's policy.
//! ABI: https://docs.kernel.org/userspace-api/seccomp_filter.html
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd};

fn program() -> Result<Vec<libc::sock_filter>, String> {
    let arch = if cfg!(target_arch = "x86_64") {
        0xc000003e // AUDIT_ARCH_X86_64
    } else if cfg!(target_arch = "aarch64") {
        0xc00000b7 // AUDIT_ARCH_AARCH64
    } else {
        return Err("HTML syscall confinement is unavailable for this architecture".into());
    };
    let instruction = |code, jt, jf, k| libc::sock_filter { code, jt, jf, k };
    const LOAD: u16 = (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16;
    const EQUAL: u16 = (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16;
    const RETURN: u16 = (libc::BPF_RET | libc::BPF_K) as u16;
    let mut code = vec![
        instruction(LOAD, 0, 0, 4), // seccomp_data.arch
        instruction(EQUAL, 1, 0, arch),
        instruction(RETURN, 0, 0, libc::SECCOMP_RET_KILL_PROCESS),
        instruction(LOAD, 0, 0, 0), // seccomp_data.nr
    ];
    // x32 shares the x86-64 audit architecture but uses different syscall IDs.
    if cfg!(target_arch = "x86_64") {
        code.push(instruction(
            (libc::BPF_JMP | libc::BPF_JGE | libc::BPF_K) as u16,
            0,
            1,
            0x40000000,
        ));
        code.push(instruction(RETURN, 0, 0, libc::SECCOMP_RET_KILL_PROCESS));
    }
    for syscall in [
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_userfaultfd,
        libc::SYS_io_uring_setup,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_open_by_handle_at,
    ] {
        code.push(instruction(EQUAL, 0, 1, syscall as u32));
        code.push(instruction(
            RETURN,
            0,
            0,
            libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
        ));
    }
    code.push(instruction(RETURN, 0, 0, libc::SECCOMP_RET_ALLOW));
    Ok(code)
}

/// Bubblewrap loads the sealed program after creating its namespaces and before
/// executing the browser. Only the trusted child receives this descriptor.
pub(crate) fn file() -> Result<File, String> {
    let code = program()?;
    // SAFETY: constant NUL-terminated name and valid memfd flags.
    let fd = unsafe {
        libc::memfd_create(
            c"ottrin-html-seccomp".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: memfd_create returned a fresh, owned descriptor.
    let mut file = unsafe { File::from_raw_fd(fd) };
    for instruction in code {
        file.write_all(&instruction.code.to_ne_bytes())
            .and_then(|_| file.write_all(&[instruction.jt, instruction.jf]))
            .and_then(|_| file.write_all(&instruction.k.to_ne_bytes()))
            .map_err(|e| e.to_string())?;
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    // SAFETY: fcntl operates on the owned memfd with documented seal flags.
    if unsafe {
        libc::fcntl(
            file.as_raw_fd(),
            libc::F_ADD_SEALS,
            libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL,
        )
    } < 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn self_read() -> Result<u8, i32> {
        let source = 73_u8;
        let mut destination = 0_u8;
        let local = libc::iovec {
            iov_base: (&mut destination as *mut u8).cast(),
            iov_len: 1,
        };
        let remote = libc::iovec {
            iov_base: (&source as *const u8).cast_mut().cast(),
            iov_len: 1,
        };
        // SAFETY: both iovecs refer to live one-byte objects in this process.
        let read = unsafe { libc::process_vm_readv(libc::getpid(), &local, 1, &remote, 1, 0) };
        if read == 1 {
            Ok(destination)
        } else {
            Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(-1))
        }
    }

    #[test]
    fn inherited_html_filter_denies_real_self_inspection_in_an_owned_child() {
        assert_eq!(self_read(), Ok(73), "Unfiltered positive control must work");
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux_html_filter::tests::filtered_child",
                "--ignored",
                "--nocapture",
            ])
            .env("OTTRIN_HTML_FILTER_TEST_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    #[ignore = "Only the owning test launches this fresh process to install an irreversible filter"]
    fn filtered_child() {
        assert_eq!(
            std::env::var("OTTRIN_HTML_FILTER_TEST_CHILD").as_deref(),
            Ok("1")
        );
        assert_eq!(self_read(), Ok(73));
        let mut code = program().unwrap();
        let description = libc::sock_fprog {
            len: code.len().try_into().unwrap(),
            filter: code.as_mut_ptr(),
        };
        // SAFETY: this disposable child owns its irreversible policy; BPF storage
        // remains live while the kernel copies the complete sock_fprog.
        unsafe {
            assert_eq!(libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
            assert_eq!(
                libc::prctl(
                    libc::PR_SET_SECCOMP,
                    libc::SECCOMP_MODE_FILTER,
                    &description
                ),
                0
            );
        }
        assert_eq!(self_read(), Err(libc::EPERM));
        let mut sealed = file().unwrap();
        assert_eq!(
            sealed.write(b"changed").unwrap_err().raw_os_error(),
            Some(libc::EPERM)
        );
    }
}
