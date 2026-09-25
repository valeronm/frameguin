//! One module per control: its detection, its read, its commands, its words.

pub mod battery;
pub mod charging_led;
pub mod chassis;
pub mod ports;
pub mod power_led;
pub mod privacy_switches;
pub mod touchpad;
pub mod touchscreen;
pub mod usb;

use std::rc::Rc;

use frameguin_contract::{
    BatteryControl, Board, BoardControl, ChargingLedControl, ChassisControl, DeviceError,
    DeviceResult, PortsControl, PowerLedControl, PrivacySwitchesControl, TouchpadControl,
    TouchscreenControl, UsbControl,
};

use crate::port::Placement;

/// Whether a device is there, decided by the device's own path: a read the
/// control answers is the device, one it answers `Absent` is no device, and
/// anything else is the device being unreachable, which says nothing about
/// presence and is passed up as the error it is.
fn present<T>(probe: DeviceResult<T>) -> DeviceResult<Option<T>> {
    match probe {
        Ok(answer) => Ok(Some(answer)),
        Err(DeviceError::Absent(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Nothing is powering the machine — a state two controls reach from
/// different devices, the EC's own flag and the USB-C ports, and one neither
/// of them owns.
pub(crate) const NO_SUPPLY: &str = "Not connected";

/// A two-state reading, worded alike by every control that shows one.
#[must_use]
pub(crate) fn yes_no(set: bool) -> &'static str {
    if set { "Yes" } else { "No" }
}

/// The controls this board has, each behind the one implementation of the
/// control traits. `None` is a control whose device answered for itself as
/// absent, and the front-ends gate on that.
pub struct Controls<C> {
    pub board: Board,
    pub battery: Option<Rc<battery::Battery<C>>>,
    pub touchpad: Option<Rc<touchpad::Touchpad<C>>>,
    pub touchscreen: Option<Rc<touchscreen::Touchscreen<C>>>,
    pub power_led: Option<Rc<power_led::PowerLed<C>>>,
    pub charging_led: Option<Rc<charging_led::ChargingLed<C>>>,
    pub ports: Option<Rc<ports::Ports<C>>>,
    pub chassis: Option<Rc<chassis::Chassis<C>>>,
    pub privacy_switches: Option<Rc<privacy_switches::PrivacySwitches<C>>>,
    pub usb: Option<Rc<usb::Usb<C>>>,
}

impl<
    C: BoardControl
        + BatteryControl
        + TouchpadControl
        + TouchscreenControl
        + PowerLedControl
        + ChargingLedControl
        + PortsControl
        + ChassisControl
        + PrivacySwitchesControl
        + UsbControl,
> Controls<C>
{
    /// Asks each control's device to detect itself. Fails only where the
    /// device could not be asked at all — an absent device is an answer, not
    /// a failure. The ports' device does not know where its sockets are, and
    /// nothing it reads says so.
    pub async fn detect(control: &Rc<C>) -> DeviceResult<Self> {
        let board = control.board().await?;
        Ok(Self {
            battery: battery::Battery::detect(control).await?.map(Rc::new),
            touchpad: touchpad::Touchpad::detect(control).await?.map(Rc::new),
            touchscreen: touchscreen::Touchscreen::detect(control)
                .await?
                .map(Rc::new),
            power_led: power_led::PowerLed::detect(control).await?.map(Rc::new),
            charging_led: charging_led::ChargingLed::detect(control)
                .await?
                .map(Rc::new),
            ports: ports::Ports::detect(control, Placement::of(board.platform()))
                .await?
                .map(Rc::new),
            chassis: chassis::Chassis::detect(control).await?.map(Rc::new),
            privacy_switches: privacy_switches::PrivacySwitches::detect(control)
                .await?
                .map(Rc::new),
            usb: usb::Usb::detect(control).await?.map(Rc::new),
            board,
        })
    }

    /// Whether the main window has nothing to show: the readings only the
    /// Readings window carries are left out.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.battery.is_none()
            && self.touchpad.is_none()
            && self.touchscreen.is_none()
            && self.power_led.is_none()
            && self.charging_led.is_none()
            && self.ports.is_none()
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use frameguin_contract::DeviceError;

    use super::Controls;
    use crate::testing::{Fault, Machine, absent, ready};

    fn detect(machine: &Rc<Machine>) -> frameguin_contract::DeviceResult<Controls<Machine>> {
        ready(Controls::detect(machine))
    }

    #[test]
    fn every_control_its_device_answered_for_is_there() {
        let controls = detect(&Machine::new()).unwrap();
        assert!(controls.battery.is_some());
        assert!(controls.touchpad.is_some());
        assert!(controls.touchscreen.is_some());
        assert!(controls.power_led.is_some());
        assert!(controls.charging_led.is_some());
        assert!(controls.chassis.is_some());
        assert!(controls.privacy_switches.is_some());
        assert!(controls.usb.is_some());
        assert!(!controls.is_empty());
    }

    #[test]
    fn a_board_whose_devices_all_answer_absent_has_no_controls() {
        let controls = detect(&Machine::failing(absent())).unwrap();
        assert!(controls.is_empty());
    }

    #[test]
    fn an_absent_device_takes_only_its_own_control() {
        let machine = Machine {
            touchpad: Fault::failing(absent()),
            ..Machine::default()
        };
        let controls = detect(&Rc::new(machine)).unwrap();
        assert!(controls.touchpad.is_none());
        assert!(controls.battery.is_some());
        assert!(controls.touchscreen.is_some());
        assert!(controls.power_led.is_some());
        assert!(controls.chassis.is_some());
        assert!(controls.privacy_switches.is_some());
        assert!(controls.usb.is_some());
    }

    #[test]
    fn a_board_that_cannot_be_read_fails_the_whole_detection() {
        let error = DeviceError::Failed("no reply".into());
        let machine = Machine {
            board: Fault::failing(error.clone()),
            ..Machine::default()
        };
        assert_eq!(detect(&Rc::new(machine)).err(), Some(error));
    }

    #[test]
    fn hardware_that_cannot_be_asked_fails_the_whole_detection() {
        let error = DeviceError::Failed("no reply".into());
        let machine = Machine::failing(error.clone());
        assert_eq!(detect(&machine).err(), Some(error));
    }
}
