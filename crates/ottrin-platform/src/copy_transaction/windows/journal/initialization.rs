//! Roll back only this live owner's pre-header stage. Incomplete durable journal
//! setup remains retained; this grants no restart or unknown-stage authority.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Point {
    LocalCreated,
    ControlsCreated,
    DirectoriesFlushed,
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
    pub(super) static HANDOFF: std::cell::RefCell<Option<Handoff>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(super) fn boundary(
    point: Point,
    stage: &namespace::PrivateStage<'_>,
    central: &File,
) -> Result<(), PlatformError> {
    if point == Point::LocalCreated
        && let Some(handoff) = HANDOFF.with(|h| h.borrow_mut().take())
    {
        handoff(&stage.directory, central, &stage.name);
    }
    if FAULT.with(|f| f.get()) == Some(point) {
        return Err(io("injected pre-header Copy setup failure"));
    }
    Ok(())
}

pub(super) fn discard_empty(
    stage: namespace::PrivateStage<'_>,
    central: &File,
) -> Result<(), PlatformError> {
    stage.validate()?;
    let creator = security::current_owner()?;
    let security = security::PrivateSecurity::capture(&stage.directory, &creator)?;
    cleanup_finalize::metadata(
        &stage.directory,
        stage.identities().1,
        &security,
        &creator,
        None,
        &mut |_| Ok(()),
    )?;
    windows_fs::directory_children_limited(&stage.directory, 0).map_err(io)?;
    let parent = stage.parent().try_clone().map_err(io)?;
    stage.validate()?;
    cleanup_tree::dispose(&stage.directory)?;
    drop(stage);
    parent.sync_all().map_err(io)?;
    central.sync_all().map_err(io)
}
