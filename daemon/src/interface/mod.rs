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
use zbus::object_server::ObjectServer;

use crate::served::Served;
use crate::service::Service;

/// Every device with an interface, None where detection found none. One
/// struct for the daemon and its tests both, so a device served by one and
/// not the other is a missing field rather than a missing line.
pub(crate) struct Devices {
    pub(crate) touchpad: Option<Touchpad>,
    pub(crate) touchscreen: Option<Touchscreen>,
    pub(crate) power_led: Option<PowerLed>,
    pub(crate) battery: Option<Battery>,
}

impl Devices {
    /// A device not detected is not on the bus: the interfaces present at
    /// the path are the inventory.
    pub(crate) async fn serve(
        self,
        server: &ObjectServer,
        service: &Arc<Service>,
    ) -> zbus::Result<()> {
        if let Some(touchpad) = self.touchpad {
            server
                .at(OBJECT_PATH, Served::new(touchpad, service.clone()))
                .await?;
        }
        if let Some(touchscreen) = self.touchscreen {
            server
                .at(OBJECT_PATH, Served::new(touchscreen, service.clone()))
                .await?;
        }
        if let Some(power_led) = self.power_led {
            server
                .at(OBJECT_PATH, Served::new(power_led, service.clone()))
                .await?;
        }
        if let Some(battery) = self.battery {
            server
                .at(OBJECT_PATH, Served::new(battery, service.clone()))
                .await?;
        }
        Ok(())
    }
}
