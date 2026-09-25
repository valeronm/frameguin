//! One D-Bus interface per device's control, each a thin adapter on
//! [`crate::served::Served`] over `frameguin_hardware`'s implementation of
//! that control trait, and the one function that puts them and the root
//! interface at the path.
//!
//! What an adapter adds is the idle clock and the polkit check every setter
//! makes before anything else.

pub(crate) mod battery;
pub(crate) mod charging_led;
pub(crate) mod chassis;
pub(crate) mod ports;
pub(crate) mod power_led;
pub(crate) mod privacy_switches;
pub(crate) mod touchpad;
pub(crate) mod touchscreen;
pub(crate) mod usb;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use frameguin_contract::DeviceResult;
use frameguin_hardware::device::Devices;
use frameguin_hardware::device::battery::Battery;
use frameguin_hardware::device::power_led::PowerLed;
use frameguin_hardware::device::touchpad::Touchpad;
use frameguin_hardware::device::touchscreen::Touchscreen;
use frameguin_hardware::restore::Restorable;
use frameguin_wire::OBJECT_PATH;
use zbus::object_server::{Interface, ObjectServer};

use crate::Daemon;
use crate::served::Served;
use crate::service::Service;

/// Everything the daemon puts at [`OBJECT_PATH`]: the root interface, then
/// an interface per device detected — a device detection did not find is
/// not on the bus, so the interfaces at the path are the inventory. Spelled
/// once rather than at each caller, so the harness cannot serve a shorter
/// list than the daemon does.
pub(crate) async fn serve_all(
    server: &ObjectServer,
    root: Daemon,
    devices: Devices,
) -> zbus::Result<()> {
    let service = root.service.clone();
    let Devices {
        battery,
        touchpad,
        touchscreen,
        power_led,
        charging_led,
        ports,
        chassis,
        privacy_switches,
        usb,
    } = devices;
    server.at(OBJECT_PATH, root).await?;
    serve_one(server, &service, battery).await?;
    serve_one(server, &service, touchpad).await?;
    serve_one(server, &service, touchscreen).await?;
    serve_one(server, &service, power_led).await?;
    serve_one(server, &service, charging_led).await?;
    serve_one(server, &service, ports).await?;
    serve_one(server, &service, chassis).await?;
    serve_one(server, &service, privacy_switches).await?;
    serve_one(server, &service, usb).await
}

#[derive(Clone, Copy)]
pub(crate) enum Op {
    Remember,
    Restore,
}

impl Op {
    fn done(self) -> &'static str {
        match self {
            Self::Remember => "remembered",
            Self::Restore => "restored",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Self::Remember => "remember",
            Self::Restore => "restore",
        }
    }
}

/// Through the object server rather than a list of its own: the interfaces
/// at the path are the inventory, so a device not on the bus has nothing to
/// write back.
pub(crate) async fn each_restorable(server: &ObjectServer, op: Op) -> DeviceResult<()> {
    let battery = one_restorable::<Battery>(server, op).await;
    let power_led = one_restorable::<PowerLed>(server, op).await;
    let touchscreen = one_restorable::<Touchscreen>(server, op).await;
    battery.and(power_led).and(touchscreen)
}

/// A touchpad holding no mirror is left alone, and nothing is logged for it.
pub(crate) async fn resend_mirrors(server: &ObjectServer) -> DeviceResult<()> {
    let Ok(touchpad) = server.interface::<_, Served<Touchpad>>(OBJECT_PATH).await else {
        return Ok(());
    };
    let touchpad = touchpad.get().await;
    let device = touchpad.device();
    if !device.mirrored() {
        return Ok(());
    }
    logged::<Touchpad>("resent", "resend", device.resend())
}

async fn one_restorable<D: Restorable>(server: &ObjectServer, op: Op) -> DeviceResult<()>
where
    Served<D>: Interface,
{
    let Ok(interface) = server.interface::<_, Served<D>>(OBJECT_PATH).await else {
        return Ok(());
    };
    let guard = interface.get().await;
    let device = guard.device();
    let outcome = match op {
        Op::Remember => device.remember().await,
        Op::Restore => device.restore().await,
    };
    logged::<D>(op.done(), op.verb(), outcome)
}

fn logged<D>(done: &str, verb: &str, outcome: DeviceResult<()>) -> DeviceResult<()>
where
    Served<D>: Interface,
{
    let name = <Served<D> as Interface>::name();
    outcome
        .inspect(|()| eprintln!("{done} {name}"))
        .inspect_err(|e| eprintln!("could not {verb} {name}: {e}"))
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
