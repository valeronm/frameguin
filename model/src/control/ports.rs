//! The USB-C ports: one read, and the words a reader needs for it.

use std::rc::Rc;

use frameguin_wire::{
    Cable, CableLatency, CableSpeed, DataRole, DeviceResult as Result, Epr, PortPartner, PortState,
    PortsControl, PowerRole,
};

use super::present;
use crate::port::Placement;

pub struct Ports<C> {
    control: Rc<C>,
    placement: Placement,
}

impl<C: PortsControl> Ports<C> {
    pub fn new(control: Rc<C>, placement: Placement) -> Self {
        Self { control, placement }
    }

    pub async fn detect(control: &Rc<C>, placement: Placement) -> Result<Option<Self>> {
        Ok(present(control.ports(0).await)?.map(|_| Self::new(control.clone(), placement)))
    }

    pub async fn read(&self, controller_ports: u8) -> Result<Vec<PortState>> {
        self.control.ports(controller_ports).await
    }

    /// Where this board's sockets are, fixed for the device's run.
    #[must_use]
    pub fn placement(&self) -> Placement {
        self.placement
    }
}

/// An empty port, worded as the absence it is rather than as a kind of
/// partner.
pub const NOTHING_ATTACHED: &str = "Nothing attached";

/// What is attached, in the words a reader would use for it. None for an
/// empty port, which the caller words as [`NOTHING_ATTACHED`].
#[must_use]
pub fn partner_label(partner: PortPartner) -> Option<&'static str> {
    Some(match partner {
        PortPartner::Nothing => return None,
        PortPartner::Sink => "Drawing power",
        PortPartner::Source => "Supplying power",
        PortPartner::Debug => "Debug accessory",
        PortPartner::Audio => "Audio accessory",
        PortPartner::PoweredAccessory => "Powered accessory",
        PortPartner::Unsupported => "Unsupported",
        PortPartner::Invalid => "Not recognised",
    })
}

/// What the link carries as a person reads it — volts, amps and the watts
/// they come to. A port with no power delivery contract still carries what
/// Type-C's own resistors advertise. None where the port settled on
/// nothing, a supply of zero being worth no row.
#[must_use]
pub fn carried(port: &PortState) -> Option<String> {
    if port.millivolts == 0 || port.milliamps == 0 {
        return None;
    }
    Some(format!(
        "{}, {:.2} A ({})",
        volts(port.millivolts),
        f64::from(port.milliamps) / 1000.0,
        watts(port),
    ))
}

/// The bus voltage the port's controller measured, and None where it was not
/// read.
#[must_use]
pub fn measured_label(port: &PortState) -> Option<String> {
    (port.measured_millivolts > 0).then(|| volts(port.measured_millivolts))
}

fn volts(millivolts: u16) -> String {
    format!("{:.1} V", f64::from(millivolts) / 1000.0)
}

/// The port the machine draws its power through, named as that rather than as
/// one more charger among those that may be attached.
pub const POWERING_THE_MACHINE: &str = "Powering the machine";

/// One port on one line: what is attached — the device in the slot where
/// one was placed there, named for powering the machine where it does — and
/// the watts it carries where it carries any.
///
/// A lead naming the device in the slot says nothing about which way its
/// power goes. The arrow is the direction for the thing attached, not for
/// the machine.
#[must_use]
pub fn port_summary(port: &PortState, device: Option<&str>) -> String {
    let partner = partner_label(port.partner);
    let Some(lead) = (if port.charging && (partner.is_some() || device.is_some()) {
        Some(POWERING_THE_MACHINE)
    } else {
        device.or(partner)
    }) else {
        return NOTHING_ATTACHED.to_owned();
    };
    if port.millivolts == 0 || port.milliamps == 0 {
        return lead.to_owned();
    }
    let flow = match port.partner {
        PortPartner::Sink => "↓ ",
        PortPartner::Source => "↑ ",
        _ => "",
    };
    format!("{lead} · {flow}{}", watts(port))
}

