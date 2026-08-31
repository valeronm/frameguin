use std::cell::Cell;
use std::task::{Context, Poll, Waker};

use frameguin_wire::{
    BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, BatteryState, ChargeFlow,
    ClickForce, DeviceError, DeviceResult, NO_CHARGE_CURRENT_LIMIT, PowerLedControl, PowerLedLevel,
    TouchpadControl, TouchscreenControl,
};

/// A 4640 mAh pack, the Laptop 13's.
pub(crate) const CAPACITY: u32 = 4640;

/// Mid-charge on the same pack's four cells.
pub(crate) const MILLIVOLTS: u32 = 15_400;

/// What that pack is rated at, which is what its energy is measured against
/// however charged it happens to be.
pub(crate) const NOMINAL_MILLIVOLTS: u32 = 15_640;

pub(crate) fn state(flow: ChargeFlow, milliamps: u32) -> BatteryState {
    BatteryState {
        percent: 62,
        flow,
        milliamps,
        millivolts: MILLIVOLTS,
    }
}

/// The pack's block, part way through a charge.
pub(crate) fn block() -> BatteryInfo {
    BatteryInfo {
        state: state(ChargeFlow::Charging, 2320),
        remaining_capacity: 2843,
        last_full_capacity: 4176,
        design_capacity: CAPACITY,
        design_millivolts: NOMINAL_MILLIVOLTS,
        cycle_count: 40,
        charger_connected: true,
        critical: false,
        manufacturer: "NVT".into(),
        model: "FRANGWA".into(),
        serial: String::new(),
        chemistry: "LION".into(),
        manufactured: String::new(),
    }
}

/// What a stub does besides answer: refuses every write once told to, and
/// answers every read with `failing` where one is set.
#[derive(Default)]
pub(crate) struct Fault {
    refusing: Cell<bool>,
    failing: Option<DeviceError>,
}

impl Fault {
    pub(crate) fn failing(error: DeviceError) -> Self {
        Self {
            failing: Some(error),
            ..Self::default()
        }
    }

    pub(crate) fn refuse(&self) {
        self.refusing.set(true);
    }

    pub(crate) fn write(&self) -> DeviceResult<()> {
        if self.refusing.get() {
            Err(DeviceError::AccessDenied("not authorized".into()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn read<T>(&self, value: T) -> DeviceResult<T> {
        self.failing.clone().map_or(Ok(value), Err)
    }
}

/// What a device answers where the hardware does not serve it at all.
pub(crate) fn absent() -> DeviceError {
    DeviceError::Absent("no such interface".into())
}

/// A board of four columns, each answering as its own fault says — the one
/// stub implementing all four traits, which is what detecting the set at
/// once asks for.
#[derive(Default)]
pub(crate) struct Board {
    pub(crate) battery: Fault,
    pub(crate) touchpad: Fault,
    pub(crate) touchscreen: Fault,
    pub(crate) power_led: Fault,
}

impl Board {
    pub(crate) fn bare() -> Self {
        Self::failing(absent())
    }

    pub(crate) fn failing(error: DeviceError) -> Self {
        Self {
            battery: Fault::failing(error.clone()),
            touchpad: Fault::failing(error.clone()),
            touchscreen: Fault::failing(error.clone()),
            power_led: Fault::failing(error),
        }
    }
}

impl BatteryControl for Board {
    async fn info(&self) -> DeviceResult<BatteryInfo> {
        self.battery.read(block())
    }

    async fn condition(&self) -> DeviceResult<BatteryCondition> {
        self.battery.read(BatteryCondition {
            cell_millivolts: vec![3_850; 4],
            alarms: Vec::new(),
            decicelsius: 300,
        })
    }

    async fn features(&self) -> DeviceResult<Vec<BatteryFeature>> {
        self.battery.read(vec![BatteryFeature::ChargeLimit])
    }

    async fn charge_limit(&self) -> DeviceResult<u8> {
        self.battery.read(100)
    }

    async fn set_charge_limit(&self, _percent: u8) -> DeviceResult<bool> {
        self.battery.write().map(|()| true)
    }

    async fn charge_current_limit(&self) -> DeviceResult<u32> {
        self.battery.read(NO_CHARGE_CURRENT_LIMIT)
    }

    async fn set_charge_current_limit(&self, _milliamps: u32) -> DeviceResult<bool> {
        self.battery.write().map(|()| true)
    }
}

impl TouchpadControl for Board {
    async fn haptic_intensity(&self) -> DeviceResult<u8> {
        self.touchpad.read(50)
    }

    async fn set_haptic_intensity(&self, _percent: u8) -> DeviceResult<()> {
        self.touchpad.write()
    }

    async fn click_force(&self) -> DeviceResult<ClickForce> {
        self.touchpad.read(ClickForce::Medium)
    }

    async fn set_click_force(&self, _force: ClickForce) -> DeviceResult<()> {
        self.touchpad.write()
    }
}

impl TouchscreenControl for Board {
    async fn enabled(&self) -> DeviceResult<bool> {
        self.touchscreen.read(true)
    }

    async fn set_enabled(&self, _enabled: bool) -> DeviceResult<()> {
        self.touchscreen.write()
    }
}

impl PowerLedControl for Board {
    async fn brightness(&self) -> DeviceResult<(u8, PowerLedLevel)> {
        self.power_led.read((55, PowerLedLevel::High))
    }

    async fn levels(&self) -> DeviceResult<Vec<PowerLedLevel>> {
        self.power_led.read(PowerLedLevel::ALL.to_vec())
    }

    async fn set_level(&self, _level: PowerLedLevel) -> DeviceResult<()> {
        self.power_led.write()
    }

    async fn set_brightness(&self, _percent: u8) -> DeviceResult<()> {
        self.power_led.write()
    }
}

/// Polls once: a stub answers on the spot, so a future here never pends.
pub(crate) fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("a stub never pends"),
    }
}
