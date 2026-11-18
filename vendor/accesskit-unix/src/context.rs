// Copyright 2023 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use accesskit::{ActivationHandler, DeactivationHandler};
use accesskit_atspi_common::{Adapter as AdapterImpl, AppContext, Event};
#[cfg(not(feature = "tokio"))]
use async_channel::{Receiver, Sender};
use atspi::proxy::bus::StatusProxy;
#[cfg(not(feature = "tokio"))]
use futures_util::{pin_mut as pin, select, StreamExt};
use std::{
    sync::{Arc, Mutex, OnceLock, RwLock},
    thread,
};
#[cfg(feature = "tokio")]
use tokio::{
    pin, select,
    sync::mpsc::{UnboundedReceiver as Receiver, UnboundedSender as Sender},
};
#[cfg(feature = "tokio")]
use tokio_stream::{wrappers::UnboundedReceiverStream, StreamExt};
use zbus::{connection::Builder, Connection};

use crate::{
    adapter::{AdapterState, Callback, Message},
    atspi::Bus,
    executor::Executor,
    util::block_on,
};

static APP_CONTEXT: OnceLock<Arc<RwLock<AppContext>>> = OnceLock::new();
static MESSAGES: OnceLock<Sender<Message>> = OnceLock::new();

fn app_name() -> Option<String> {
    std::env::current_exe().ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
    })
}

pub(crate) fn get_or_init_app_context<'a>() -> &'a Arc<RwLock<AppContext>> {
    APP_CONTEXT.get_or_init(|| AppContext::new(app_name()))
}

pub(crate) fn get_or_init_messages() -> Sender<Message> {
    MESSAGES
        .get_or_init(|| {
            #[cfg(not(feature = "tokio"))]
            let (tx, rx) = async_channel::unbounded();
            #[cfg(feature = "tokio")]
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

            thread::spawn(|| {
                let executor = Executor::new();
                block_on(executor.run(async {
                    if let Ok(session_bus) = Builder::session() {
                        match session_bus.internal_executor(false).build().await {
                            Ok(session_bus) => {
                                run_event_loop(&executor, session_bus, rx).await;
                            }
                            Err(error) => eprintln!(
                                "accesskit_unix: failed to connect to the D-Bus session bus \
                                 ({error}); accessibility is unavailable for this process"
                            ),
                        }
                    } else {
                        eprintln!(
                            "accesskit_unix: no D-Bus session bus address is available; \
                             accessibility is unavailable for this process"
                        );
                    }
                }))
            });

            tx
        })
        .clone()
}

struct AdapterEntry {
    id: usize,
    activation_handler: Box<dyn ActivationHandler>,
    deactivation_handler: Box<dyn DeactivationHandler>,
    state: Arc<Mutex<AdapterState>>,
}

fn activate_adapter(entry: &mut AdapterEntry) {
    let mut state = entry.state.lock().unwrap();
    if let AdapterState::Inactive {
        is_window_focused,
        root_window_bounds,
        action_handler,
    } = &*state
    {
        *state = match entry.activation_handler.request_initial_tree() {
            Some(initial_state) => {
                let r#impl = AdapterImpl::with_wrapped_action_handler(
                    entry.id,
                    get_or_init_app_context(),
                    Callback::new(),
                    initial_state,
                    *is_window_focused,
                    *root_window_bounds,
                    Arc::clone(action_handler),
                );
                AdapterState::Active(r#impl)
            }
            None => AdapterState::Pending {
                is_window_focused: *is_window_focused,
                root_window_bounds: *root_window_bounds,
                action_handler: Arc::clone(action_handler),
            },
        };
    }
}

fn deactivate_adapter(entry: &mut AdapterEntry) {
    let mut state = entry.state.lock().unwrap();
    match &*state {
        AdapterState::Inactive { .. } => (),
        AdapterState::Pending {
            is_window_focused,
            root_window_bounds,
            action_handler,
        } => {
            *state = AdapterState::Inactive {
                is_window_focused: *is_window_focused,
                root_window_bounds: *root_window_bounds,
                action_handler: Arc::clone(action_handler),
            };
            drop(state);
            entry.deactivation_handler.deactivate_accessibility();
        }
        AdapterState::Active(r#impl) => {
            *state = AdapterState::Inactive {
                is_window_focused: r#impl.is_window_focused(),
                root_window_bounds: r#impl.root_window_bounds(),
                action_handler: r#impl.wrapped_action_handler(),
            };
            drop(state);
            entry.deactivation_handler.deactivate_accessibility();
        }
    }
}

