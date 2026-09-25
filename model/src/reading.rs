//! One reading of the machine: what a front end asked to be read, read in one
//! pass over the controls, and every value that arrived.
//!
//! Nothing here needs a pack. The block is an absent extra on a board with
//! none, the way a failed read is, and the ports are still read.
//!
//! The reading is always the whole block, never the summary the battery row
//! alone would need. The daemon walks the same memmap for either, and the two
//! figures it reaches past the memmap for are asked of the pack once per
//! daemon run and remembered — so the wide call costs a few strings of
//! marshalling, where the narrow one would cost a branch that can be wrong,
//! and being wrong here means a report showing a row it did not read.

use frameguin_contract::{
    Attached, BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, ChargingLedControl,
    ChargingLedFeature, ChargingLedSide, ChassisControl, ChassisFeature, ChassisState, DeckState,
    DeviceError, DeviceResult, ExtenderState, PortSet, PortState, PortsControl, PrivacyState,
    PrivacySwitchesControl, UsbControl,
};

use crate::control::Controls;
use crate::control::battery::ChargeSpeeds;

/// The extras one reading asks for.
///
/// A set rather than a flag apiece in the signatures: each extra is its own
/// call to the daemon, over a connection whose calls block one another, so a
/// reading asking for none of them costs none of them — and a new extra
/// should be a field here rather than another parameter everywhere.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent extra; `with` merges them by OR, and no combination is invalid"
)]
pub struct Request {
    /// The EC's battery block.
    pub battery: bool,
    /// A transfer per cell plus two, each an EC host command carrying an
    /// `SMBus` pair to a gauge the EC is itself polling.
    pub condition: bool,
    /// The USB-C ports. One call however many there are, and host commands
    /// only.
    pub ports: bool,
    /// The ports whose controller registers are read with the ports, asking
    /// for the ports too: an I2C transfer to a PD controller per port asked
    /// for that has something attached.
    pub controller_ports: PortSet,
    /// Two host commands and no transfer past them.
    pub chassis: bool,
    /// One host command.
    pub deck: bool,
    /// One host command, whose handler prints two lines to the EC console.
    pub privacy_switches: bool,
    /// The extender and the charge limit its window is weighed against: two
    /// host commands.
    pub extender: bool,
    /// The devices on the USB root ports: a walk of sysfs, no EC transfer.
    pub usb: bool,
    /// Two host commands.
    pub charging_led_side: bool,
}

impl Request {
    /// Every extra either asks for.
    #[must_use]
    pub fn with(self, other: Self) -> Self {
        Self {
            battery: self.battery || other.battery,
            condition: self.condition || other.condition,
            ports: self.ports || other.ports,
            controller_ports: self.controller_ports.union(other.controller_ports),
            chassis: self.chassis || other.chassis,
            deck: self.deck || other.deck,
            privacy_switches: self.privacy_switches || other.privacy_switches,
            extender: self.extender || other.extender,
            usb: self.usb || other.usb,
            charging_led_side: self.charging_led_side || other.charging_led_side,
        }
    }
}

/// An extra is None where nothing asked for it and where the ask failed alike.
pub struct Reading {
    pub info: Option<BatteryInfo>,
    /// Arrives with the block.
    pub charge_speeds: Option<ChargeSpeeds>,
    pub condition: Option<BatteryCondition>,
    pub ports: Option<Vec<PortState>>,
    pub chassis: Option<ChassisState>,
    pub deck: Option<DeckState>,
    pub privacy_switches: Option<PrivacyState>,
    pub extender: Option<ExtenderState>,
    /// Read beside the extender, and None on a board with no charge limit.
    pub charge_limit: Option<u8>,
    pub usb: Option<Vec<Attached>>,
    pub charging_led_side: Option<ChargingLedSide>,
}

/// One extra a [`Request`] can ask for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Extra {
    Battery,
    Condition,
    Ports,
    Chassis,
    Deck,
    PrivacySwitches,
    Extender,
    ChargeLimit,
    Usb,
    ChargingLedSide,
}

