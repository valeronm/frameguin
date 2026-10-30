//! What a PD controller reads of the state, the cable, the contract and the
//! bus on one of its ports, and where each board's controllers sit on the EC's I2C
//! buses — apart from [`crate::ec`] so the decoding is testable without an
//! EC.
//!
//! The registers are Infineon's host-interface map, shared across the
//! `CCGx` controllers on every Framework board.

use frameguin_contract::{
    Cable, CableLatency, CableSpeed, CcPolarity, DataRole, Epr, PdContract, PeakCurrent, Platform,
    PortPartner, PortReading, PortRegisters, PowerRole, SupplyKind,
};

use crate::bay;
use crate::ec::PdPorts;

pub(crate) const PD_STATUS: u16 = 0x08;
/// The partner in bits 2 to 4, the CC polarity in [`CC2`], and in bits 6 and
/// 7 the current a Type-C source advertises.
const TYPE_C_STATUS: u16 = 0x0C;
/// One bit per connector, set where it is enabled.
pub(crate) const PDPORT_ENABLE: u16 = 0x2C;
/// The controller's ADC reading of VBUS, 16 bits in 100 mV units.
const BUS_VOLTAGE: u16 = 0x0D;
/// The source's PDO the current contract stands on.
const CURRENT_PDO: u16 = 0x10;
/// The sink's RDO against [`CURRENT_PDO`].
const CURRENT_RDO: u16 = 0x14;
/// The Cable VDO exactly as the cable's e-marker sent it.
const CABLE_VDO: u16 = 0x18;
/// `PD_STATUS` through `CABLE_VDO`, which one passthrough read returns whole;
/// the registers between them are plain data.
pub(crate) const SPAN: usize = (CABLE_VDO + 4 - PD_STATUS) as usize;

/// Type-C's own voltage, which a port carries without a contract.
const TYPE_C_MILLIVOLTS: u16 = 5000;

/// Bit 6 of `PD_STATUS`'s first byte, set where this machine is the
/// downstream-facing end.
const DATA_ROLE_DFP: u8 = 1 << 6;
/// Bit 0 of `PD_STATUS`'s second byte, set where this machine is the source.
const POWER_ROLE_SOURCE: u8 = 1;
/// Bit 5 of `PD_STATUS`'s second byte.
const VCONN_SOURCE: u8 = 1 << 5;
/// Bit 7 of `PD_STATUS`'s third byte.
const EPR_ACTIVE: u8 = 1 << 7;
/// Bit 1.
const CC2: u8 = 1 << 1;

/// Bit 3 of `PD_STATUS`'s second byte, which the EC's own console prints as
/// `EMCA`.
const E_MARKER: u8 = 1 << 3;
/// Bit 2 of `PD_STATUS`'s second byte, which the EC reads as a contract
/// standing.
const CONTRACT: u8 = 1 << 2;
/// Bit 6 of `PD_STATUS`'s third byte, which the EC's own console prints as
/// an `Active` cable.
const ACTIVE_CABLE: u8 = 1 << 6;

/// The EC numbers a port as its controller times this plus which of the
/// controller's connectors it is.
pub(crate) const PORTS_PER_CONTROLLER: u8 = 2;

/// The controller's own `c_state` code, which the EC copies unchanged.
pub(crate) fn partner(c_state: u8) -> PortPartner {
    match c_state {
        0 => PortPartner::Nothing,
        1 => PortPartner::Sink,
        2 => PortPartner::Source,
        3 => PortPartner::Debug,
        4 => PortPartner::Audio,
        5 => PortPartner::PoweredAccessory,
        6 => PortPartner::Unsupported,
        _ => PortPartner::Invalid,
    }
}

/// Where `port`'s controller sits in the EC's controller order.
pub(crate) fn controller_of(port: u8) -> usize {
    usize::from(port / PORTS_PER_CONTROLLER)
}

/// `port`'s bit in its controller's [`PDPORT_ENABLE`] mask.
pub(crate) fn enable_bit(port: u8) -> u8 {
    1 << (port % PORTS_PER_CONTROLLER)
}

