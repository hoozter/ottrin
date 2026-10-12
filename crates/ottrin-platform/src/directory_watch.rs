//! Session directory identity guards without long-lived mount-pinning handles.
use rustix::fs::inotify;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

type Identity = (u64, u64);
#[derive(Debug)]
struct Registration {
    wd: i32,
    valid: Weak<AtomicBool>,
}
#[derive(Debug)]
struct State {
    fd: std::os::fd::OwnedFd,
    registrations: HashMap<Identity, Registration>,
}
#[derive(Debug)]
struct Registry(Mutex<State>);
static REGISTRY: OnceLock<Mutex<Weak<Registry>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub(super) struct DirectoryWatch {
    registry: Arc<Registry>,
    identity: Identity,
    valid: Arc<AtomicBool>,
}

impl State {
    fn drain(&mut self) -> io::Result<()> {
        let mut buffer = [std::mem::MaybeUninit::uninit(); 4096];
        let mut reader = inotify::Reader::new(&self.fd, &mut buffer);
        loop {
            let event = match reader.next() {
                Ok(event) => event,
                Err(rustix::io::Errno::WOULDBLOCK) => return Ok(()),
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => {
                    for record in self.registrations.values() {
                        if let Some(valid) = record.valid.upgrade() {
                            valid.store(false, Ordering::SeqCst);
                        }
                    }
                    return Err(error.into());
                }
            };
            // Overflow invalidates every proof: a lost deletion/unmount event
            // must never silently permit a recycled directory identity.
            let overflow = event.events().contains(inotify::ReadFlags::QUEUE_OVERFLOW);
            for record in self.registrations.values() {
                if (overflow || record.wd == event.wd())
                    && let Some(valid) = record.valid.upgrade()
                {
                    valid.store(false, Ordering::SeqCst);
                }
            }
        }
    }
}

impl DirectoryWatch {
    pub(super) fn new(directory: &File) -> io::Result<Self> {
        let metadata = directory.metadata()?;
        let identity = (metadata.dev(), metadata.ino());
        let registry = {
            let mut global = REGISTRY
                .get_or_init(|| Mutex::new(Weak::new()))
                .lock()
                .map_err(|_| io::Error::other("directory watch registry poisoned"))?;
            if let Some(registry) = global.upgrade() {
                registry
            } else {
                let registry = Arc::new(Registry(Mutex::new(State {
                    fd: inotify::init(
                        inotify::CreateFlags::NONBLOCK | inotify::CreateFlags::CLOEXEC,
                    )?,
                    registrations: HashMap::new(),
                })));
                *global = Arc::downgrade(&registry);
                registry
            }
        };
        let valid = {
            let mut state = registry
                .0
                .lock()
                .map_err(|_| io::Error::other("directory watch registry poisoned"))?;
            state.drain()?;
            if let Some(valid) = state
                .registrations
                .get(&identity)
                .and_then(|record| record.valid.upgrade())
            {
                if !valid.load(Ordering::SeqCst) {
                    return Err(io::Error::other("directory watch identity invalidated"));
                }
                valid
            } else {
                // Watch the already-open object, not its possibly redirected name.
                let path = format!("/proc/self/fd/{}", directory.as_raw_fd());
                let wd = inotify::add_watch(
                    &state.fd,
                    path.as_str(),
                    inotify::WatchFlags::ONLYDIR | inotify::WatchFlags::DELETE_SELF,
                )?;
                let valid = Arc::new(AtomicBool::new(true));
                state.registrations.insert(
                    identity,
                    Registration {
                        wd,
                        valid: Arc::downgrade(&valid),
                    },
                );
                valid
            }
        };
        Ok(Self {
            registry,
            identity,
            valid,
        })
    }

    pub(super) fn validate(&self) -> io::Result<()> {
        self.registry
            .0
            .lock()
            .map_err(|_| io::Error::other("directory watch registry poisoned"))?
            .drain()?;
        if self.valid.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(io::Error::other(
                "directory was deleted, unmounted, or its watch invalidated",
            ))
        }
    }
}

impl Drop for DirectoryWatch {
    fn drop(&mut self) {
        if let Ok(mut state) = self.registry.0.lock()
            && Arc::strong_count(&self.valid) == 1
            && let Some(record) = state.registrations.remove(&self.identity)
        {
            // Kernel removal can already have happened after deletion/unmount.
            // Drain its ignored notification before a new watch is registered.
            let _ = inotify::remove_watch(&state.fd, record.wd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletion_invalidates_cloned_watch_without_invalidating_other_directory() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        let other = root.path().join("other");
        std::fs::create_dir(&original).unwrap();
        std::fs::create_dir(&other).unwrap();
        let watch = DirectoryWatch::new(&File::open(&original).unwrap()).unwrap();
        let clone = watch.clone();
        let other_watch = DirectoryWatch::new(&File::open(&other).unwrap()).unwrap();
        drop(watch);
        clone.validate().unwrap();
        std::fs::remove_dir(&original).unwrap();
        std::fs::create_dir(&original).unwrap();
        assert!(clone.validate().is_err());
        other_watch.validate().unwrap();
    }

    #[test]
    fn large_batches_share_one_instance_and_releasing_one_owner_keeps_other_proofs() {
        let root = tempfile::tempdir().unwrap();
        let mut watches: Vec<DirectoryWatch> = Vec::new();
        for index in 0..500 {
            let path = root.path().join(index.to_string());
            std::fs::create_dir(&path).unwrap();
            let watch = DirectoryWatch::new(&File::open(&path).unwrap()).unwrap();
            if let Some(first) = watches.first() {
                assert!(Arc::ptr_eq(&watch.registry, &first.registry));
            }
            watches.push(watch);
        }
        let survivor = watches[0].clone();
        drop(watches);
        survivor.validate().unwrap();
        std::fs::remove_dir(root.path().join("0")).unwrap();
        assert!(survivor.validate().is_err());
    }
}
