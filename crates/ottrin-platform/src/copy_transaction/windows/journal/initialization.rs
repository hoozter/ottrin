//! Construction boundaries for persistent, unpublished journal preparations.
use super::*;

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
