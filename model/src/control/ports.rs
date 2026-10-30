//! The USB-C ports: one read, which way power crosses each and so which
//! port is powering the machine, and the switch each port is.

use std::rc::Rc;

use frameguin_contract::{
    DeviceResult as Result, PdContract, Platform, PortMask, PortPartner, PortReading, PortSet,
    PortState, PortsControl, PowerRole, SupplyKind,
};

use super::present;
use crate::port::Placement;

pub struct Ports<C> {
    control: Rc<C>,
    placement: Placement,
    switchable: PortSet,
}

impl<C: PortsControl> Ports<C> {
    pub fn new(control: Rc<C>, platform: Platform, switchable: PortSet) -> Self {
        Self {
            control,
            placement: Placement::of(platform),
            switchable,
        }
    }

    pub async fn detect(control: &Rc<C>, platform: Platform) -> Result<Option<Self>> {
        let Some(switchable) = present(control.switchable().await)? else {
            return Ok(None);
        };
        Ok(Some(Self::new(control.clone(), platform, switchable)))
    }

    pub async fn read(&self) -> Result<Vec<PortState>> {
        self.control.ports().await
    }

    /// The ports offered a switch, fixed for the device's run.
    #[must_use]
    pub fn switchable(&self) -> PortSet {
        self.switchable
    }

    pub async fn set_enabled(&self, port: u8, enabled: bool) -> Result<()> {
        self.control.set_enabled(port, enabled).await
    }

    /// Where this board's sockets are, fixed for the device's run.
    #[must_use]
    pub fn placement(&self) -> Placement {
        self.placement
    }
}

/// Which way power crosses a port, said of the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    In,
    Out,
}

/// None where no power crosses the port. Power comes in through the port
/// the EC charges from, on its word alone, and goes out only to a partner
/// drawing it.
#[must_use]
pub fn flow(reading: &PortReading) -> Option<Flow> {
    if reading.charging {
        Some(Flow::In)
    } else if reading.partner == PortPartner::Sink && carries_power(reading) {
        Some(Flow::Out)
    } else {
        None
    }
}

/// Whether anything is attached: the partner the port reports, or the
/// charger the EC draws from where the port's entry reports none.
#[must_use]
pub fn attached(reading: &PortReading) -> bool {
    reading.partner != PortPartner::Nothing || flow(reading) == Some(Flow::In)
}

#[must_use]
pub fn carries_power(reading: &PortReading) -> bool {
    reading.millivolts != 0 && reading.milliamps != 0
}

/// The bus voltages a port's contract allows where its controller measures,
/// in millivolts, both ends included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoltageWindow {
    low: u32,
    high: u32,
}

impl VoltageWindow {
    #[must_use]
    pub fn low_millivolts(self) -> u32 {
        self.low
    }