/// Whether `mask`, read from `port`'s controller, has `port` enabled.
pub(crate) fn enabled(mask: u8, port: u8) -> bool {
    mask & enable_bit(port) != 0
}

/// A connector's register block: `0x1000` for a controller's first
/// connector and `0x2000` for its second.
pub(crate) fn block(port: u8) -> u16 {
    (u16::from(port % PORTS_PER_CONTROLLER) + 1) * 0x1000
}

/// Each mainboard controller's (EC I2C port, 7-bit address), in the EC's
/// controller order, and empty for a board this table does not cover.
///
/// Copied from `framework_lib`'s `PdPort::i2c_port` and `i2c_address`, which
/// its `PdController` keeps private for reads. The Laptop 13 Pro's pair was
/// confirmed by reading each controller's version register.
pub(crate) fn controllers(platform: Platform) -> &'static [(u8, u16)] {
    match platform {
        Platform::Laptop13Gen11 => &[(6, 0x08), (6, 0x40)],
        Platform::Laptop13Gen12 | Platform::Laptop13Gen13 => &[(6, 0x08), (7, 0x40)],
        Platform::Laptop13Ultra1 | Platform::Laptop12Gen13 | Platform::Laptop12Core3 => {
            &[(1, 0x08), (2, 0x40)]
        }
        Platform::Laptop13Amd7040
        | Platform::Laptop13AmdAi300
        | Platform::Laptop13ProUltra3
        | Platform::Laptop16Amd7040
        | Platform::Laptop16AmdAi300 => &[(1, 0x42), (2, 0x40)],
        Platform::DesktopAmdAiMax300 => &[(1, 0x08)],
        Platform::Unknown => &[],
    }
}

/// What a board's expansion bay adds to its controllers.
pub(crate) enum Bay {
    /// The board has no bay.
    None,
    /// The module declares no CCG8S, and the EC fills a bay port's entry only
    /// from one.
    Silent,
    Controller((u8, u16)),
}

/// The bay's PD controller sits on the module's own EEPROM bus, at the
/// address the module declares.
pub(crate) fn bay(platform: Platform, ec: &dyn PdPorts) -> Bay {
    const BUS: u8 = 5;
    if !matches!(
        platform,
        Platform::Laptop16Amd7040 | Platform::Laptop16AmdAi300
    ) {
        return Bay::None;
    }
    match bay::ccg8s(ec, BUS) {
        Some(address) => Bay::Controller((BUS, u16::from(address))),
        None => Bay::Silent,
    }
}

/// What one read of [`SPAN`] bytes at `PD_STATUS` says of a port: its state
/// as the controller holds it now, and its cable, contract and bus.
pub struct ControllerRead {
    pub partner: PortPartner,
    pub contract: bool,
    pub power_role: PowerRole,
    pub data_role: DataRole,
    pub millivolts: u16,
    pub milliamps: u16,
    pub vconn: bool,
    pub cc: CcPolarity,
    pub epr_active: bool,
    pub registers: PortRegisters,
}

impl ControllerRead {
    /// The port's reading, the EC's `entry` answering for which port it
    /// charges from, the alternate mode and whether a source offers EPR: the
    /// registers carry none of them.
    pub(crate) fn over(self, entry: &PortReading) -> PortReading {
        PortReading {
            partner: self.partner,
            contract: self.contract,
            power_role: self.power_role,
            data_role: self.data_role,
            millivolts: self.millivolts,
            milliamps: self.milliamps,
            charging: entry.charging,
            video: entry.video,
            vconn: self.vconn,
            cc: self.cc,
            epr: if self.epr_active {
                Epr::Active
            } else if entry.epr == Epr::Unsupported {
                Epr::Unsupported
            } else {
                Epr::Supported
            },
            registers: (self.partner != PortPartner::Nothing).then_some(self.registers),
        }
    }
}

