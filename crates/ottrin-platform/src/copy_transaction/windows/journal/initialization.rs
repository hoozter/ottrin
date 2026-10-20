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