    #[must_use]
    pub fn high_millivolts(self) -> u32 {
        self.high
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusVoltage {
    measured_millivolts: u16,
    window: Option<VoltageWindow>,
}

impl BusVoltage {
    #[must_use]
    pub fn measured_millivolts(self) -> u16 {
        self.measured_millivolts
    }

    /// None where no window applies: a port carrying no power, a contract on
    /// a supply other than a fixed one or whose offer did not read, or a
    /// power role the controller did not report.
    #[must_use]
    pub fn window(self) -> Option<VoltageWindow> {
        self.window
    }

    /// False where no window applies.
    #[must_use]
    pub fn out_of_range(self) -> bool {
        let measured = u32::from(self.measured_millivolts);
        self.window
            .is_some_and(|window| !(window.low..=window.high).contains(&measured))
    }
}

/// USB PD's vSrcNew for a fixed supply, at the source's connector.
const FIXED_TOLERANCE_PERCENT: u32 = 5;

/// Type-C's vSafe5V, at the source's connector.
const VSAFE5V_MILLIVOLTS: u32 = 5_000;
const VSAFE5V_LOW_MILLIVOLTS: u32 = 4_750;
const VSAFE5V_HIGH_MILLIVOLTS: u32 = 5_500;

/// Type-C's cable IR drop limit at a cable's rated current (section 4.4.1):
/// 500 mV on VBUS and 250 mV on ground.
const CABLE_DROP_MILLIVOLTS: u32 = 500 + 250;

/// None where the port's controller registers were not read.
#[must_use]
pub fn bus_voltage(reading: &PortReading) -> Option<BusVoltage> {
    let registers = reading.registers?;
    Some(BusVoltage {
        measured_millivolts: registers.measured_millivolts,
        window: voltage_window(reading, registers.pd),
    })
}

/// The controller measures on the machine's side of the cable.
fn voltage_window(reading: &PortReading, pd: Option<PdContract>) -> Option<VoltageWindow> {
    if !carries_power(reading) {
        return None;
    }
    let vsafe5v = VoltageWindow {
        low: VSAFE5V_LOW_MILLIVOLTS,
        high: VSAFE5V_HIGH_MILLIVOLTS,
    };
    let at_source = if reading.contract {
        if pd?.kind != SupplyKind::Fixed {
            return None;
        }
        match u32::from(reading.millivolts) {
            // USB PD's vSrcNew does not apply at vSafe5V.
            VSAFE5V_MILLIVOLTS => vsafe5v,
            target => VoltageWindow {
                low: target * (100 - FIXED_TOLERANCE_PERCENT) / 100,
                high: target * (100 + FIXED_TOLERANCE_PERCENT) / 100,
            },
        }
    } else {
        vsafe5v
    };
    match reading.power_role {
        PowerRole::Source => Some(at_source),
        PowerRole::Sink => Some(VoltageWindow {
            low: at_source.low.saturating_sub(CABLE_DROP_MILLIVOLTS),
            ..at_source
        }),
        PowerRole::Unknown => None,
    }
}

/// The port the machine is drawing its power through, by its index, and
/// None where none is. At most one answers, the EC picking among those
/// offering.
#[must_use]
pub fn powering(ports: &[PortState]) -> Option<(u8, &PortReading)> {
    ports.iter().find_map(|port| match &port.mask {
        PortMask::Off => None,
        PortMask::On(reading, _) | PortMask::Unchecked(reading, _) => {
            (flow(reading) == Some(Flow::In)).then_some((port.index, reading))
        }
    })
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use frameguin_contract::{
        DeviceError, DeviceResult as Result, Platform, PortMask, PortPartner, PortReading, PortSet,
        PortState, PowerRole, SupplyKind,
    };

    use super::{BusVoltage, Flow, Ports, VoltageWindow, attached, bus_voltage, flow, powering};
    use crate::fixtures::measured;
    use crate::testing::{Machine, absent, port, reading, ready};

    fn detect(machine: &Rc<Machine>) -> Result<Option<Ports<Machine>>> {
        ready(Ports::detect(machine, Platform::Unknown))
    }

    #[test]
    fn ports_the_hardware_answers_for_are_detected() {
        assert!(detect(&Machine::new()).unwrap().is_some());
    }

    #[test]
    fn a_board_the_hardware_serves_no_ports_for_is_absent() {
        let machine = Machine::failing(absent());
        assert!(detect(&machine).unwrap().is_none());
    }

    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_set_of_ports() {
        let error = DeviceError::Failed("no reply".into());
        let machine = Machine::failing(error.clone());
        assert_eq!(detect(&machine).err(), Some(error));
    }

    #[test]
    fn a_read_carries_every_port() {
        let ports = Ports::new(Machine::new(), Platform::Unknown, PortSet::default());
        let read = ready(ports.read()).unwrap();
        assert_eq!(read.len(), 4);
        assert_eq!(read[0], port(0));
    }

    #[test]
    fn the_port_charging_the_machine_is_the_one_powering_it() {
        let ports: Vec<_> = (0..4).map(port).collect();
        assert_eq!(powering(&ports).map(|(index, _)| index), Some(0));
        let idle: Vec<_> = (1..4).map(port).collect();
        assert!(powering(&idle).is_none());
    }

    #[test]
    fn a_port_whose_mask_went_unread_still_powers_the_machine() {
        let unchecked = PortState {
            index: 0,
            mask: PortMask::Unchecked(reading(0), None),
        };
        assert_eq!(powering(&[unchecked]).map(|(index, _)| index), Some(0));
    }

    #[test]
    fn power_comes_in_on_the_ecs_word_and_goes_out_only_with_watts() {
        let negotiating = PortReading {
            millivolts: 0,
            milliamps: 0,
            ..reading(0)
        };
        assert_eq!(flow(&negotiating), Some(Flow::In));
        let device = PortReading {
            partner: PortPartner::Sink,
            charging: false,
            millivolts: 5_000,
            milliamps: 400,
            ..reading(1)
        };
        assert_eq!(flow(&device), Some(Flow::Out));
        let idle = PortReading {
            milliamps: 0,
            ..device
        };
        assert_eq!(flow(&idle), None);
        let spare = PortReading {
            charging: false,
            ..reading(0)
        };
        assert_eq!(flow(&spare), None);
    }

    #[test]
    fn a_charger_the_ec_draws_from_is_attached_with_no_partner_reported() {
        let unnamed = PortReading {
            partner: PortPartner::Nothing,
            ..reading(0)
        };
        assert!(attached(&unnamed));
        let empty = PortReading {
            charging: false,
            ..unnamed
        };
        assert!(!attached(&empty));
    }

    #[test]
    fn detection_carries_the_switchable_ports() {
        let ports = detect(&Machine::new()).unwrap().expect("ports answered");
        assert!(ports.switchable().contains(3));
    }

    fn supplying(contract: bool) -> PortReading {
        PortReading {
            contract,
            power_role: PowerRole::Source,
            millivolts: 5000,
            ..reading(0)
        }
    }

    fn voltage(reading: &PortReading) -> BusVoltage {
        bus_voltage(reading).expect("registers read")
    }

    fn window(low: u32, high: u32) -> VoltageWindow {
        VoltageWindow { low, high }
    }

    #[test]
    fn a_fixed_contract_drawn_from_allows_its_tolerance_and_the_cables_drop() {
        let drawn = measured(&reading(0), Some(SupplyKind::Fixed), 19_800);
        assert_eq!(voltage(&drawn).window(), Some(window(18_250, 21_000)));
    }

    #[test]
    fn a_measured_voltage_on_either_end_of_the_window_is_within_it() {
        for (millivolts, out_of_range) in [
            (18_250, false),
            (21_000, false),
            (18_200, true),
            (21_100, true),
        ] {
            let drawn = measured(&reading(0), Some(SupplyKind::Fixed), millivolts);
            assert_eq!(
                voltage(&drawn).out_of_range(),
                out_of_range,
                "{millivolts} mV"
            );
        }
    }

    #[test]
    fn type_c_power_drawn_from_allows_its_range_and_the_cables_drop() {
        let type_c = PortReading {
            contract: false,
            millivolts: 5000,
            ..reading(0)
        };
        let drawn = measured(&type_c, None, 5_000);
        assert_eq!(voltage(&drawn).window(), Some(window(4_000, 5_500)));
    }

    #[test]
    fn power_the_machine_supplies_is_measured_with_no_cable_between() {
        let nine_volts = PortReading {
            millivolts: 9_000,
            ..supplying(true)
        };
        let pd = measured(&nine_volts, Some(SupplyKind::Fixed), 9_000);
        assert_eq!(voltage(&pd).window(), Some(window(8_550, 9_450)));
        let type_c = measured(&supplying(false), None, 5_000);
        assert_eq!(voltage(&type_c).window(), Some(window(4_750, 5_500)));
    }

    #[test]
    fn a_fixed_contract_at_five_volts_is_held_to_vsafe5v() {
        let five_volts = PortReading {
            millivolts: 5_000,
            ..reading(0)
        };
        let drawn = measured(&five_volts, Some(SupplyKind::Fixed), 5_000);
        assert_eq!(voltage(&drawn).window(), Some(window(4_000, 5_500)));
        let supplied = measured(&supplying(true), Some(SupplyKind::Fixed), 5_000);
        assert_eq!(voltage(&supplied).window(), Some(window(4_750, 5_500)));
    }

    #[test]
    fn a_supply_with_no_fixed_voltage_is_unchecked() {
        for kind in [
            SupplyKind::Pps,
            SupplyKind::SprAvs,
            SupplyKind::EprAvs,
            SupplyKind::Variable,
            SupplyKind::Battery,
            SupplyKind::Reserved,
        ] {
            let drawn = measured(&reading(0), Some(kind), 9_000);
            assert_eq!(voltage(&drawn).window(), None, "{kind:?}");
        }
    }

    #[test]
    fn a_contract_whose_offer_did_not_read_is_unchecked() {
        let drawn = measured(&reading(0), None, 9_000);
        assert_eq!(voltage(&drawn).window(), None);
    }

    #[test]
    fn a_power_role_the_controller_did_not_report_is_unchecked() {
        let unknown = PortReading {
            power_role: PowerRole::Unknown,
            ..reading(0)
        };
        let drawn = measured(&unknown, Some(SupplyKind::Fixed), 20_000);
        assert_eq!(voltage(&drawn).window(), None);
    }

    #[test]
    fn a_port_carrying_no_power_is_unchecked() {
        let idle = measured(&reading(1), None, 0);
        assert_eq!(voltage(&idle).window(), None);
    }

    #[test]
    fn the_bus_voltage_carries_what_the_controller_measured() {
        let drawn = measured(&reading(0), Some(SupplyKind::Fixed), 19_800);
        assert_eq!(
            bus_voltage(&drawn).map(BusVoltage::measured_millivolts),
            Some(19_800)
        );
    }

    #[test]
    fn a_port_whose_registers_did_not_read_has_no_bus_voltage() {
        assert_eq!(bus_voltage(&reading(0)), None);
    }
}
