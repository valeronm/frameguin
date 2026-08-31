use std::cell::Cell;
use std::rc::Rc;
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

/// A board of four columns, each answering as its own fault says and
/// remembering what it was written. The one stub implementing all four
/// traits: detecting the set at once asks for that, and a control asks only
/// its own column.
pub(crate) struct Board {
    pub(crate) battery: Fault,
    pub(crate) touchpad: Fault,
    pub(crate) touchscreen: Fault,
    pub(crate) power_led: Fault,
    pub(crate) limit: Cell<u8>,
    pub(crate) cap: Cell<u32>,
    pub(crate) haptic_intensity: Cell<u8>,
    pub(crate) click_force: Cell<ClickForce>,
    pub(crate) enabled: Cell<bool>,
    pub(crate) percent: Cell<u8>,
    pub(crate) level: Cell<PowerLedLevel>,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            battery: Fault::default(),
            touchpad: Fault::default(),
            touchscreen: Fault::default(),
            power_led: Fault::default(),
            limit: Cell::new(100),
            cap: Cell::new(NO_CHARGE_CURRENT_LIMIT),
            haptic_intensity: Cell::new(50),
            click_force: Cell::new(ClickForce::Low),
            enabled: Cell::new(true),
            percent: Cell::new(55),
            level: Cell::new(PowerLedLevel::High),
        }
    }
}

impl Board {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub(crate) fn failing(error: DeviceError) -> Rc<Self> {
        Rc::new(Self {
            battery: Fault::failing(error.clone()),
            touchpad: Fault::failing(error.clone()),
            touchscreen: Fault::failing(error.clone()),
            power_led: Fault::failing(error),
            ..Self::default()
        })
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
        self.battery.read(self.limit.get())
    }

    async fn set_charge_limit(&self, percent: u8) -> DeviceResult<bool> {
        self.battery.write()?;
        self.limit.set(percent);
        Ok(true)
    }

    async fn charge_current_limit(&self) -> DeviceResult<u32> {
        self.battery.read(self.cap.get())
    }

    async fn set_charge_current_limit(&self, milliamps: u32) -> DeviceResult<bool> {
        self.battery.write()?;
        self.cap.set(milliamps);
        Ok(true)
    }
}

impl TouchpadControl for Board {
    async fn haptic_intensity(&self) -> DeviceResult<u8> {
        self.touchpad.read(self.haptic_intensity.get())
    }

    async fn set_haptic_intensity(&self, percent: u8) -> DeviceResult<()> {
        self.touchpad.write()?;
        self.haptic_intensity.set(percent);
        Ok(())
    }

    async fn click_force(&self) -> DeviceResult<ClickForce> {
        self.touchpad.read(self.click_force.get())
    }

    async fn set_click_force(&self, force: ClickForce) -> DeviceResult<()> {
        self.touchpad.write()?;
        self.click_force.set(force);
        Ok(())
    }
}

impl TouchscreenControl for Board {
    async fn enabled(&self) -> DeviceResult<bool> {
        self.touchscreen.read(self.enabled.get())
    }

    async fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        self.touchscreen.write()?;
        self.enabled.set(enabled);
        Ok(())
    }
}

impl PowerLedControl for Board {
    async fn brightness(&self) -> DeviceResult<(u8, PowerLedLevel)> {
        self.power_led.read((self.percent.get(), self.level.get()))
    }

    async fn levels(&self) -> DeviceResult<Vec<PowerLedLevel>> {
        self.power_led.read(PowerLedLevel::ALL.to_vec())
    }

    async fn set_level(&self, level: PowerLedLevel) -> DeviceResult<()> {
        self.power_led.write()?;
        self.level.set(level);
        Ok(())
    }

    async fn set_brightness(&self, percent: u8) -> DeviceResult<()> {
        self.power_led.write()?;
        self.percent.set(percent);
        self.level.set(PowerLedLevel::Custom);
        Ok(())
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