fn watts(port: &PortState) -> String {
    let watts = f64::from(port.millivolts) * f64::from(port.milliamps) / 1_000_000.0;
    format!("{} W", crate::part::trimmed(format!("{watts:.1}")))
}

/// The port the machine is drawing its power through, and None where none
/// is. At most one answers, the EC picking among those offering.
#[must_use]
pub fn powering(ports: &[PortState]) -> Option<&PortState> {
    ports.iter().find(|port| port.charging)
}

/// What the machine is being powered with, for a row that has one line for
/// it: the watts where a port is supplying them, and the absence named
/// where none is.
///
/// Named for the supply rather than the charger, `battery::reading` having
/// its own `charger_label` for whether the EC sees one attached at all — two
/// answers about a charger, and a shared name would leave which one a row
/// shows decided by its import. The word for the absence they do share.
#[must_use]
pub fn supply_label(ports: &[PortState]) -> String {
    powering(ports).map_or_else(|| super::NO_SUPPLY.to_owned(), watts)
}

/// Where the power is coming in, to sit under [`supply_label`]. None where
/// nothing is supplying — a port named under "Disconnected" would name the
/// one that stopped.
#[must_use]
pub fn supply_port(ports: &[PortState], placement: Placement) -> Option<String> {
    powering(ports).map(|port| placement.label(port.index))
}

/// The supply and where it comes in, joined for a caller with one line to
/// put both on. Just the supply where nothing is supplying, there being no
/// port to name then — an unmeasured board still joins, its port named by
/// number.
///
/// The words only; what the line is *about* is the caller's to say, as a row
/// title is everywhere else.
#[must_use]
pub fn supply_summary(ports: &[PortState], placement: Placement) -> String {
    let supply = supply_label(ports);
    powering(ports).map_or(supply.clone(), |port| {
        format!("{supply} · {}", placement.inline(port.index))
    })
}

/// None where the port is not the one powering the machine.
#[must_use]
pub fn powering_label(port: &PortState) -> Option<&'static str> {
    port.charging.then_some("Yes")
}

/// `DisplayPort` alternate mode, and None where the port is not in it.
#[must_use]
pub fn display_port_label(port: &PortState) -> Option<&'static str> {
    port.video.then_some("Connected")
}

/// Whether the link negotiated power delivery or is carrying power on
/// Type-C's own terms.
#[must_use]
pub fn contract_label(contract: bool) -> &'static str {
    if contract {
        "Power delivery"
    } else {
        "Type-C only"
    }
}

/// What the far end does with power, read off the machine's own role, the
/// two ends of a link being opposites. None where [`partner_label`] already
/// says it.
#[must_use]
pub fn power_role_label(partner: PortPartner, role: PowerRole) -> Option<&'static str> {
    if matches!(
        (partner, role),
        (PortPartner::Sink, PowerRole::Source) | (PortPartner::Source, PowerRole::Sink)
    ) {
        return None;
    }
    Some(match role {
        PowerRole::Sink => "Supplying power",
        PowerRole::Source => "Drawing power",
        PowerRole::Unknown => "Unknown",
    })
}

/// Which end drives the data link, said of the far end, the machine's own
/// role being what the EC reports.
#[must_use]
pub fn data_role_label(role: DataRole) -> &'static str {
    match role {
        DataRole::UpstreamFacing => "Host",
        DataRole::DownstreamFacing => "Peripheral",
        DataRole::Disconnected => "Disconnected",
        DataRole::Unknown => "Unknown",
    }
}

/// Extended power range, and None where the port does not offer it — a
/// capability nothing on this machine can use is not worth a row.
#[must_use]
pub fn epr_label(epr: Epr) -> Option<&'static str> {
    match epr {
        Epr::Unsupported => None,
        Epr::Supported => Some("Supported"),
        Epr::Active => Some("Active"),
    }
}

