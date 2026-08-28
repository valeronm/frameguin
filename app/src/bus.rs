//! The bus implementation of every control trait in `frameguin_wire`: each
//! operation a call on the daemon's interface for that device.
//!
//! One connection carries every proxy, dialled once per app run — see
//! [`crate::reading::Feed`] for who holds it.

use frameguin_wire::{
    BUS_NAME, ClickForce, DeviceResult, FrameguinProxy, OBJECT_PATH, PowerLedControl,
    PowerLedLevel, PowerLedProxy, TouchpadControl, TouchpadProxy, TouchscreenControl,
    TouchscreenProxy,
};
use zbus::proxy::ProxyImpl;

pub(crate) struct Bus {
    /// The daemon's own interface, for the operations no device owns yet.
    pub(crate) frameguin: FrameguinProxy<'static>,
    touchpad: TouchpadProxy<'static>,
    touchscreen: TouchscreenProxy<'static>,
    power_led: PowerLedProxy<'static>,
}

impl Bus {
    /// Dials the system bus. Building a proxy is free of I/O, so a device the
    /// daemon did not register costs nothing until something calls it.
    pub(crate) async fn connect() -> zbus::Result<Self> {
        let conn = zbus::Connection::system().await?;
        Ok(Self {
            frameguin: proxy(&conn).await?,
            touchpad: proxy(&conn).await?,
            touchscreen: proxy(&conn).await?,
            power_led: proxy(&conn).await?,
        })
    }
}

/// Every proxy on the daemon's one name and path.
async fn proxy<P: ProxyImpl<'static> + From<zbus::Proxy<'static>>>(
    conn: &zbus::Connection,
) -> zbus::Result<P> {
    P::builder(conn)
        .destination(BUS_NAME)?
        .path(OBJECT_PATH)?
        .build()
        .await
}

impl TouchpadControl for Bus {
    async fn haptic_intensity(&self) -> DeviceResult<u8> {
        Ok(self.touchpad.get_haptic_intensity().await?)
    }

    async fn set_haptic_intensity(&self, percent: u8) -> DeviceResult<()> {
        Ok(self.touchpad.set_haptic_intensity(percent).await?)
    }

    async fn click_force(&self) -> DeviceResult<ClickForce> {
        Ok(self.touchpad.get_click_force().await?)
    }

    async fn set_click_force(&self, force: ClickForce) -> DeviceResult<()> {
        Ok(self.touchpad.set_click_force(force).await?)
    }
}

impl TouchscreenControl for Bus {
    async fn enabled(&self) -> DeviceResult<bool> {
        Ok(self.touchscreen.get_enabled().await?)
    }

    async fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        Ok(self.touchscreen.set_enabled(enabled).await?)
    }
}

impl PowerLedControl for Bus {
    async fn brightness(&self) -> DeviceResult<(u8, PowerLedLevel)> {
        Ok(self.power_led.get_brightness().await?)
    }

    async fn levels(&self) -> DeviceResult<Vec<PowerLedLevel>> {
        Ok(self.power_led.get_levels().await?)
    }

    async fn set_level(&self, level: PowerLedLevel) -> DeviceResult<()> {
        Ok(self.power_led.set_level(level).await?)
    }

    async fn set_brightness(&self, percent: u8) -> DeviceResult<()> {
        Ok(self.power_led.set_brightness(percent).await?)
    }
}