/// The state as `cypd_update_port_state` derives the EC's entry from these
/// registers, a programmable contract apart: the EC's entry holds the top of
/// its offer.
pub(crate) fn read(span: &[u8; SPAN]) -> ControllerRead {
    let typec = span[usize::from(TYPE_C_STATUS - PD_STATUS)];
    let contract = span[1] & CONTRACT != 0;
    let sourcing = span[1] & POWER_ROLE_SOURCE != 0;
    let partner = partner((typec >> 2) & 0b111);
    let advertised_ma = match typec >> 6 {
        0 => 900,
        1 => 1500,
        2 => 3000,
        _ => 0,
    };
    let (mut millivolts, mut milliamps) = match partner {
        PortPartner::Sink | PortPartner::Source => (TYPE_C_MILLIVOLTS, advertised_ma),
        _ => (0, 0),
    };
    if contract {
        (millivolts, milliamps) = if sourcing {
            (TYPE_C_MILLIVOLTS, rdo_operating_ma(word(span, CURRENT_RDO)))
        } else {
            drawn(word(span, CURRENT_PDO), word(span, CURRENT_RDO))
        };
    }
    ControllerRead {
        partner,
        contract,
        power_role: if sourcing {
            PowerRole::Source
        } else {
            PowerRole::Sink
        },
        data_role: if span[0] & DATA_ROLE_DFP != 0 {
            DataRole::DownstreamFacing
        } else {
            DataRole::UpstreamFacing
        },
        millivolts,
        milliamps,
        vconn: span[1] & VCONN_SOURCE != 0,
        cc: if typec & CC2 != 0 {
            CcPolarity::Cc2
        } else {
            CcPolarity::Cc1
        },
        epr_active: span[2] & EPR_ACTIVE != 0,
        registers: decode(span),
    }
}

/// The voltage and the current of a contract the machine draws. A
/// programmable one's are the sink's request, by the PD specification's PPS
/// and AVS request data objects; any other's are the offer's as the EC takes
/// them, a battery supply's power over its highest voltage and the top of a
/// variable range.
fn drawn(pdo: u32, rdo: u32) -> (u16, u16) {
    let (millivolts, milliamps) = match supply(pdo).0 {
        SupplyKind::Fixed => (field(pdo, 10, 10) * 50, field(pdo, 0, 10) * 10),
        SupplyKind::Battery => {
            let top_millivolts = field(pdo, 20, 10) * 50;
            let rated_milliwatts = field(pdo, 0, 10) * 250;
            (
                field(pdo, 10, 10) * 50,
                (rated_milliwatts * 1000)
                    .checked_div(top_millivolts)
                    .unwrap_or(0),
            )
        }
        SupplyKind::Variable => (field(pdo, 20, 10) * 50, field(pdo, 0, 10) * 10),
        SupplyKind::Pps => (field(rdo, 9, 12) * 20, field(rdo, 0, 7) * 50),
        SupplyKind::SprAvs | SupplyKind::EprAvs => (field(rdo, 9, 12) * 25, field(rdo, 0, 7) * 50),
        SupplyKind::Reserved => (field(pdo, 17, 8) * 100, field(pdo, 0, 7) * 50),
    };
    (
        u16::try_from(millivolts).unwrap_or(u16::MAX),
        u16::try_from(milliamps).unwrap_or(u16::MAX),
    )
}

/// The operating current a sink requested, in 10 mA units at bits 10 to 19.
fn rdo_operating_ma(rdo: u32) -> u16 {
    u16::try_from(field(rdo, 10, 10) * 10).unwrap_or(u16::MAX)
}

fn word(span: &[u8; SPAN], register: u16) -> u32 {
    let at = usize::from(register - PD_STATUS);
    u32::from_le_bytes(span[at..at + 4].try_into().unwrap_or_default())
}

/// A port's cable, contract and bus from one read of [`SPAN`] bytes at
/// `PD_STATUS`.
fn decode(span: &[u8; SPAN]) -> PortRegisters {
    let voltage = usize::from(BUS_VOLTAGE - PD_STATUS);
    let measured = u16::from_le_bytes(span[voltage..voltage + 2].try_into().unwrap_or_default());
    let pdo = word(span, CURRENT_PDO);
    PortRegisters {
        cable: cable([span[1], span[2]], word(span, CABLE_VDO)),
        pd: (span[1] & CONTRACT != 0 && pdo != 0).then(|| contract(pdo, word(span, CURRENT_RDO))),
        measured_millivolts: measured.saturating_mul(100),
    }
}

