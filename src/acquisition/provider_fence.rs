//! Kernel-enforced provider immutability; no process-wide signal handler.
//!
//! A read lease rejects pre-existing writers and a break request invalidates
//! the acquisition. Notifications target this synchronous thread while blocked.
//! All leases are released before its prior signal mask is restored.

use super::invalid;
pub(super) fn io(e: &std::io::Error) -> Error {
    Error::CannotVerify(format!("provider IO: {e}"))
}
use crate::{Error, Result};
use std::fs::File;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native {
    use super::*;
    use std::marker::PhantomData;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;
    use std::rc::Rc;

    pub(super) struct SignalMask {
        old: libc::sigset_t,
        set: libc::sigset_t,
        _thread: PhantomData<Rc<()>>,
    }

    #[allow(unsafe_code)]
    impl SignalMask {
        pub(super) fn new() -> Result<Self> {
            // SAFETY: initialized stack sigsets passed to libc; this guard is
            // !Send and restores the mask on the same synchronous thread.
            unsafe {
                // A caller-owned SIGIO handler is incompatible with draining
                // lease notifications. Refuse rather than steal its traffic.
                let mut action: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(libc::SIGIO, std::ptr::null(), &mut action) != 0
                    || action.sa_sigaction != libc::SIG_DFL
                {
                    return Err(invalid("provider notification signal already in use"));
                }
                let mut set = std::mem::zeroed();
                let mut old = std::mem::zeroed();
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, libc::SIGIO);
                if libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut old) != 0 {
                    return Err(invalid("provider notification fence unavailable"));
                }
                let guard = Self {
                    old,
                    set,
                    _thread: PhantomData,
                };
                let mut pending = std::mem::zeroed();
                let pending_status = libc::sigpending(&mut pending);
                if pending_status != 0
                    || libc::sigismember(&pending, libc::SIGIO) == 1
                    || libc::sigismember(&guard.old, libc::SIGIO) == 1
                {
                    // Do not consume a notification owned by another caller.
                    libc::pthread_sigmask(libc::SIG_SETMASK, &guard.old, std::ptr::null_mut());
                    std::mem::forget(guard);
                    return Err(invalid("provider notification thread already in use"));
                }
                Ok(guard)
            }
        }
    }

    #[allow(unsafe_code)]
    impl Drop for SignalMask {
        fn drop(&mut self) {
            // SAFETY: all our leased files are already closed; drain their
            // thread-targeted notifications before restoring the prior mask.
            unsafe {
                let timeout = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                };
                while libc::sigtimedwait(&self.set, std::ptr::null_mut(), &timeout) >= 0 {}
                libc::pthread_sigmask(libc::SIG_SETMASK, &self.old, std::ptr::null_mut());
            }
        }
    }

    pub(super) struct Lease {
        pub file: File,
        stamp: (u64, u64, u64, i64, i64, i64, i64),
    }

    fn stamp(file: &File) -> Result<(u64, u64, u64, i64, i64, i64, i64)> {
        let m = file.metadata().map_err(|e| io(&e))?;
        if !m.is_file() {
            return Err(invalid("provider is not a regular file"));
        }
        Ok((
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        ))
    }

    #[allow(unsafe_code)]
    impl Lease {
        pub(super) fn new(path: &str) -> Result<Self> {
            let file = File::open(path).map_err(|e| io(&e))?;
            let stamp = stamp(&file)?;
            let fd = file.as_raw_fd();
            // SAFETY: fd is owned/open; kernel ABI struct contains two ints.
            // Set the thread owner BEFORE F_SETLEASE (which preserves an
            // existing owner), so SIGIO can never hit an unblocked thread.
            unsafe {
                #[repr(C)]
                struct Owner {
                    kind: libc::c_int,
                    pid: libc::pid_t,
                }
                let owner = Owner {
                    kind: 0,
                    pid: libc::syscall(libc::SYS_gettid) as libc::pid_t,
                };
                if libc::fcntl(
                    fd, 15, /* Linux x86_64/aarch64 UAPI F_SETOWN_EX */
                    &owner,
                ) < 0
                    || libc::fcntl(fd, libc::F_SETLEASE, libc::F_RDLCK) < 0
                {
                    return Err(invalid(format!(
                        "provider read lease unavailable: {}",
                        std::io::Error::last_os_error()
                    )));
                }
            }
            Ok(Self { file, stamp })
        }

        pub(super) fn check(&self, path: &str) -> Result<()> {
            // SAFETY: GETLEASE uses only the live owned descriptor.
            let held = unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_GETLEASE) };
            let m = std::fs::metadata(path).map_err(|e| io(&e))?;
            if held != libc::F_RDLCK
                || stamp(&self.file)? != self.stamp
                || (m.dev(), m.ino()) != (self.stamp.0, self.stamp.1)
            {
                return Err(invalid("provider generation changed or lease broke"));
            }
            Ok(())
        }

        pub(super) fn opaque_id(&self) -> String {
            crate::share::sha256(format!("{:?}", self.stamp).as_bytes())
        }
    }
}

