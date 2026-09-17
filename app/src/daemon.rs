//! The app's end of the daemon: the connection to it, the controls it
//! detected and the board it runs on, dialled and asked once for its run and
//! shared by every window and the tray.

use std::rc::Rc;

use async_lock::OnceCell;
use frameguin_model::control::Controls;
use frameguin_wire::{Board, DeviceResult};

use crate::bus::Bus;

#[derive(Clone)]
pub(crate) struct Detected {
    pub(crate) controls: Rc<Controls<Bus>>,
    pub(crate) board: Rc<Board>,
}

#[derive(Default)]
pub(crate) struct Daemon {
    bus: OnceCell<Rc<Bus>>,
    detected: OnceCell<Detected>,
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
    /// so a control is one object however many views reach it.
    pub(crate) async fn controls(&self) -> DeviceResult<Rc<Controls<Bus>>> {
        Ok(self.detected().await?.controls)
    }

    /// The controls and the board they were detected for. A failure is not
    /// remembered — detection is the cold call, and caching one unlucky
    /// answer would hold the app to it for the session.
    pub(crate) async fn detected(&self) -> DeviceResult<Detected> {
        self.detected
            .get_or_try_init(async || {
                let bus = self.bus().await?;
                let board = Rc::new(bus.frameguin.get_board().await?);
                let controls = Rc::new(Controls::detect(&bus, &board).await?);
                Ok(Detected { controls, board })
            })
            .await
            .cloned()
    }
}