fn field(word: u32, shift: u32, width: u32) -> u32 {
    (word >> shift) & ((1 << width) - 1)
}

fn bit(word: u32, at: u32) -> bool {
    (word >> at) & 1 != 0
}

/// None for the code that declares no draw past the rating.
fn peak(code: u32) -> Option<PeakCurrent> {
    match code {
        1 => Some(PeakCurrent::Overload150),
        2 => Some(PeakCurrent::Overload200),
        3 => Some(PeakCurrent::Overload200Sustained),
        _ => None,
    }
}

/// The layouts are the USB PD specification's source PDO and request data
/// object; bit 26 of every RDO flags a capability mismatch.
fn contract(pdo: u32, rdo: u32) -> PdContract {
    let (kind, peak_code, power_limited) = supply(pdo);
    PdContract {
        kind,
        peak: peak_code.and_then(peak),
        power_limited,
        capability_mismatch: bit(rdo, 26),
    }
}

/// A PDO's kind by its type bits, with its peak current code and its power
/// limited bit where the kind has them.
fn supply(pdo: u32) -> (SupplyKind, Option<u32>, Option<bool>) {
    match pdo >> 30 {
        0 => (SupplyKind::Fixed, Some(field(pdo, 20, 2)), None),
        1 => (SupplyKind::Battery, None, None),
        2 => (SupplyKind::Variable, None, None),
        _ => match field(pdo, 28, 2) {
            0 => (SupplyKind::Pps, None, Some(bit(pdo, 27))),
            1 => (SupplyKind::EprAvs, Some(field(pdo, 26, 2)), None),
            2 => (SupplyKind::SprAvs, None, None),
            _ => (SupplyKind::Reserved, None, None),
        },
    }
}