/// A cable whose controller found no e-marker in it.
pub const NO_E_MARKER: &str = "None";

/// The rate a full-featured Type-C cable of that signaling carries over both
/// lanes, as cables are certified and sold, and None for a reserved code.
#[must_use]
pub fn cable_speed_label(speed: CableSpeed) -> Option<&'static str> {
    match speed {
        CableSpeed::Usb2 => Some("USB 2.0"),
        CableSpeed::Gen1 => Some("10 Gbps"),
        CableSpeed::Gen2 => Some("20 Gbps"),
        CableSpeed::Gen3 => Some("40 Gbps"),
        CableSpeed::Gen4 => Some("80 Gbps"),
        CableSpeed::Unknown => None,
    }
}

/// The current a cable is rated for and the watts it is sold by, and None
/// where the e-marker left the current at a reserved code.
///
/// The watts are what a contract can reach over the cable, 48 V under
/// extended power range and 20 V otherwise: a cable's voltage rating alone
/// does not let a charger enter extended power range, its EPR bit does.
#[must_use]
pub fn cable_rating_label(cable: &Cable) -> Option<String> {
    let amps = cable.milliamps / 1000;
    let contract_volts = if cable.epr { 48 } else { 20 };
    (amps > 0).then(|| format!("{amps} A ({} W)", contract_volts * amps))
}