impl Extra {
    #[must_use]
    pub fn requested(self, request: Request) -> bool {
        match self {
            Self::Battery => request.battery,
            Self::Condition => request.condition,
            Self::Ports => request.ports || !request.controller_ports.is_empty(),
            Self::Chassis => request.chassis,
            Self::Deck => request.deck,
            Self::PrivacySwitches => request.privacy_switches,
            Self::Extender | Self::ChargeLimit => request.extender,
            Self::Usb => request.usb,
            Self::ChargingLedSide => request.charging_led_side,
        }
    }

    /// What a report of this read's failure calls the attempt.
    #[must_use]
    pub fn attempt(self) -> &'static str {
        match self {
            Self::Battery => "Reading the battery",
            Self::Condition => "Reading the battery's condition",
            Self::Ports => "Reading the USB-C ports",
            Self::Chassis => "Reading the chassis",
            Self::Deck => "Reading the input deck",
            Self::PrivacySwitches => "Reading the privacy switches",
            Self::Extender => "Reading the battery extender",
            Self::ChargeLimit => "Reading the charge limit",
            Self::Usb => "Reading the USB devices",
            Self::ChargingLedSide => "Reading the charging LED's side",
        }
    }
}

#[derive(Debug)]
pub struct Failure {
    pub extra: Extra,
    pub error: DeviceError,
}

struct Extras {
    request: Request,
    failures: Vec<Failure>,
}

impl Extras {
    /// None where nothing asked for `extra`, where the board has no device to
    /// ask, where the read failed, and once the daemon has not answered.
    async fn read<T>(
        &mut self,
        extra: Extra,
        read: Option<impl Future<Output = DeviceResult<T>>>,
    ) -> Option<T> {
        if !extra.requested(self.request) || self.unreachable() {
            return None;
        }
        match read?.await {
            Ok(value) => Some(value),
            Err(error) => {
                self.failures.push(Failure { extra, error });
                None
            }
        }
    }

    /// A daemon that left one call unanswered leaves every later call waiting
    /// as long.
    fn unreachable(&self) -> bool {
        self.failures
            .iter()
            .any(|failure| matches!(failure.error, DeviceError::Unreachable(_)))
    }
}

