use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use frameguin_contract::{
    Attached, BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, Board, BoardControl,
    ChargeCurrentLimit, ChargeFlow, ChargingLedControl, ChargingLedFeature, ChargingLedSide,
    ChassisControl, ChassisFeature, ChassisState, ClickForce, DeckState, DeviceError, DeviceResult,
    ExtenderStage, ExtenderState, PortSet, PortState, PortsControl, PowerLedControl, PowerLedLevel,
    PrivacyState, PrivacySwitchesControl, TouchpadControl, TouchscreenControl, UsbControl,
    UsbSpeed,
};

pub(crate) use crate::fixtures::{CAPACITY, NOMINAL_MILLIVOLTS, cap, port, state};

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
    }
}

/// What a stub does besides answer: refuses every write once told to, and
/// answers every read with `failing` where one is set.
#[derive(Default)]
pub(crate) struct Fault {
    refusing: Cell<bool>,
    failing: RefCell<Option<DeviceError>>,
}

impl Fault {
    pub(crate) fn failing(error: DeviceError) -> Self {
        let fault = Self::default();
        fault.fail(error);
        fault
    }

    /// Fails every read from here on, a device already detected included.
    pub(crate) fn fail(&self, error: DeviceError) {
        *self.failing.borrow_mut() = Some(error);
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
        self.failing.borrow().clone().map_or(Ok(value), Err)
    }
}

/// What a device answers where the hardware does not serve it at all.
pub(crate) fn absent() -> DeviceError {
    DeviceError::Absent("no such interface".into())
}

/// A machine of every column, each answering as its own fault says and
/// remembering what it was written. The one stub implementing every control
/// trait: detecting the set at once asks for that, and a control asks only
/// its own column.
pub(crate) struct Machine {
    pub(crate) board: Fault,
    pub(crate) battery: Fault,
    pub(crate) touchpad: Fault,
    pub(crate) touchscreen: Fault,
    pub(crate) power_led: Fault,
    pub(crate) charging_led: Fault,
    pub(crate) ports: Fault,
    pub(crate) chassis: Fault,
    pub(crate) privacy_switches: Fault,
    pub(crate) usb: Fault,
    pub(crate) limit: Cell<u8>,
    pub(crate) cap: Cell<ChargeCurrentLimit>,
    pub(crate) haptic_intensity: Cell<u8>,
    pub(crate) click_force: Cell<ClickForce>,
    pub(crate) enabled: Cell<bool>,
    pub(crate) percent: Cell<u8>,
    pub(crate) level: Cell<PowerLedLevel>,
    pub(crate) lit: Cell<bool>,
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            board: Fault::default(),
            battery: Fault::default(),
            touchpad: Fault::default(),
            touchscreen: Fault::default(),
            power_led: Fault::default(),
            charging_led: Fault::default(),
            ports: Fault::default(),
            chassis: Fault::default(),
            privacy_switches: Fault::default(),
            usb: Fault::default(),
            limit: Cell::new(100),
            cap: Cell::new(ChargeCurrentLimit::NoLimit),
            haptic_intensity: Cell::new(50),
            click_force: Cell::new(ClickForce::Low),
            enabled: Cell::new(true),
            percent: Cell::new(55),
            level: Cell::new(PowerLedLevel::High),
            lit: Cell::new(true),
        }
    }
}

impl Machine {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub(crate) fn failing(error: DeviceError) -> Rc<Self> {
        Rc::new(Self {
            battery: Fault::failing(error.clone()),
            touchpad: Fault::failing(error.clone()),
            touchscreen: Fault::failing(error.clone()),
            power_led: Fault::failing(error.clone()),
            charging_led: Fault::failing(error.clone()),
            ports: Fault::failing(error.clone()),
            chassis: Fault::failing(error.clone()),
            privacy_switches: Fault::failing(error.clone()),
            usb: Fault::failing(error),
            ..Self::default()
        })
    }
}

impl BoardControl for Machine {
    async fn board(&self) -> DeviceResult<Board> {
        self.board.read(Board::default())
    }
}

impl BatteryControl for Machine {
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

    async fn charge_current_limit(&self) -> DeviceResult<ChargeCurrentLimit> {
        self.battery.read(self.cap.get())
    }

    async fn set_charge_current_limit(&self, limit: ChargeCurrentLimit) -> DeviceResult<bool> {
        self.battery.write()?;
        self.cap.set(limit);
        Ok(true)
    }

    async fn extender(&self) -> DeviceResult<ExtenderState> {
        self.battery.read(ExtenderState {
            enabled: true,
            stage: ExtenderStage::Inactive,
            trigger_days: 5,
            first_stage_seconds: 3 * 86_400,
            reset_minutes: 30,
        })
    }
}

impl TouchpadControl for Machine {
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

impl PortsControl for Machine {
    async fn ports(&self, _controller_ports: PortSet) -> DeviceResult<Vec<PortState>> {
        self.ports.read((0..4).map(port).collect())
    }
}

impl PrivacySwitchesControl for Machine {
    async fn switches(&self) -> DeviceResult<PrivacyState> {
        self.privacy_switches.read(PrivacyState {
            camera: true,
            microphone: false,
        })
    }
}

impl UsbControl for Machine {
    async fn attached(&self) -> DeviceResult<Vec<Attached>> {
        self.usb.read(vec![Attached {
            controller: "0000:00:14.0".to_owned(),
            root_port: 5,
            vendor_id: 0x32ac,
            product_id: 0x0002,
            manufacturer: "Framework".to_owned(),
            product: "HDMI Expansion Card".to_owned(),
            speed: UsbSpeed::Full,
            firmware: String::new(),
            network: Vec::new(),
            storage: Vec::new(),
        }])
    }
}

impl ChassisControl for Machine {
    async fn state(&self) -> DeviceResult<ChassisState> {
        self.chassis.read(ChassisState {
            open: false,
            opened: 2,
            found_open: 1,
        })
    }

    async fn features(&self) -> DeviceResult<Vec<ChassisFeature>> {
        self.chassis.read(vec![ChassisFeature::Deck])
    }

    async fn deck_state(&self) -> DeviceResult<DeckState> {
        self.chassis.read(DeckState::On)
    }
}

impl TouchscreenControl for Machine {
    async fn enabled(&self) -> DeviceResult<bool> {
        self.touchscreen.read(self.enabled.get())
    }

    async fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        self.touchscreen.write()?;
        self.enabled.set(enabled);
        Ok(())
    }
}

impl PowerLedControl for Machine {
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

impl ChargingLedControl for Machine {
    async fn enabled(&self) -> DeviceResult<bool> {
        self.charging_led.read(self.lit.get())
    }

    async fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        self.charging_led.write()?;
        self.lit.set(enabled);
        Ok(())
    }

    async fn features(&self) -> DeviceResult<Vec<ChargingLedFeature>> {
        self.charging_led.read(vec![ChargingLedFeature::Side])
    }

    async fn side(&self) -> DeviceResult<ChargingLedSide> {
        self.charging_led.read(ChargingLedSide::Left)
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
