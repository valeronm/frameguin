//! The USB-C ports: the words a reader needs for one.

use frameguin_contract::{
    Cable, CableLatency, CableSpeed, DataRole, Epr, OffRefusal, PdContract, PeakCurrent, PortMask,
    PortPartner, PortReading, PortState, PowerRole, SupplyKind,
};
use frameguin_model::control::ports::{Flow, carries_power, flow, powering};
use frameguin_model::port::Placement;

use crate::port;
use crate::units::{Kind, amps, tenth_volts, volts, watts};
use crate::words::{NO_SUPPLY, yes_no};

/// An empty port, worded as the absence it is rather than as a kind of
/// partner.
const NOTHING_ATTACHED: &str = "Empty";

/// What an attached partner is to the machine's power, and what it is where
/// it takes no part in it.
fn role(reading: &PortReading) -> &'static str {
    if reading.charging {
        return POWERING;
    }
    match reading.partner {
        PortPartner::Nothing => NOTHING_ATTACHED,
        PortPartner::Sink => POWERED,
        PortPartner::Source => AVAILABLE,
        PortPartner::Debug => "Debug accessory",
        PortPartner::Audio => "Audio accessory",
        PortPartner::PoweredAccessory => "Powered accessory",
        PortPartner::Unsupported => "Unsupported",
        PortPartner::Invalid => "Not recognized",
    }
}

/// What the link carries as a person reads it — volts, amps and the watts
/// they come to. A port with no power delivery contract still carries what
/// Type-C's own resistors advertise. None where the port settled on
/// nothing, a supply of zero being worth no row.
#[must_use]
pub fn carried(port: &PortReading) -> Option<String> {
    if !carries_power(port) {
        return None;
    }
    Some(format!(
        "{}, {} ({})",
        volts(port.millivolts.into(), Kind::Static),
        amps(port.milliamps.into(), Kind::Static),
        power(port),
    ))
}

#[must_use]
pub fn measured_label(millivolts: u16) -> String {
    tenth_volts(millivolts.into())
}

/// The port the machine draws its power through, named as that rather than as
/// one more charger among those that may be attached.
const POWERING: &str = "Powering";

/// A charger the machine is not drawing from, which still holds its contract
/// because the EC draws through one port at a time.
const AVAILABLE: &str = "Available";

/// A partner drawing power from the machine.
const POWERED: &str = "Powered";

pub const DISPLAY_PORT: &str = "DisplayPort";

/// A port its controller has disabled, worded over what it last reported.
const TURNED_OFF: &str = "Turned off";

/// A port's lines beside the machine: the watts it carries and under them
/// what is attached, or the words alone where it carries none.
#[derive(Debug, PartialEq, Eq)]
pub struct PortLine {
    pub main: String,
    /// Where the port is, where no drawn socket shows it.
    pub name: Option<String>,
    pub lead: Option<&'static str>,
    /// Where the port is in `DisplayPort` alternate mode.
    pub display: bool,
}

