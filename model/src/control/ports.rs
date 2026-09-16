//! The USB-C ports: one read, and the words a reader needs for it.

use std::rc::Rc;

use frameguin_wire::{
    DataRole, DeviceResult as Result, Epr, PortPartner, PortState, PortsControl, PowerRole,
};

use super::present;

pub struct Ports<C> {
    control: Rc<C>,
}

impl<C: PortsControl> Ports<C> {
    pub fn new(control: Rc<C>) -> Self {
        Self { control }
    }

    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        Ok(present(control.ports().await)?.map(|_| Self::new(control.clone())))
    }

    pub async fn read(&self) -> Result<Vec<PortState>> {
        self.control.ports().await
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
        "{:.1} V, {:.2} A ({})",
        f64::from(port.millivolts) / 1000.0,
        f64::from(port.milliamps) / 1000.0,
        watts(port),
    ))
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
pub fn supply_port(ports: &[PortState], product: &str) -> Option<String> {
    powering(ports).map(|port| crate::port::label(product, port.index))
}

/// The supply and where it comes in, joined for a caller with one line to
/// put both on. Just the supply where nothing is supplying, there being no
/// port to name then — an unmeasured board still joins, its port named by
/// number.
///
/// The words only; what the line is *about* is the caller's to say, as a row
/// title is everywhere else.
#[must_use]
pub fn supply_summary(ports: &[PortState], product: &str) -> String {
    let supply = supply_label(ports);
    powering(ports).map_or(supply.clone(), |port| {
        format!("{supply} · {}", crate::port::inline(product, port.index))
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

#[cfg(test)]
mod tests {
    use frameguin_wire::{DataRole, DeviceError, PortPartner, PortState, PowerRole};

    use super::{
        Ports, carried, data_role_label, display_port_label, partner_label, port_summary,
        power_role_label, powering, powering_label, supply_label, supply_summary,
    };
    use crate::testing::{Board, absent, port, ready};

    #[test]
    fn ports_the_hardware_answers_for_are_detected() {
        assert!(ready(Ports::detect(&Board::new())).unwrap().is_some());
    }

    #[test]
    fn a_board_the_hardware_serves_no_ports_for_is_absent() {
        let board = Board::failing(absent());
        assert!(ready(Ports::detect(&board)).unwrap().is_none());
    }

    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_set_of_ports() {
        let error = DeviceError::Failed("no reply".into());
        let board = Board::failing(error.clone());
        assert_eq!(ready(Ports::detect(&board)).err(), Some(error));
    }

    #[test]
    fn a_read_carries_every_port() {
        let ports = Ports::new(Board::new());
        let read = ready(ports.read()).unwrap();
        assert_eq!(read.len(), 4);
        assert!(read[0].charging);
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
        assert_eq!(
            supply_summary(&ports, frameguin_wire::BOARD_LAPTOP13_PRO_ULTRA_3),
            "100 W · Right front"
        );
        assert_eq!(supply_summary(&ports, "Laptop 16"), "100 W · Port 0");
        assert_eq!(
            supply_summary(&ports, frameguin_wire::BOARD_LAPTOP13_AMD_AI_300),
            "100 W · Right, port 0"
        );
    }

    /// Nothing supplying means no port to name, and the separator would
    /// otherwise dangle.
    #[test]
    fn a_line_with_nothing_supplying_names_no_port() {
        assert_eq!(supply_summary(&[], "Laptop 16"), "Disconnected");
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
}