/// Runs the adapter event loop for the lifetime of the process. Connection
/// level D-Bus failures (a missing or stale session or AT-SPI bus socket, a
/// disconnected bus) are reported on stderr and disable the affected bus
/// instead of propagating: the loop stays alive, so a later `IsEnabled` change
/// re-establishes accessibility through the normal path, and the message
/// receiver is never dropped while the loop is running.
async fn run_event_loop(executor: &Executor<'_>, session_bus: Connection, rx: Receiver<Message>) {
    let session_bus_copy = session_bus.clone();
    let _session_bus_task = executor.spawn(
        async move {
            loop {
                session_bus_copy.executor().tick().await;
            }
        },
        "accesskit_session_bus_task",
    );

    let status = match StatusProxy::new(&session_bus).await {
        Ok(status) => status,
        Err(error) => {
            eprintln!(
                "accesskit_unix: failed to create the AT-SPI status proxy ({error}); \
                 accessibility is unavailable for this process"
            );
            return;
        }
    };
    let changes = status.receive_is_enabled_changed().await.fuse();
    pin!(changes);

    #[cfg(not(feature = "tokio"))]
    let messages = rx.fuse();
    #[cfg(feature = "tokio")]
    let messages = UnboundedReceiverStream::new(rx).fuse();
    pin!(messages);

    let mut atspi_bus = None;
    let mut adapters: Vec<AdapterEntry> = Vec::new();

    loop {
        select! {
            change = changes.next() => {
                atspi_bus = None;
                let Some(change) = change else {
                    eprintln!(
                        "accesskit_unix: the AT-SPI status connection closed; \
                         accessibility is unavailable for this process"
                    );
                    for entry in &mut adapters {
                        deactivate_adapter(entry);
                    }
                    return;
                };
                let mut enabled = false;
                match change.get().await {
                    Ok(is_enabled) => enabled = is_enabled,
                    Err(error) => eprintln!(
                        "accesskit_unix: failed to read the AT-SPI IsEnabled property \
                         ({error}); treating accessibility as disabled until its state \
                         next changes"
                    ),
                }
                if enabled {
                    atspi_bus = match Bus::new(&session_bus, executor).await {
                        Ok(bus) => Some(bus),
                        Err(error) => {
                            eprintln!(
                                "accesskit_unix: failed to connect to the AT-SPI bus ({error}); \
                                 accessibility will be retried when its state next changes"
                            );
                            None
                        }
                    };
                }
                for entry in &mut adapters {
                    if atspi_bus.is_some() {
                        activate_adapter(entry);
                    } else {
                        deactivate_adapter(entry);
                    }
                }
            }
            message = messages.next() => {
                if let Some(message) = message {
                    if let Err(error) =
                        process_adapter_message(&atspi_bus, &mut adapters, message).await
                    {
                        eprintln!(
                            "accesskit_unix: failed to process an accessibility message \
                             ({error}); disabling the AT-SPI bus until its state next changes"
                        );
                        atspi_bus = None;
                        for entry in &mut adapters {
                            deactivate_adapter(entry);
                        }
                    }
                } else {
                    return;
                }
            }
        }
    }
}

async fn process_adapter_message(
    atspi_bus: &Option<Bus>,
    adapters: &mut Vec<AdapterEntry>,
    message: Message,
) -> zbus::Result<()> {
    match message {
        Message::AddAdapter {
            id,
            activation_handler,
            deactivation_handler,
            state,
        } => {
            adapters.push(AdapterEntry {
                id,
                activation_handler,
                deactivation_handler,
                state,
            });
            if atspi_bus.is_some() {
                let entry = adapters.last_mut().unwrap();
                activate_adapter(entry);
            }
        }
        Message::RemoveAdapter { id } => {
            if let Ok(index) = adapters.binary_search_by(|entry| entry.id.cmp(&id)) {
                adapters.remove(index);
            }
        }
        Message::RegisterInterfaces { node, interfaces } => {
            if let Some(bus) = atspi_bus {
                bus.register_interfaces(node, interfaces).await?
            }
        }
        Message::UnregisterInterfaces {
            adapter_id,
            node_id,
            interfaces,
        } => {
            if let Some(bus) = atspi_bus {
                bus.unregister_interfaces(adapter_id, node_id, interfaces)
                    .await?
            }
        }
        Message::EmitEvent {
            adapter_id,
            event: Event::Object { target, event },
        } => {
            if let Some(bus) = atspi_bus {
                bus.emit_object_event(adapter_id, target, event).await?
            }
        }
        Message::EmitEvent {
            adapter_id,
            event:
                Event::Window {
                    target,
                    name,
                    event,
                },
        } => {
            if let Some(bus) = atspi_bus {
                bus.emit_window_event(adapter_id, target, name, event)
                    .await?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::process_adapter_message;
    use crate::{adapter::Message, util::block_on};
    use accesskit_atspi_common::{Event, NodeIdOrRoot, ObjectEvent};

    /// With the AT-SPI bus disabled after a connection failure, message
    /// processing must remain a no-op that keeps the receiver alive instead of
    /// returning an error that would abort the worker.
    #[test]
    fn disabled_bus_drains_messages_without_error() {
        let event = Message::EmitEvent {
            adapter_id: 0,
            event: Event::Object {
                target: NodeIdOrRoot::Root,
                event: ObjectEvent::SelectionChanged,
            },
        };
        let remove = Message::RemoveAdapter { id: 0 };
        block_on(async {
            process_adapter_message(&None, &mut Vec::new(), event)
                .await
                .expect("a disabled AT-SPI bus must not fail message processing");
            process_adapter_message(&None, &mut Vec::new(), remove)
                .await
                .expect("a disabled AT-SPI bus must not fail adapter removal");
        });
    }
}