/// None where the e-marker bit is clear or the controller has stored no VDO:
/// a zero VDO's current code would be the reserved `00`.
fn cable(status: [u8; 2], vdo: u32) -> Option<Cable> {
    if status[0] & E_MARKER == 0 || vdo == 0 {
        return None;
    }
    Some(Cable {
        speed: match field(vdo, 0, 3) {
            0 => Some(CableSpeed::Usb2),
            1 => Some(CableSpeed::Gen1),
            2 => Some(CableSpeed::Gen2),
            3 => Some(CableSpeed::Gen3),
            4 => Some(CableSpeed::Gen4),
            _ => None,
        },
        milliamps: match field(vdo, 5, 2) {
            1 => Some(3000),
            2 => Some(5000),
            _ => None,
        },
        epr: bit(vdo, 17),
        latency: match field(vdo, 13, 4) {
            1 => Some(CableLatency::Under10Ns),
            2 => Some(CableLatency::Under20Ns),
            3 => Some(CableLatency::Under30Ns),
            4 => Some(CableLatency::Under40Ns),
            5 => Some(CableLatency::Under50Ns),
            6 => Some(CableLatency::Under60Ns),
            7 => Some(CableLatency::Under70Ns),
            8 => Some(CableLatency::Over70Ns),
            _ => None,
        },
        active: status[1] & ACTIVE_CABLE != 0,
    })
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{
        CableLatency, CableSpeed, PdContract, PeakCurrent, Platform, SupplyKind,
    };

    use frameguin_contract::{CcPolarity, DataRole, PortPartner, PowerRole};

    use super::{SPAN, block, cable, controllers, decode, read};

    const CHARGING_CABLE_STATUS: [u8; 2] = [0x9c, 0x25];
    const CHARGING_CABLE_VDO: u32 = 0x000a_6640;
    const NO_E_MARKER_STATUS: [u8; 2] = [0x40, 0x25];

    #[test]
    fn a_measured_cable_vdo_decodes_to_its_rating() {
        let cable = cable(CHARGING_CABLE_STATUS, CHARGING_CABLE_VDO).expect("marked");
        assert_eq!(cable.speed, Some(CableSpeed::Usb2));
        assert_eq!(cable.milliamps, Some(5000));
        assert!(cable.epr);
        assert_eq!(cable.latency, Some(CableLatency::Under30Ns));
    }

    #[test]
    fn a_clear_e_marker_bit_reads_no_cable() {
        assert_eq!(cable(NO_E_MARKER_STATUS, CHARGING_CABLE_VDO), None);
    }

    #[test]
    fn an_e_marker_with_no_vdo_yet_reads_no_cable() {
        assert_eq!(cable(CHARGING_CABLE_STATUS, 0), None);
    }

    #[test]
    fn reserved_codes_leave_their_fields_empty() {
        const RESERVED_SPEED: u32 = 0b111;
        const RESERVED_CURRENT: u32 = 0b11 << 5;
        let cable =
            cable(CHARGING_CABLE_STATUS, RESERVED_SPEED | RESERVED_CURRENT).expect("marked");
        assert_eq!(cable.speed, None);
        assert_eq!(cable.milliamps, None);
        assert_eq!(cable.latency, None);
    }

    #[test]
    fn one_read_across_both_registers_decodes_as_the_two_did() {
        let span = [
            0x76, 0x9c, 0x25, 0x01, 0x89, 0xc9, 0x00, 0x00, 0xf4, 0x41, 0x06, 0x00, 0xf4, 0xd1,
            0xc7, 0x42, 0x40, 0x66, 0x0a, 0x00,
        ];
        let registers = decode(&span);
        assert_eq!(
            registers.cable,
            cable(CHARGING_CABLE_STATUS, CHARGING_CABLE_VDO)
        );
        assert_eq!(registers.measured_millivolts, 20_100);
    }

    const CHARGER_SPAN: [u8; 20] = [
        0x76, 0x8c, 0xa5, 0x01, 0x89, 0x14, 0x01, 0x00, 0xf4, 0xc1, 0x88, 0x00, 0xf4, 0xd1, 0xc7,
        0x82, 0x40, 0x46, 0x0a, 0x00,
    ];
    const CARD_SPAN: [u8; 20] = [
        0x76, 0xb5, 0x0d, 0x01, 0x67, 0x31, 0x00, 0x00, 0x96, 0x90, 0x01, 0x27, 0x44, 0x10, 0x81,
        0x12, 0x00, 0x00, 0x00, 0x00,
    ];
    const TYPE_C_ONLY_SPAN: [u8; 20] = [
        0x76, 0x59, 0x25, 0x01, 0x67, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x43, 0x26, 0x0a, 0x45,
    ];

    fn pd(span: &[u8; SPAN]) -> Option<PdContract> {
        decode(span).pd
    }

    #[test]
    fn a_contract_decodes_its_kind_from_the_offer() {
        let charger = pd(&CHARGER_SPAN).expect("a contract stands");
        assert_eq!(charger.kind, SupplyKind::Fixed);
        assert_eq!(charger.peak, None);
        assert_eq!(charger.power_limited, None);
        assert!(!charger.capability_mismatch);
        assert_eq!(pd(&CARD_SPAN).map(|pd| pd.kind), Some(SupplyKind::Fixed));
    }

    #[test]
    fn a_port_without_a_contract_has_none_read() {
        assert_eq!(pd(&TYPE_C_ONLY_SPAN), None);
    }

    #[test]
    fn a_marked_cables_active_bit_is_carried() {
        let mut span = TYPE_C_ONLY_SPAN;
        assert!(!decode(&span).cable.expect("marked").active);
        span[2] |= super::ACTIVE_CABLE;
        assert!(decode(&span).cable.expect("marked").active);
    }

    #[test]
    fn a_programmable_offer_carries_its_power_limit() {
        let pdo = (0b11 << 30) | (1 << 27);
        let pd = super::contract(pdo, 1 << 26);
        assert_eq!(pd.kind, SupplyKind::Pps);
        assert_eq!(pd.power_limited, Some(true));
        assert_eq!(pd.peak, None);
        assert!(pd.capability_mismatch);
    }

    #[test]
    fn a_fixed_offer_carries_its_peak_current() {
        let pd = super::contract(0b10 << 20, 0);
        assert_eq!(pd.kind, SupplyKind::Fixed);
        assert_eq!(pd.peak, Some(PeakCurrent::Overload200));
    }

    #[test]
    fn a_controllers_second_connector_is_its_second_block() {
        assert_eq!(block(0), 0x1000);
        assert_eq!(block(1), 0x2000);
        assert_eq!(block(2), 0x1000);
        assert_eq!(block(4), 0x1000);
    }

    #[test]
    fn the_laptop_13_pros_controllers_are_where_they_were_read() {
        assert_eq!(
            controllers(Platform::Laptop13ProUltra3),
            &[(1, 0x42), (2, 0x40)]
        );
    }

    #[test]
    fn a_board_nobody_addressed_has_no_controllers() {
        assert!(controllers(Platform::Unknown).is_empty());
    }

    #[test]
    fn a_charger_under_an_epr_contract_reads_as_the_ec_reported_it() {
        let read = read(&CHARGER_SPAN);
        assert_eq!(read.partner, PortPartner::Source);
        assert!(read.contract);
        assert_eq!(read.power_role, PowerRole::Sink);
        assert_eq!(read.data_role, DataRole::DownstreamFacing);
        assert_eq!((read.millivolts, read.milliamps), (28_000, 5000));
        assert_eq!(read.cc, CcPolarity::Cc1);
        assert!(!read.vconn);
        assert!(read.epr_active);
    }

    #[test]
    fn a_card_this_machine_powers_reads_its_requested_current() {
        let read = read(&CARD_SPAN);
        assert_eq!(read.partner, PortPartner::Sink);
        assert_eq!(read.power_role, PowerRole::Source);
        assert_eq!((read.millivolts, read.milliamps), (5000, 680));
        assert!(read.vconn);
    }

    #[test]
    fn a_device_without_a_contract_carries_the_advertised_current() {
        let read = read(&TYPE_C_ONLY_SPAN);
        assert!(!read.contract);
        assert_eq!((read.millivolts, read.milliamps), (5000, 1500));
    }

    fn charging_on(pdo: u32, rdo: u32) -> [u8; SPAN] {
        let mut span = CHARGER_SPAN;
        let at = |register: u16| usize::from(register - super::PD_STATUS);
        span[at(super::CURRENT_PDO)..at(super::CURRENT_PDO) + 4]
            .copy_from_slice(&pdo.to_le_bytes());
        span[at(super::CURRENT_RDO)..at(super::CURRENT_RDO) + 4]
            .copy_from_slice(&rdo.to_le_bytes());
        span
    }

    #[test]
    fn a_programmable_contract_carries_the_voltage_and_current_requested() {
        let pdo = (0b11 << 30) | ((21_000 / 100) << 17) | ((3_300 / 100) << 8) | (3_000 / 50);
        let rdo = (4 << 28) | ((9_000 / 20) << 9) | (2_000 / 50);
        let read = read(&charging_on(pdo, rdo));
        assert_eq!((read.millivolts, read.milliamps), (9_000, 2_000));
    }

    #[test]
    fn an_adjustable_contract_carries_the_voltage_and_current_requested() {
        let rdo = (4 << 28) | ((15_000 / 25) << 9) | (2_500 / 50);
        let standard = read(&charging_on((0b11 << 30) | (0b10 << 28), rdo));
        assert_eq!((standard.millivolts, standard.milliamps), (15_000, 2_500));
        let rdo = (4 << 28) | ((28_000 / 25) << 9) | (5_000 / 50);
        let extended = read(&charging_on((0b11 << 30) | (0b01 << 28), rdo));
        assert_eq!((extended.millivolts, extended.milliamps), (28_000, 5_000));
    }

    #[test]
    fn a_port_with_nothing_attached_carries_nothing() {
        let mut span = CHARGER_SPAN;
        span[4] = 0;
        span[1] &= !super::CONTRACT;
        let read = read(&span);
        assert_eq!(read.partner, PortPartner::Nothing);
        assert_eq!((read.millivolts, read.milliamps), (0, 0));
    }
}
