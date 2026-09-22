//! The bus implementation of every control trait in `frameguin_wire`: each
//! operation a call on the daemon's interface for that device.
//!
//! One connection carries every proxy, dialled once per app run — see
//! [`crate::daemon::Daemon`] for who holds it.

use frameguin_wire::{
    Attached, BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, ChargingLedControl,
    ChargingLedFeature, ChargingLedSide, ChassisControl, ChassisFeature, ChassisState, ClickForce,
    DeckState, DeviceResult, ExtenderState, FrameguinProxy, PortSet, PortState, PortsControl,
    PowerLedControl, PowerLedLevel, PrivacyState, PrivacySwitchesControl, Proxies, TouchpadControl,
    TouchscreenControl, UsbControl, proxy,
};

pub(crate) struct Bus {
    /// The root interface, for what belongs to no device: the board, the
    /// inventory, the daemon's build, and the restore switch.
    pub(crate) frameguin: FrameguinProxy<'static>,
    devices: Proxies,
}

impl Bus {
    /// Dials the system bus. Building a proxy is free of I/O, so a device the
    /// daemon did not register costs nothing until something calls it.
    pub(crate) async fn connect() -> zbus::Result<Self> {
        let conn = zbus::Connection::system().await?;
        Ok(Self {
            frameguin: proxy(&conn).await?,
            devices: Proxies::dial(&conn).await?,
        })
    }
}

impl BatteryControl for Bus {
    async fn info(&self) -> DeviceResult<BatteryInfo> {
        Ok(self.devices.battery.get_info().await?)
    }

    async fn condition(&self) -> DeviceResult<BatteryCondition> {
        Ok(self.devices.battery.get_condition().await?)
    }

    async fn features(&self) -> DeviceResult<Vec<BatteryFeature>> {
        Ok(self.devices.battery.get_features().await?)
    }

    async fn charge_limit(&self) -> DeviceResult<u8> {
        Ok(self.devices.battery.get_charge_limit().await?)
    }

    async fn set_charge_limit(&self, percent: u8) -> DeviceResult<bool> {
        Ok(self.devices.battery.set_charge_limit(percent).await?)
    }

    async fn charge_current_limit(&self) -> DeviceResult<u32> {
        Ok(self.devices.battery.get_charge_current_limit().await?)
    }

    async fn set_charge_current_limit(&self, milliamps: u32) -> DeviceResult<bool> {
        Ok(self
            .devices
            .battery
            .set_charge_current_limit(milliamps)
            .await?)
    }

    async fn extender(&self) -> DeviceResult<ExtenderState> {
        Ok(self.devices.battery.get_extender().await?)
    }
}

impl TouchpadControl for Bus {
    async fn haptic_intensity(&self) -> DeviceResult<u8> {
        Ok(self.devices.touchpad.get_haptic_intensity().await?)
    }

    async fn set_haptic_intensity(&self, percent: u8) -> DeviceResult<()> {
        Ok(self.devices.touchpad.set_haptic_intensity(percent).await?)
    }

    async fn click_force(&self) -> DeviceResult<ClickForce> {
        Ok(self.devices.touchpad.get_click_force().await?)
    }

    async fn set_click_force(&self, force: ClickForce) -> DeviceResult<()> {
        Ok(self.devices.touchpad.set_click_force(force).await?)
    }
}

impl TouchscreenControl for Bus {
    async fn enabled(&self) -> DeviceResult<bool> {
        Ok(self.devices.touchscreen.get_enabled().await?)
    }

    async fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        Ok(self.devices.touchscreen.set_enabled(enabled).await?)
    }
}

impl ChargingLedControl for Bus {
    async fn enabled(&self) -> DeviceResult<bool> {
        Ok(self.devices.charging_led.get_enabled().await?)
    }

    async fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        Ok(self.devices.charging_led.set_enabled(enabled).await?)
    }

    async fn features(&self) -> DeviceResult<Vec<ChargingLedFeature>> {
        Ok(self.devices.charging_led.get_features().await?)
    }

    async fn side(&self) -> DeviceResult<ChargingLedSide> {
        Ok(self.devices.charging_led.get_side().await?)
    }
}

impl PortsControl for Bus {
    async fn ports(&self, controller_ports: PortSet) -> DeviceResult<Vec<PortState>> {
        Ok(self.devices.ports.get_ports(controller_ports).await?)
    }
}

impl ChassisControl for Bus {
    async fn state(&self) -> DeviceResult<ChassisState> {
        Ok(self.devices.chassis.get_state().await?)
    }

    async fn features(&self) -> DeviceResult<Vec<ChassisFeature>> {
        Ok(self.devices.chassis.get_features().await?)
    }

    async fn deck_state(&self) -> DeviceResult<DeckState> {
        Ok(self.devices.chassis.get_deck_state().await?)
    }
}

impl PrivacySwitchesControl for Bus {
    async fn switches(&self) -> DeviceResult<PrivacyState> {
        Ok(self.devices.privacy_switches.get_switches().await?)
    }
}

impl UsbControl for Bus {
    async fn attached(&self) -> DeviceResult<Vec<Attached>> {
        Ok(self.devices.usb.get_attached().await?)
    }
}

impl PowerLedControl for Bus {
    async fn brightness(&self) -> DeviceResult<(u8, PowerLedLevel)> {
        Ok(self.devices.power_led.get_brightness().await?)
    }

    async fn levels(&self) -> DeviceResult<Vec<PowerLedLevel>> {
        Ok(self.devices.power_led.get_levels().await?)
    }

    async fn set_level(&self, level: PowerLedLevel) -> DeviceResult<()> {
        Ok(self.devices.power_led.set_level(level).await?)
    }

    async fn set_brightness(&self, percent: u8) -> DeviceResult<()> {
        Ok(self.devices.power_led.set_brightness(percent).await?)
    }
}