impl PortLine {
    /// The line under the main one, empty where it has nothing to say.
    #[must_use]
    pub fn caption(&self) -> String {
        [self.name.as_deref(), self.lead]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// [`line`], naming the port in its caption unless `arrowed`: a socket drawn
/// with an arrow for its [`Flow`] shows where the port is.
#[must_use]
pub fn port_line(placement: Placement, port: &PortState, arrowed: bool) -> PortLine {
    PortLine {
        name: (!arrowed).then(|| port::inline(placement, port.index)),
        ..line(port, arrowed)
    }
}

/// What a screen reader says for a port: where it is, then its lines, the
/// way power goes worded on the watts since no arrow is heard.
#[must_use]
pub fn spoken(placement: Placement, port: &PortState) -> String {
    let line = port_line(placement, port, false);
    let label = port::label(placement, port.index);
    [
        Some(label.as_str()),
        Some(line.main.as_str()),
        line.display.then_some(DISPLAY_PORT),
        line.lead,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ")
}

/// Why a port's switch cannot turn it off.
#[must_use]
pub fn refusal_label(refusal: OffRefusal) -> &'static str {
    match refusal {
        OffRefusal::LastPort => "Can't be turned off to allow charging",
        OffRefusal::Unchecked => "Can't be turned off while a port can't be checked",
        OffRefusal::LastDisplay => "Can't be turned off while it drives the only display",
    }
}

/// What a port holds, which its socket and its line are both drawn from.
enum Holding<'a> {
    Off,
    Empty,
    Attached(&'a PortReading),
}

fn holding(port: &PortState) -> Holding<'_> {
    match &port.mask {
        PortMask::Off => Holding::Off,
        PortMask::On(reading, _) | PortMask::Unchecked(reading, _) => {
            if reading.partner == PortPartner::Nothing {
                Holding::Empty
            } else {
                Holding::Attached(reading)
            }
        }
    }
}

/// The watts a port carries and what the EC reports attached, named for
/// powering the machine where it does. The way worded beside the watts is
/// the attached thing's, not the machine's.
fn line(port: &PortState, arrowed: bool) -> PortLine {
    let bare = |main: &str| PortLine {
        main: main.to_owned(),
        name: None,
        lead: None,
        display: false,
    };
    let reading = match holding(port) {
        Holding::Off => return bare(TURNED_OFF),
        Holding::Empty => return bare(NOTHING_ATTACHED),
        Holding::Attached(reading) => reading,
    };
    let verbed = flow(reading).filter(|_| !arrowed);
    let role = role(reading);
    let (main, lead) = if carries_power(reading) {
        let watts = power(reading);
        let main = match verbed {
            Some(Flow::In) => format!("Supplying {watts}"),
            Some(Flow::Out) => format!("Drawing {watts}"),
            None => watts,
        };
        // Drawing on the watts already says what Powered would.
        (main, (verbed != Some(Flow::Out)).then_some(role))
    } else {
        (role.to_owned(), None)
    };
    PortLine {
        main,
        name: None,
        lead,
        display: reading.video,
    }
}

/// What a port's socket shows of it where the machine is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Socket {
    Off,
    /// Where [`port_line`]'s main line reads that nothing is attached.
    Empty,
    InUse(Option<Flow>),
}

#[must_use]
pub fn socket(port: &PortState) -> Socket {
    match holding(port) {
        Holding::Off => Socket::Off,
        Holding::Empty => Socket::Empty,
        Holding::Attached(reading) => Socket::InUse(flow(reading)),
    }
}

/// Where the machine is powered from, through the port at `index`.
#[must_use]
pub fn powered_from(placement: Placement, index: u8) -> String {
    format!("Powered from {}", port::place(placement, index))
}

fn power(port: &PortReading) -> String {
    watts(port.millivolts.into(), port.milliamps.into(), Kind::Static)
}

/// What the machine is being powered with, for a row that has one line for
/// it: the watts where a port is supplying them, and the absence named
/// where none is.
///
/// Named for the supply rather than the charger,
/// [`crate::battery::reading::charger_label`] being the word for whether the
/// EC sees one attached at all — two answers about a charger, and a shared
/// name would leave which one a row shows decided by its import. The word
/// for the absence they do share.
#[must_use]
pub fn supply_label(ports: &[PortState]) -> String {
    powering(ports).map_or_else(|| NO_SUPPLY.to_owned(), |(_, port)| power(port))
}

/// Where the power is coming in, to sit under [`supply_label`]. None where
/// nothing is supplying — a port named under "Not connected" would name the
/// one that stopped.
#[must_use]
pub fn supply_port(ports: &[PortState], placement: Placement) -> Option<String> {
    powering(ports).map(|(index, _)| port::label(placement, index))
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
    powering(ports).map_or(supply.clone(), |(index, _)| {
        format!("{supply} · {}", port::inline(placement, index))
    })
}

/// `DisplayPort` alternate mode, and None where the port is not in it.
#[must_use]
pub fn display_port_label(port: &PortReading) -> Option<&'static str> {
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
/// two ends of a link being opposites. None where the partner's kind already
/// gives it.
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
/// role being what the EC reports. None where the far end is a peripheral,
/// the usual case.
#[must_use]
pub fn data_role_label(role: DataRole) -> Option<&'static str> {
    match role {
        DataRole::UpstreamFacing => Some("Host"),
        DataRole::DownstreamFacing => None,
        DataRole::Disconnected => Some("Disconnected"),
        DataRole::Unknown => Some("Unknown"),
    }
}

