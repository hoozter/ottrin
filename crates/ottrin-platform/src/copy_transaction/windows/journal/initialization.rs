//! Construction boundaries for persistent, unpublished journal preparations.
use super::*;

const PREPARATION_PREFIX: &str = ".preparing-";

pub(super) fn preparation_name(transaction: &str) -> OsString {
    OsString::from(format!("{PREPARATION_PREFIX}{transaction}"))
}

pub(super) fn prepare(central: &File, transaction: &str) -> Result<File, PlatformError> {
    windows_fs::create_private(
        central,
        &preparation_name(transaction),
        false,
        FILE_SHARE_READ,
    )
    .map_err(io)
}

/// A presentation candidate only: preserve native names without adopting an ID.
pub(super) fn preparation_candidate(name: &OsStr) -> bool {
    let mut units = name.encode_wide();
    PREPARATION_PREFIX.bytes().all(|byte| {
        units.next().is_some_and(|unit| {
            let folded = if (65..=90).contains(&unit) {
                unit + 32
            } else {
                unit
            };
            folded == u16::from(byte)
        })
    })
}

pub(super) fn retained_error(
    claim: claims::Lease,
    location: &File,
    error: PlatformError,
) -> PlatformError {
    let location = windows_fs::location(location)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "native Copy setup".into());
    let error = claim.abort_unstarted(error);
    PlatformError::CleanupPending {
        transaction: location,
        message: format!("{error}; unstarted Copy setup retained for verification"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Point {
    LocalCreated,
    ControlsCreated,
    DirectoriesFlushed,
    HeadersFlushed,
    Published,
}
#[cfg(not(test))]
pub(super) fn boundary(
    _: Point,
    _: &namespace::PrivateStage<'_>,
    _: &File,
) -> Result<(), PlatformError> {
    Ok(())
}
#[cfg(test)]
type Handoff = Box<dyn FnOnce(&File, &File, &OsStr)>;
#[cfg(test)]
thread_local! {
    pub(super) static FAULT: std::cell::Cell<Option<Point>> = const { std::cell::Cell::new(None) };
    pub(super) static HANDOFF: std::cell::RefCell<Option<(Point, Handoff)>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(super) fn boundary(
    point: Point,
    stage: &namespace::PrivateStage<'_>,
    central: &File,
) -> Result<(), PlatformError> {
    HANDOFF.with(|h| {
        let mut h = h.borrow_mut();
        if h.as_ref().is_some_and(|(at, _)| *at == point) {
            let (_, handoff) = h.take().unwrap();
            handoff(&stage.directory, central, &stage.name);
        }
    });
    if FAULT.with(|f| f.get()) == Some(point) {
        return Err(io("injected pre-header Copy setup failure"));
    }
    Ok(())
}

/// A held paired Begin proves no payload command was issued. This owns readers
/// only; it cannot construct a writer or authorize deletion of the retained setup.
pub(super) struct UnstartedSetup {
    snapshot: Snapshot,
    central_name: OsString,
}
impl UnstartedSetup {
    pub(super) fn revalidate(&self, parent: &File, central: &File) -> Result<(), PlatformError> {
        self.snapshot
            .revalidate_pair(parent, central, &self.central_name)
    }
}

/// Called only while admission holds the exclusive, validated name-claim writer.
/// Prefer the canonical record; a collision/corruption must never fall back to a
/// more convenient unpublished copy. Partial or unknown setups remain reserved.
pub(super) fn unstarted_setup(
    parent: &File,
    central: &File,
    transaction: &str,
    binding: &claims::Binding,
) -> Result<Option<UnstartedSetup>, PlatformError> {
    let canonical = OsString::from(format!("{transaction}.copy-journal"));
    let central_name = match unsafe {
        windows_fs::open_relative(
            central,
            &canonical,
            RelativeOptions {
                directory: None,
                create_new: false,
                access: windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES
                    | windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE,
                sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                security: ptr::null(),
            },
        )
    } {
        Ok(_) => canonical,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => preparation_name(transaction),
        Err(error) => return Err(io(error)),
    };
    let snapshot = Snapshot::read_pair(
        parent,
        central,
        OsStr::new(&format!(".ottrin-copy-{transaction}")),
        SnapshotAccess::Read,
        None,
        Some(&central_name),
    )?;
    if &snapshot.state.header.as_ref().unwrap().claim != binding {
        return Err(io(
            "Copy setup does not match its destination claim; reservation retained",
        ));
    }
    if snapshot.sequence != 1 {
        return Ok(None);
    }
    Ok(Some(UnstartedSetup {
        snapshot,
        central_name,
    }))
}
