//! Native desktop preference, separate from saved palettes and user configuration.
#[derive(Default)]
pub(crate) struct SystemAppearance {
    preferred: Option<egui::Theme>,
    #[cfg(target_os = "linux")]
    updates: Option<std::sync::mpsc::Receiver<Option<egui::Theme>>>,
    #[cfg(target_os = "linux")]
    stop: Option<std::sync::mpsc::Sender<()>>,
    #[cfg(target_os = "linux")]
    worker: Option<std::thread::JoinHandle<()>>,
}

impl SystemAppearance {
    pub(crate) fn preferred(&self) -> Option<egui::Theme> {
        self.preferred
    }

    pub(crate) fn poll(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(updates) = &self.updates {
            while let Ok(theme) = updates.try_recv() {
                self.preferred = theme;
            }
        }
    }

    pub(crate) fn start(ctx: egui::Context) -> Self {
        #[cfg(target_os = "linux")]
        {
            use std::{sync::mpsc, time::Duration};
            let (send, updates) = mpsc::channel();
            let (stop, stopped) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                let mut connection = None;
                let mut previous = None;
                let mut previous_error = None;
                loop {
                    let preference = read_portal_preference(&mut connection);
                    let theme = match preference {
                        Ok(theme) => {
                            previous_error = None;
                            theme
                        }
                        Err(error) => {
                            connection = None;
                            if previous_error.as_ref() != Some(&error) {
                                eprintln!("Desktop appearance preference unavailable: {error}");
                                previous_error = Some(error);
                            }
                            None
                        }
                    };
                    if previous != Some(theme) {
                        if send.send(theme).is_err() {
                            break;
                        }
                        previous = Some(theme);
                        ctx.request_repaint();
                    }
                    // The bus call is bounded to one second; waiting here is interruptible.
                    // Polling also recovers when the desktop portal restarts or becomes available.
                    match stopped.recv_timeout(Duration::from_secs(2)) {
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        _ => break,
                    }
                }
            });
            Self {
                updates: Some(updates),
                stop: Some(stop),
                worker: Some(worker),
                ..Default::default()
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = ctx;
            Self::default()
        }
    }
}

#[cfg(target_os = "linux")]
fn read_portal_preference(
    connection: &mut Option<zbus::blocking::Connection>,
) -> Result<Option<egui::Theme>, String> {
    if connection.is_none() {
        *connection = Some(
            zbus::blocking::connection::Builder::session()
                .and_then(|builder| {
                    builder
                        .method_timeout(std::time::Duration::from_secs(1))
                        .build()
                })
                .map_err(|error| error.to_string())?,
        );
    }
    let connection = connection
        .as_ref()
        .expect("desktop bus connection was initialized");
    let proxy = zbus::blocking::Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .map_err(|error| error.to_string())?;
    let value: zbus::zvariant::OwnedValue = proxy
        .call("Read", &("org.freedesktop.appearance", "color-scheme"))
        .map_err(|error| error.to_string())?;
    // Portals may return either a variant or a nested variant (the Settings Read contract).
    let value = if let Ok(inner) = value.downcast_ref::<zbus::zvariant::Value>() {
        inner.try_to_owned().map_err(|error| error.to_string())?
    } else {
        value
    };
    let preference = u32::try_from(value).map_err(|error| error.to_string())?;
    match preference {
        0 => Ok(None),
        1 => Ok(Some(egui::Theme::Dark)),
        2 => Ok(Some(egui::Theme::Light)),
        other => Err(format!("unsupported desktop color-scheme value {other}")),
    }
}

impl Drop for SystemAppearance {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
            if let Some(worker) = self.worker.take()
                && worker.join().is_err()
            {
                eprintln!("Desktop appearance observer exited unexpectedly");
            }
        }
    }
}
