//! One D-Bus interface per device's control, each a thin adapter on
//! [`crate::served::Served`] over `frameguin_hardware`'s implementation of
//! that control trait.
//!
//! What an adapter adds is the bus's business alone — the idle clock, and
//! the order validate → skip → authorize → write, with the polkit prompt in
//! the place that order puts it. The operation itself, its argument check
//! included, is the device's, so a caller reaching the hardware crate
//! directly gets the same refusals without this layer.

pub(crate) mod battery;
pub(crate) mod power_led;
pub(crate) mod touchpad;
pub(crate) mod touchscreen;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use frameguin_hardware::device::battery::Battery;
use frameguin_hardware::device::power_led::PowerLed;
use frameguin_hardware::device::touchpad::Touchpad;
use frameguin_hardware::device::touchscreen::Touchscreen;
use frameguin_wire::OBJECT_PATH;
use zbus::object_server::{Interface, ObjectServer};

use crate::served::Served;
use crate::service::Service;

/// Every device with an interface, None where detection found none. One
/// struct for the daemon and its tests both, so a device served by one and
/// not the other is a missing field rather than a missing line.
pub(crate) struct Devices {
    pub(crate) battery: Option<Battery>,
    pub(crate) touchpad: Option<Touchpad>,
    pub(crate) touchscreen: Option<Touchscreen>,
    pub(crate) power_led: Option<PowerLed>,
}

impl Devices {
    /// A device not detected is not on the bus: the interfaces present at
    /// the path are the inventory.
    pub(crate) async fn serve(
        self,
        server: &ObjectServer,
        service: &Arc<Service>,
    ) -> zbus::Result<()> {
        let Self {
            battery,
            touchpad,
            touchscreen,
            power_led,
        } = self;
        serve_one(server, service, battery).await?;
        serve_one(server, service, touchpad).await?;
        serve_one(server, service, touchscreen).await?;
        serve_one(server, service, power_led).await
    }
}

async fn serve_one<D>(
    server: &ObjectServer,
    service: &Arc<Service>,
    device: Option<D>,
) -> zbus::Result<()>
where
    Served<D>: Interface,
{
    if let Some(device) = device {
        server
            .at(OBJECT_PATH, Served::new(device, service.clone()))
            .await?;
    }
    Ok(())
}