/// Reads what `request` asks for, and hands back the extras that failed. A
/// device the board does not have, or an extra its device does not offer, is
/// not a failure: its field arrives as None, as one nothing asked for does.
pub async fn read<C>(controls: &Controls<C>, request: Request) -> (Reading, Vec<Failure>)
where
    C: BatteryControl
        + ChargingLedControl
        + ChassisControl
        + PortsControl
        + PrivacySwitchesControl
        + UsbControl,
{
    let mut extras = Extras {
        request,
        failures: Vec::new(),
    };
    let battery = controls.battery.as_ref();
    let info = extras.read(Extra::Battery, battery.map(|b| b.read())).await;
    let charge_speeds = info.as_ref().and(battery).and_then(|b| b.charge_speeds());
    let condition = extras
        .read(
            Extra::Condition,
            battery
                .filter(|b| b.has(BatteryFeature::Condition))
                .map(|b| b.condition()),
        )
        .await;
    // Asked of the ports control rather than the pack's: a board can have
    // one and not the other.
    let ports = extras
        .read(
            Extra::Ports,
            controls
                .ports
                .as_ref()
                .map(|p| p.read(request.controller_ports)),
        )
        .await;
    let chassis_control = controls.chassis.as_ref();
    let chassis = extras
        .read(Extra::Chassis, chassis_control.map(|c| c.read()))
        .await;
    let deck = extras
        .read(
            Extra::Deck,
            chassis_control
                .filter(|c| c.has(ChassisFeature::Deck))
                .map(|c| c.deck_state()),
        )
        .await;
    let privacy_switches = extras
        .read(
            Extra::PrivacySwitches,
            controls.privacy_switches.as_ref().map(|s| s.read()),
        )
        .await;
    let extender = extras
        .read(
            Extra::Extender,
            battery
                .filter(|b| b.has(BatteryFeature::Extender))
                .map(|b| b.extender()),
        )
        .await;
    let charge_limit = extras
        .read(
            Extra::ChargeLimit,
            battery
                .filter(|b| b.has(BatteryFeature::ChargeLimit))
                .map(|b| b.charge_limit()),
        )
        .await;
    let usb = extras
        .read(Extra::Usb, controls.usb.as_ref().map(|u| u.read()))
        .await;
    let charging_led_side = extras
        .read(
            Extra::ChargingLedSide,
            controls
                .charging_led
                .as_ref()
                .filter(|l| l.has(ChargingLedFeature::Side))
                .map(|l| l.side()),
        )
        .await;
    let reading = Reading {
        info,
        charge_speeds,
        condition,
        ports,
        chassis,
        deck,
        privacy_switches,
        extender,
        charge_limit,
        usb,
        charging_led_side,
    };
    (reading, extras.failures)
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use frameguin_contract::{DeviceError, PortSet};

    use super::{Extra, Request, read};
    use crate::control::Controls;
    use crate::testing::{Machine, ready};

    fn detected() -> (Rc<Machine>, Controls<Machine>) {
        let machine = Machine::new();
        let controls = ready(Controls::detect(&machine)).unwrap();
        (machine, controls)
    }

    #[test]
    fn nothing_requested_reads_nothing() {
        let (_, controls) = detected();
        let (reading, failures) = ready(read(&controls, Request::default()));
        assert!(reading.info.is_none());
        assert!(reading.ports.is_none());
        assert!(reading.chassis.is_none());
        assert!(failures.is_empty());
    }

    #[test]
    fn a_requested_extra_arrives_and_the_rest_do_not() {
        let (_, controls) = detected();
        let request = Request {
            battery: true,
            ..Request::default()
        };
        let (reading, failures) = ready(read(&controls, request));
        assert!(reading.info.is_some());
        assert!(reading.charge_speeds.is_some());
        assert!(reading.chassis.is_none());
        assert!(failures.is_empty());
    }

    #[test]
    fn a_failed_extra_is_reported_and_arrives_as_none() {
        let (machine, controls) = detected();
        machine.chassis.fail(DeviceError::Failed("no reply".into()));
        let request = Request {
            battery: true,
            chassis: true,
            ..Request::default()
        };
        let (reading, failures) = ready(read(&controls, request));
        assert!(reading.info.is_some());
        assert!(reading.chassis.is_none());
        let failed: Vec<Extra> = failures.iter().map(|f| f.extra).collect();
        assert_eq!(failed, [Extra::Chassis]);
    }

    #[test]
    fn an_unreachable_daemon_stops_the_reads_after_it() {
        let (machine, controls) = detected();
        machine
            .battery
            .fail(DeviceError::Unreachable("gone".into()));
        let request = Request {
            battery: true,
            chassis: true,
            privacy_switches: true,
            ..Request::default()
        };
        let (reading, failures) = ready(read(&controls, request));
        assert!(reading.chassis.is_none());
        assert!(reading.privacy_switches.is_none());
        let failed: Vec<Extra> = failures.iter().map(|f| f.extra).collect();
        assert_eq!(failed, [Extra::Battery]);
    }

    #[test]
    fn an_extra_the_device_does_not_offer_is_not_read() {
        let (_, controls) = detected();
        let request = Request {
            battery: true,
            condition: true,
            extender: true,
            ..Request::default()
        };
        let (reading, failures) = ready(read(&controls, request));
        assert!(reading.info.is_some());
        assert!(reading.condition.is_none());
        assert!(reading.extender.is_none());
        assert!(failures.is_empty());
    }

    #[test]
    fn controller_ports_alone_ask_for_the_ports() {
        let request = Request {
            controller_ports: PortSet::of(0),
            ..Request::default()
        };
        assert!(Extra::Ports.requested(request));
        assert!(!Extra::Battery.requested(request));
    }
}
