//! The app's end of the daemon: the connection to it, the controls it
//! detected and the board it runs on, dialled and asked once for its run and
//! shared by every window and the tray.

use std::rc::Rc;

use async_lock::OnceCell;
use frameguin_contract::DeviceResult;
use frameguin_model::control::Controls;
use frameguin_wire::{Bus, from_bus_error};

#[derive(Default)]
pub(crate) struct Daemon {
    bus: OnceCell<Rc<Bus>>,
    controls: OnceCell<Rc<Controls<Bus>>>,
}

impl Daemon {
    /// The one connection, dialled on first use: a session that only ever
    /// shows the tray opens none.
    pub(crate) async fn bus(&self) -> DeviceResult<Rc<Bus>> {
        // The tray and the window both ask at startup.
        self.bus
            .get_or_try_init(async || Ok(Rc::new(Bus::connect().await.map_err(from_bus_error)?)))
            .await
            .cloned()
    }

    /// The controls whose devices detected themselves and the board they
    /// were detected on, shared by every window so a control is one object
    /// however many views reach it. A failure is not remembered — detection
    /// is the cold call, and caching one unlucky answer would hold the app to
    /// it for the session.
    pub(crate) async fn controls(&self) -> DeviceResult<Rc<Controls<Bus>>> {
        self.controls
            .get_or_try_init(async || Ok(Rc::new(Controls::detect(&self.bus().await?).await?)))
            .await
            .cloned()
    }
}