/// None where this machine is not supplying VCONN, which leaves open whether
/// the far end is or nothing draws it.
#[must_use]
pub fn vconn_label(vconn: bool) -> Option<&'static str> {
    vconn.then_some("From this machine")
}

/// The kind of supply the offer is, and None for a fixed one, the kind
/// nearly every contract is.
#[must_use]
pub fn supply_kind_label(kind: SupplyKind) -> Option<&'static str> {
    match kind {
        // A reserved type code says nothing of the supply to word either.
        SupplyKind::Fixed | SupplyKind::Reserved => None,
        SupplyKind::Battery => Some("Battery"),
        SupplyKind::Variable => Some("Variable"),
        SupplyKind::Pps => Some("Programmable (PPS)"),
        SupplyKind::SprAvs => Some("Adjustable (AVS)"),
        SupplyKind::EprAvs => Some("Adjustable, extended range (AVS)"),
    }
}

/// None where the sink found an offer enough.
#[must_use]
pub fn mismatch_label(contract: &PdContract) -> Option<&'static str> {
    contract.capability_mismatch.then_some(yes_no(true))
}

/// How far past its rating the attached source lets a draw go for a moment,
/// and None where it is not the source or declares none past the rating.
#[must_use]
pub fn peak_label(contract: &PdContract, partner: PortPartner) -> Option<&'static str> {
    if partner != PortPartner::Source {
        return None;
    }
    Some(match contract.peak? {
        PeakCurrent::Overload150 => "Up to 150% briefly",
        PeakCurrent::Overload200 => "Up to 200% briefly",
        PeakCurrent::Overload200Sustained => "Up to 200% briefly, 150% for 10 ms",
    })
}

/// Whether the attached programmable source can give its full current
/// across its whole range, and None where it is no such source.
#[must_use]
pub fn power_limited_label(contract: &PdContract, partner: PortPartner) -> Option<&'static str> {
    if partner != PortPartner::Source {
        return None;
    }
    contract.power_limited.map(yes_no)
}

/// None for a passive cable, the kind nearly every cable is.
#[must_use]
pub fn cable_type_label(active: bool) -> Option<&'static str> {
    active.then_some("Active cable")
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