/// The length the PD specification ties to each latency class, and None
/// for a class it reserves or an active cable's.
#[must_use]
pub fn cable_length_label(latency: CableLatency) -> Option<&'static str> {
    match latency {
        CableLatency::Under10Ns => Some("About 1 m"),
        CableLatency::Under20Ns => Some("About 2 m"),
        CableLatency::Under30Ns => Some("About 3 m"),
        CableLatency::Under40Ns => Some("About 4 m"),
        CableLatency::Under50Ns => Some("About 5 m"),
        CableLatency::Under60Ns => Some("About 6 m"),
        CableLatency::Under70Ns => Some("About 7 m"),
        CableLatency::Over70Ns => Some("Over 7 m"),
        CableLatency::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use frameguin_wire::{
        Cable, CableLatency, CableMarking, CableSpeed, DataRole, DeviceError,
        DeviceResult as Result, PortPartner, PortState, PowerRole,
    };

    use super::{
        Ports, cable_length_label, cable_rating_label, cable_speed_label, carried, data_role_label,
        display_port_label, measured_label, partner_label, port_summary, power_role_label,
        powering, powering_label, supply_label, supply_summary,
    };
    use crate::port::Placement;
    use crate::testing::{Machine, absent, port, ready};

    fn detect(machine: &Rc<Machine>) -> Result<Option<Ports<Machine>>> {
        ready(Ports::detect(machine, Placement::default()))
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
        let ports = Ports::new(Machine::new(), Placement::default());
        let read = ready(ports.read(0)).unwrap();
        assert_eq!(read.len(), 4);
        assert!(read[0].charging);
    }

    #[test]
    fn a_measured_voltage_reads_to_a_tenth_of_a_volt() {
        let measured = PortState {
            measured_millivolts: 20_100,
            ..port(0)
        };
        assert_eq!(measured_label(&measured).as_deref(), Some("20.1 V"));
        assert_eq!(measured_label(&port(0)), None);
    }

    #[test]
    fn what_a_link_carries_reads_as_volts_amps_and_watts() {
        assert_eq!(carried(&port(0)).as_deref(), Some("20.0 V, 5.00 A (100 W)"));
    }

    #[test]
    fn a_contract_short_of_a_whole_watt_keeps_its_tenth() {
        let usb = PortState {
            millivolts: 5_000,
            milliamps: 1_500,
            ..port(0)
        };
        assert_eq!(carried(&usb).as_deref(), Some("5.0 V, 1.50 A (7.5 W)"));
        assert_eq!(supply_label(&[usb]), "7.5 W");
    }

    #[test]
    fn a_port_that_settled_on_nothing_carries_nothing_to_show() {
        assert_eq!(carried(&port(1)), None);
    }

    #[test]
    fn the_charger_row_names_the_watts_of_the_port_supplying_them() {
        let ports: Vec<_> = (0..4).map(port).collect();
        assert_eq!(powering(&ports).map(|p| p.index), Some(0));
        assert_eq!(supply_label(&ports), "100 W");
    }

    #[test]
    fn nothing_supplying_reads_as_disconnected_rather_than_zero_watts() {
        let ports: Vec<_> = (1..4).map(port).collect();
        assert!(powering(&ports).is_none());
        assert_eq!(supply_label(&ports), "Disconnected");
        assert_eq!(supply_label(&[]), "Disconnected");
    }

    /// One line carrying what the window spreads over a value and a
    /// subtitle, an unmeasured board included — its port is named by number
    /// rather than left out.
    #[test]
    fn the_joined_line_carries_the_supply_and_its_port() {
        let ports: Vec<_> = (0..4).map(port).collect();
        let summary = |platform| supply_summary(&ports, Placement::of(platform));
        assert_eq!(
            summary(frameguin_wire::Platform::Laptop13ProUltra3),
            "100 W · Right front"
        );
        assert_eq!(summary(frameguin_wire::Platform::Unknown), "100 W · Port 0");
        assert_eq!(
            summary(frameguin_wire::Platform::Laptop13AmdAi300),
            "100 W · Right, port 0"
        );
    }

    /// Nothing supplying means no port to name, and the separator would
    /// otherwise dangle.
    #[test]
    fn a_line_with_nothing_supplying_names_no_port() {
        assert_eq!(supply_summary(&[], Placement::default()), "Disconnected");
    }

    #[test]
    fn an_empty_port_is_worded_as_absence_rather_than_a_partner() {
        assert_eq!(partner_label(PortPartner::Nothing), None);
        assert_eq!(partner_label(PortPartner::Source), Some("Supplying power"));
    }

    #[test]
    fn a_port_line_names_the_partner_and_the_watts_of_its_contract() {
        let second_charger = PortState {
            charging: false,
            ..port(0)
        };
        assert_eq!(
            port_summary(&second_charger, None),
            "Supplying power · ↑ 100 W"
        );
        assert_eq!(port_summary(&port(1), None), "Nothing attached");
    }

    #[test]
    fn the_port_the_machine_draws_from_is_named_for_that_among_chargers() {
        assert_eq!(
            port_summary(&port(0), None),
            "Powering the machine · ↑ 100 W"
        );
    }

    #[test]
    fn a_partner_with_no_contract_is_named_without_watts() {
        let accessory = PortState {
            partner: PortPartner::Audio,
            ..port(1)
        };
        assert_eq!(port_summary(&accessory, None), "Audio accessory");
    }

    #[test]
    fn a_device_in_the_slot_leads_its_ports_summary() {
        let card = PortState {
            partner: PortPartner::Sink,
            charging: false,
            millivolts: 5_000,
            milliamps: 680,
            ..port(0)
        };
        assert_eq!(
            port_summary(&card, Some("HDMI Expansion Card")),
            "HDMI Expansion Card · ↓ 3.4 W"
        );
    }

    #[test]
    fn powering_the_machine_outranks_the_device_in_the_slot() {
        assert_eq!(
            port_summary(&port(0), Some("USB-C Hub")),
            "Powering the machine · ↑ 100 W"
        );
    }

    #[test]
    fn an_empty_port_with_no_device_is_still_nothing_attached() {
        assert_eq!(port_summary(&port(1), None), "Nothing attached");
    }

    #[test]
    fn powering_the_machine_outranks_a_device_even_when_the_partner_is_nothing() {
        let charger = PortState {
            partner: PortPartner::Nothing,
            ..port(0)
        };
        assert_eq!(
            port_summary(&charger, Some("HDMI Expansion Card")),
            "Powering the machine · 100 W"
        );
    }

    #[test]
    fn a_device_with_no_partner_and_no_contract_is_named_alone() {
        assert_eq!(
            port_summary(&port(1), Some("HDMI Expansion Card")),
            "HDMI Expansion Card"
        );
    }

    #[test]
    fn a_power_role_the_partner_already_gave_is_left_unsaid() {
        assert_eq!(power_role_label(PortPartner::Sink, PowerRole::Source), None);
        assert_eq!(power_role_label(PortPartner::Source, PowerRole::Sink), None);
    }

    #[test]
    fn a_power_role_names_what_the_far_end_does_and_not_the_machine() {
        assert_eq!(
            power_role_label(PortPartner::Sink, PowerRole::Sink),
            Some("Supplying power")
        );
        assert_eq!(
            power_role_label(PortPartner::Audio, PowerRole::Source),
            Some("Drawing power")
        );
        assert_eq!(
            power_role_label(PortPartner::Sink, PowerRole::Unknown),
            Some("Unknown")
        );
    }

    #[test]
    fn a_data_role_names_what_the_far_end_is_and_not_the_machine() {
        assert_eq!(data_role_label(DataRole::DownstreamFacing), "Peripheral");
        assert_eq!(data_role_label(DataRole::UpstreamFacing), "Host");
        assert_eq!(data_role_label(DataRole::Disconnected), "Disconnected");
        assert_eq!(data_role_label(DataRole::Unknown), "Unknown");
    }

    #[test]
    fn only_the_port_powering_the_machine_says_so() {
        assert_eq!(powering_label(&port(0)), Some("Yes"));
        assert_eq!(powering_label(&port(1)), None);
    }

    #[test]
    fn only_a_port_in_display_port_mode_says_so() {
        let video = PortState {
            video: true,
            ..port(1)
        };
        assert_eq!(display_port_label(&video), Some("Connected"));
        assert_eq!(display_port_label(&port(1)), None);
    }

    #[test]
    fn a_cable_reads_in_the_unit_its_rate_is_sold_in() {
        assert_eq!(cable_speed_label(CableSpeed::Usb2), Some("USB 2.0"));
        assert_eq!(cable_speed_label(CableSpeed::Gen1), Some("10 Gbps"));
        assert_eq!(cable_speed_label(CableSpeed::Gen2), Some("20 Gbps"));
        assert_eq!(cable_speed_label(CableSpeed::Gen3), Some("40 Gbps"));
        assert_eq!(cable_speed_label(CableSpeed::Gen4), Some("80 Gbps"));
        assert_eq!(cable_speed_label(CableSpeed::Unknown), None);
    }

    #[test]
    fn a_cables_rating_reads_in_the_watts_it_is_sold_by() {
        let standard = Cable {
            marking: CableMarking::Marked,
            milliamps: 3000,
            ..Cable::default()
        };
        let five_amp = Cable {
            milliamps: 5000,
            ..standard
        };
        let extended = Cable {
            epr: true,
            ..five_amp
        };
        assert_eq!(cable_rating_label(&standard).as_deref(), Some("3 A (60 W)"));
        assert_eq!(
            cable_rating_label(&five_amp).as_deref(),
            Some("5 A (100 W)")
        );
        assert_eq!(
            cable_rating_label(&extended).as_deref(),
            Some("5 A (240 W)")
        );
    }

    #[test]
    fn a_current_the_e_marker_left_reserved_has_no_rating() {
        assert_eq!(cable_rating_label(&Cable::default()), None);
    }

    #[test]
    fn a_cables_length_is_worded_as_the_approximation_it_is() {
        assert_eq!(
            cable_length_label(CableLatency::Under30Ns),
            Some("About 3 m")
        );
        assert_eq!(cable_length_label(CableLatency::Over70Ns), Some("Over 7 m"));
        assert_eq!(cable_length_label(CableLatency::Unknown), None);
    }
}
