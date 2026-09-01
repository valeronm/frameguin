//! The USB-C ports: one read, and the words a reader needs for it.

use std::rc::Rc;

use frameguin_wire::{
    CcPolarity, DataRole, DeviceResult as Result, Epr, PortPartner, PortState, PortsControl,
    PowerRole,
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

/// What is attached, in the words a reader would use for it. None for an
/// empty port, which the caller words as the absence it is rather than as a
/// kind of partner.
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

/// The negotiated supply as a person reads it — volts, amps and the watts
/// they come to. None where nothing was negotiated, so a row is left out
/// rather than showing a contract of zero.
#[must_use]
pub fn negotiated(port: &PortState) -> Option<String> {
    if port.millivolts == 0 || port.milliamps == 0 {
        return None;
    }
    Some(format!(
        "{:.1} V, {:.2} A ({:.0} W)",
        f64::from(port.millivolts) / 1000.0,
        f64::from(port.milliamps) / 1000.0,
        watts(port),
    ))
}

fn watts(port: &PortState) -> f64 {
    f64::from(port.millivolts) * f64::from(port.milliamps) / 1_000_000.0
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
/// shows decided by its import.
#[must_use]
pub fn supply_label(ports: &[PortState]) -> String {
    powering(ports).map_or_else(
        || "Disconnected".to_owned(),
        |port| format!("{:.0} W", watts(port)),
    )
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
    supply_port(ports, product).map_or(supply.clone(), |port| format!("{supply} · {port}"))
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

#[must_use]
pub fn vconn_label(vconn: bool) -> &'static str {
    if vconn { "On" } else { "Off" }
}

/// Which end of the link this machine is, said from the machine's side: a
/// reader wants to know what their laptop is doing, not what the standard
/// calls the role.
#[must_use]
pub fn power_role_label(role: PowerRole) -> &'static str {
    match role {
        PowerRole::Sink => "Taking power",
        PowerRole::Source => "Giving power",
        PowerRole::Unknown => "Unknown",
    }
}

#[must_use]
pub fn data_role_label(role: DataRole) -> &'static str {
    match role {
        DataRole::UpstreamFacing => "Device",
        DataRole::DownstreamFacing => "Host",
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

/// The plug's orientation. The debug variants are the same two channels
/// reached through a debug accessory, so they name the channel and say so.
#[must_use]
pub fn cc_label(cc: CcPolarity) -> &'static str {
    match cc {
        CcPolarity::Cc1 => "CC1",
        CcPolarity::Cc2 => "CC2",
        CcPolarity::Cc1Debug => "CC1 (debug)",
        CcPolarity::Cc2Debug => "CC2 (debug)",
        CcPolarity::Unknown => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{DeviceError, PortPartner};

    use super::{Ports, negotiated, partner_label, powering, supply_label, supply_summary};
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
    fn a_negotiated_contract_reads_as_volts_amps_and_watts() {
        assert_eq!(
            negotiated(&port(0)).as_deref(),
            Some("20.0 V, 5.00 A (100 W)")
        );
    }

    #[test]
    fn a_port_that_negotiated_nothing_has_no_contract_to_show() {
        assert_eq!(negotiated(&port(1)), None);
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
            supply_summary(&ports, "Laptop 13 Pro (Intel Core Ultra Series 3)"),
            "100 W · Right front"
        );
        assert_eq!(supply_summary(&ports, "Laptop 16"), "100 W · Port 0");
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
}