/// The cable as one line: its rate, its rating and its length, each left
/// out where the e-marker did not report it, and None where it reported none.
#[must_use]
pub fn cable_label(cable: &Cable) -> Option<String> {
    let parts: Vec<String> = [
        cable.speed.map(|speed| cable_speed_label(speed).to_owned()),
        cable_rating_label(*cable),
        cable
            .latency
            .map(|latency| cable_length_label(latency).to_owned()),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The rate a cable is certified for: per lane through USB 3.2, whose links
/// use one, and over both lanes for USB4, whose links use two.
#[must_use]
fn cable_speed_label(speed: CableSpeed) -> &'static str {
    match speed {
        CableSpeed::Usb2 => "480 Mbps",
        CableSpeed::Gen1 => "5 Gbps",
        CableSpeed::Gen2 => "10 Gbps",
        CableSpeed::Gen3 => "40 Gbps",
        CableSpeed::Gen4 => "80 Gbps",
    }
}

/// The watts a cable is sold by, and None where the e-marker left the
/// current at a reserved code.
///
/// The watts are what a contract can reach over the cable, 48 V under
/// extended power range and 20 V otherwise: a cable's voltage rating alone
/// does not let a charger enter extended power range, its EPR bit does.
#[must_use]
fn cable_rating_label(cable: Cable) -> Option<String> {
    let milliamps = u32::from(cable.milliamps?);
    let contract_millivolts = if cable.epr { 48_000 } else { 20_000 };
    Some(watts(contract_millivolts, milliamps, Kind::Static))
}

/// The length the PD specification ties to each latency class.
#[must_use]
fn cable_length_label(latency: CableLatency) -> &'static str {
    match latency {
        CableLatency::Under10Ns => "About 1 m",
        CableLatency::Under20Ns => "About 2 m",
        CableLatency::Under30Ns => "About 3 m",
        CableLatency::Under40Ns => "About 4 m",
        CableLatency::Under50Ns => "About 5 m",
        CableLatency::Under60Ns => "About 6 m",
        CableLatency::Under70Ns => "About 7 m",
        CableLatency::Over70Ns => "Over 7 m",
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{
        Cable, CableLatency, CableSpeed, DataRole, PdContract, PeakCurrent, Platform, PortMask,
        PortPartner, PortReading, PortState, PowerRole, SupplyKind,
    };
    use frameguin_model::port::Placement;

    use frameguin_model::control::ports::Flow;

    use super::{
        NOTHING_ATTACHED, PortLine, Socket, cable_label, cable_length_label, cable_rating_label,
        cable_speed_label, carried, data_role_label, display_port_label, line, measured_label,
        mismatch_label, peak_label, port_line, power_limited_label, power_role_label, powered_from,
        socket, spoken, supply_kind_label, supply_label, supply_summary,
    };
    use frameguin_model::fixtures::{port, reading};

    fn on(reading: PortReading) -> PortState {
        PortState {
            index: 0,
            mask: PortMask::On(reading, None),
        }
    }

    #[test]
    fn a_measured_voltage_reads_to_a_tenth_of_a_volt() {
        assert_eq!(measured_label(20_100), "20.1 V");
    }

    #[test]
    fn what_a_link_carries_reads_as_volts_amps_and_watts() {
        assert_eq!(carried(&reading(0)).as_deref(), Some("20 V, 5 A (100 W)"));
    }

    #[test]
    fn a_contract_short_of_a_whole_watt_keeps_its_tenth() {
        let usb = PortReading {
            millivolts: 5_000,
            milliamps: 1_500,
            ..reading(0)
        };
        assert_eq!(carried(&usb).as_deref(), Some("5 V, 1.5 A (7.5 W)"));
        assert_eq!(supply_label(&[on(usb)]), "7.5 W");
    }

    #[test]
    fn a_port_that_settled_on_nothing_carries_nothing_to_show() {
        assert_eq!(carried(&reading(1)), None);
    }

    #[test]
    fn the_charger_row_names_the_watts_of_the_port_supplying_them() {
        let ports: Vec<_> = (0..4).map(port).collect();
        assert_eq!(supply_label(&ports), "100 W");
    }

    #[test]
    fn nothing_supplying_reads_as_not_connected_rather_than_zero_watts() {
        let ports: Vec<_> = (1..4).map(port).collect();
        assert_eq!(supply_label(&ports), "Not connected");
        assert_eq!(supply_label(&[]), "Not connected");
    }

    #[test]
    fn the_joined_line_carries_the_supply_and_its_port() {
        let ports: Vec<_> = (0..4).map(port).collect();
        let summary = |platform| supply_summary(&ports, Placement::of(platform));
        assert_eq!(
            summary(frameguin_contract::Platform::Laptop13ProUltra3),
            "100 W · Right front"
        );
        assert_eq!(
            summary(frameguin_contract::Platform::Unknown),
            "100 W · Port 0"
        );
        assert_eq!(
            summary(frameguin_contract::Platform::Laptop13AmdAi300),
            "100 W · Right, port 0"
        );
    }

    #[test]
    fn a_line_with_nothing_supplying_names_no_port() {
        assert_eq!(supply_summary(&[], Placement::default()), "Not connected");
    }

    fn worded(main: &str, lead: Option<&'static str>) -> PortLine {
        PortLine {
            main: main.to_owned(),
            name: None,
            lead,
            display: false,
        }
    }

    #[test]
    fn a_charger_the_machine_is_not_drawing_from_is_available() {
        let second_charger = on(PortReading {
            charging: false,
            ..reading(0)
        });
        assert_eq!(
            line(&second_charger, true),
            worded("100 W", Some("Available"))
        );
        assert_eq!(socket(&second_charger), Socket::InUse(None));
    }

    #[test]
    fn a_port_the_ec_charges_from_powers_the_machine_before_its_watts_arrive() {
        let negotiating = on(PortReading {
            millivolts: 0,
            milliamps: 0,
            ..reading(0)
        });
        assert_eq!(socket(&negotiating), Socket::InUse(Some(Flow::In)));
        assert_eq!(line(&negotiating, true), worded("Powering", None));
    }

    #[test]
    fn a_port_is_spoken_by_place_then_its_lines() {
        let measured = Placement::of(Platform::Laptop13ProUltra3);
        assert_eq!(
            spoken(measured, &port(0)),
            "Right front, Supplying 100 W, Powering"
        );
        let monitor = PortState {
            index: 1,
            mask: PortMask::On(
                PortReading {
                    partner: PortPartner::Sink,
                    charging: false,
                    video: true,
                    millivolts: 5_000,
                    milliamps: 400,
                    ..reading(1)
                },
                None,
            ),
        };
        assert_eq!(
            spoken(measured, &monitor),
            "Right rear, Drawing 2 W, DisplayPort"
        );
    }

    #[test]
    fn a_port_driving_a_display_says_so_beside_its_words() {
        let monitor = on(PortReading {
            partner: PortPartner::Sink,
            charging: false,
            video: true,
            millivolts: 5_000,
            milliamps: 400,
            ..reading(0)
        });
        assert_eq!(
            line(&monitor, true),
            PortLine {
                display: true,
                ..worded("2 W", Some("Powered"))
            }
        );
        assert!(!line(&port(0), true).display);
    }

    #[test]
    fn a_partner_is_named_where_the_watts_carry_no_verb() {
        let sink = on(PortReading {
            partner: PortPartner::Sink,
            charging: false,
            millivolts: 5_000,
            milliamps: 1_500,
            ..reading(0)
        });
        assert_eq!(line(&sink, true), worded("7.5 W", Some("Powered")));
        assert_eq!(line(&sink, false), worded("Drawing 7.5 W", None));
        assert_eq!(line(&port(1), false), worded("Empty", None));
    }

    #[test]
    fn an_arrow_at_the_socket_leaves_the_watts_bare() {
        assert_eq!(line(&port(0), true), worded("100 W", Some("Powering")));
    }

    #[test]
    fn a_port_with_no_drawn_socket_is_named_in_its_caption() {
        let sided = Placement::of(Platform::Laptop13AmdAi300);
        let listed = port_line(sided, &port(0), false);
        assert_eq!(listed.caption(), "Right, port 0 · Powering");
        assert_eq!(listed.main, "Supplying 100 W");
        let empty = port_line(sided, &port(1), false);
        assert_eq!(empty.main, NOTHING_ATTACHED);
        assert_eq!(empty.caption(), "Right, port 1");
        let measured = Placement::of(Platform::Laptop13ProUltra3);
        assert_eq!(port_line(measured, &port(0), true).caption(), "Powering");
    }

    #[test]
    fn the_port_the_machine_draws_from_is_named_for_that_among_chargers() {
        assert_eq!(
            line(&port(0), false),
            worded("Supplying 100 W", Some("Powering"))
        );
    }

    #[test]
    fn a_partner_with_no_contract_is_named_without_watts() {
        let accessory = on(PortReading {
            partner: PortPartner::Audio,
            ..reading(1)
        });
        assert_eq!(line(&accessory, false), worded("Audio accessory", None));
    }

    #[test]
    fn a_port_turned_off_says_only_that() {
        let off = PortState {
            index: 0,
            mask: PortMask::Off,
        };
        assert_eq!(line(&off, false), worded("Turned off", None));
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
        assert_eq!(data_role_label(DataRole::DownstreamFacing), None);
        assert_eq!(data_role_label(DataRole::UpstreamFacing), Some("Host"));
        assert_eq!(
            data_role_label(DataRole::Disconnected),
            Some("Disconnected")
        );
        assert_eq!(data_role_label(DataRole::Unknown), Some("Unknown"));
    }

    #[test]
    fn only_a_port_in_display_port_mode_says_so() {
        let video = PortReading {
            video: true,
            ..reading(1)
        };
        assert_eq!(display_port_label(&video), Some("Connected"));
        assert_eq!(display_port_label(&reading(1)), None);
    }

    #[test]
    fn a_cable_reads_as_the_rate_it_is_certified_for() {
        assert_eq!(cable_speed_label(CableSpeed::Usb2), "480 Mbps");
        assert_eq!(cable_speed_label(CableSpeed::Gen1), "5 Gbps");
        assert_eq!(cable_speed_label(CableSpeed::Gen2), "10 Gbps");
        assert_eq!(cable_speed_label(CableSpeed::Gen3), "40 Gbps");
        assert_eq!(cable_speed_label(CableSpeed::Gen4), "80 Gbps");
    }

    #[test]
    fn a_cables_rating_reads_in_the_watts_it_is_sold_by() {
        let standard = marked(Some(3000));
        let five_amp = marked(Some(5000));
        let extended = Cable {
            epr: true,
            ..five_amp
        };
        assert_eq!(cable_rating_label(standard).as_deref(), Some("60 W"));
        assert_eq!(cable_rating_label(five_amp).as_deref(), Some("100 W"));
        assert_eq!(cable_rating_label(extended).as_deref(), Some("240 W"));
    }

    #[test]
    fn a_current_the_e_marker_left_reserved_has_no_rating() {
        assert_eq!(cable_rating_label(marked(None)), None);
    }

    #[test]
    fn a_cables_length_is_worded_as_the_approximation_it_is() {
        assert_eq!(cable_length_label(CableLatency::Under30Ns), "About 3 m");
        assert_eq!(cable_length_label(CableLatency::Over70Ns), "Over 7 m");
    }

    #[test]
    fn a_cable_reads_as_one_line_of_what_its_e_marker_reported() {
        let extended = Cable {
            epr: true,
            ..marked(Some(5000))
        };
        assert_eq!(
            cable_label(&extended).as_deref(),
            Some("480 Mbps · 240 W · About 1 m")
        );
        assert_eq!(
            cable_label(&marked(None)).as_deref(),
            Some("480 Mbps · About 1 m")
        );
        let reserved = Cable {
            speed: None,
            latency: None,
            ..marked(None)
        };
        assert_eq!(cable_label(&reserved), None);
    }

    fn marked(milliamps: Option<u16>) -> Cable {
        Cable {
            speed: Some(CableSpeed::Usb2),
            milliamps,
            epr: false,
            latency: Some(CableLatency::Under10Ns),
            active: false,
        }
    }

    fn fixed() -> PdContract {
        PdContract {
            kind: SupplyKind::Fixed,
            peak: None,
            power_limited: None,
            capability_mismatch: false,
        }
    }

    #[test]
    fn a_programmable_source_says_whether_it_is_power_limited() {
        let pps = PdContract {
            kind: SupplyKind::Pps,
            power_limited: Some(true),
            ..fixed()
        };
        assert_eq!(power_limited_label(&pps, PortPartner::Source), Some("Yes"));
        assert_eq!(power_limited_label(&pps, PortPartner::Sink), None);
        assert_eq!(supply_kind_label(pps.kind), Some("Programmable (PPS)"));
    }

    #[test]
    fn only_a_source_declaring_overload_has_a_peak_row() {
        assert_eq!(peak_label(&fixed(), PortPartner::Source), None);
        let overloading = PdContract {
            peak: Some(PeakCurrent::Overload150),
            ..fixed()
        };
        assert_eq!(
            peak_label(&overloading, PortPartner::Source),
            Some("Up to 150% briefly")
        );
        assert_eq!(peak_label(&overloading, PortPartner::Sink), None);
    }

    #[test]
    fn a_mismatch_is_named_only_where_flagged() {
        assert_eq!(mismatch_label(&fixed()), None);
        let short = PdContract {
            capability_mismatch: true,
            ..fixed()
        };
        assert_eq!(mismatch_label(&short), Some("Yes"));
    }

    #[test]
    fn a_socket_shows_what_its_port_holds() {
        let accessory = PortState {
            index: 2,
            mask: PortMask::On(
                PortReading {
                    partner: PortPartner::Audio,
                    ..reading(2)
                },
                None,
            ),
        };
        let off = PortState {
            index: 3,
            mask: PortMask::Off,
        };
        let ports = [port(0), port(1), accessory, off];
        assert_eq!(socket(&ports[0]), Socket::InUse(Some(Flow::In)));
        assert_eq!(socket(&ports[1]), Socket::Empty);
        assert_eq!(socket(&ports[2]), Socket::InUse(None));
        assert_eq!(socket(&ports[3]), Socket::Off);
    }

    #[test]
    fn the_ports_row_names_where_the_power_comes_in() {
        let summary = |platform| powered_from(Placement::of(platform), 0);
        assert_eq!(
            summary(Platform::Laptop13ProUltra3),
            "Powered from right front"
        );
        assert_eq!(
            summary(Platform::Laptop13AmdAi300),
            "Powered from port 0 on the right"
        );
    }
}
