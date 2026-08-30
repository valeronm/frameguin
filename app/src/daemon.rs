//! The app's end of the daemon: the connection to it and the controls it
//! detected, dialled and asked once for its run and shared by every window
//! and the tray.

use std::rc::Rc;

use async_lock::OnceCell;
use frameguin_model::control::Controls;
use frameguin_wire::DeviceResult;

use crate::bus::Bus;

#[derive(Default)]
pub(crate) struct Daemon {
    bus: OnceCell<Rc<Bus>>,
    /// Empty until asked; a failed ask is not remembered.
    controls: OnceCell<Rc<Controls<Bus>>>,
}

impl Daemon {
    /// The one connection, dialled on first use: a session that only ever
    /// shows the tray opens none.
    pub(crate) async fn bus(&self) -> zbus::Result<Rc<Bus>> {
        // The tray and the window both ask at startup.
        self.bus
            .get_or_try_init(async || Ok(Rc::new(Bus::connect().await?)))
            .await
            .cloned()
    }

    /// The controls whose devices detected themselves, shared by every window
    /// so a control is one object however many views reach it. A failure is
    /// not remembered — detection is the cold call, and caching one unlucky
    /// answer would hold the app to it for the session.
    pub(crate) async fn controls(&self) -> DeviceResult<Rc<Controls<Bus>>> {
        self.controls
            .get_or_try_init(async || {
                let bus = self.bus().await?;
                Ok(Rc::new(Controls::detect(&bus).await?))
            })
            .await
            .cloned()
    }
}