/// Drop ordering matters: close leased files, THEN unblock notifications.
pub(super) struct Fence {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    leases: Vec<(String, native::Lease)>,
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    _mask: Option<native::SignalMask>,
}

impl Fence {
    pub(super) fn new(paths: &[String]) -> Result<Self> {
        #[cfg(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        {
            let mask = if paths.is_empty() {
                None
            } else {
                Some(native::SignalMask::new()?)
            };
            let mut fence = Self {
                leases: Vec::new(),
                _mask: mask,
            };
            for path in paths {
                fence.leases.push((path.clone(), native::Lease::new(path)?));
            }
            Ok(fence)
        }
        #[cfg(not(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )))]
        {
            if !paths.is_empty() {
                return Err(invalid(
                    "attached acquisition needs a supported provider fence",
                ));
            }
            Ok(Self {})
        }
    }

    pub(super) fn file(&self, index: usize) -> Result<&File> {
        #[cfg(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        {
            self.leases
                .get(index)
                .map(|(_, l)| &l.file)
                .ok_or_else(|| invalid("provider fence missing"))
        }
        #[cfg(not(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )))]
        {
            let _ = index;
            Err(invalid("provider fence unavailable"))
        }
    }

    pub(super) fn opaque_id(&self, index: usize) -> Result<String> {
        #[cfg(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        {
            self.leases
                .get(index)
                .map(|(_, l)| l.opaque_id())
                .ok_or_else(|| invalid("provider fence missing"))
        }
        #[cfg(not(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )))]
        {
            let _ = index;
            Err(invalid("provider fence unavailable"))
        }
    }

    pub(super) fn check(&self) -> Result<()> {
        #[cfg(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        for (path, lease) in &self.leases {
            lease.check(path)?;
        }
        Ok(())
    }
}

/// Ask SQLite's actual VFS whether the mounted file moved. A path stat alone
/// cannot establish that a connection is still using that file's descriptor.
#[allow(unsafe_code)]
pub(super) fn mounted_file_unchanged(store: &crate::Store, alias: &str) -> Result<()> {
    let alias = std::ffi::CString::new(alias).map_err(|e| invalid(e.to_string()))?;
    let mut moved: std::os::raw::c_int = -1;
    // SAFETY: the live connection is exclusively borrowed by this synchronous
    // acquisition; SQLite writes one c_int into the initialized stack slot.
    let rc = unsafe {
        rusqlite::ffi::sqlite3_file_control(
            store.conn.handle(),
            alias.as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_HAS_MOVED,
            (&raw mut moved).cast(),
        )
    };
    if rc != rusqlite::ffi::SQLITE_OK || moved != 0 {
        return Err(Error::InvalidValue(
            "mounted provider handle changed or cannot be checked".into(),
        ));
    }
    Ok(())
}
