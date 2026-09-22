//! What a PD controller reads of the cable, the contract and the bus on one
//! of its ports, and where each board's controllers sit on the EC's I2C
//! buses — apart from [`crate::ec`] so the decoding is testable without an
//! EC.
//!
//! The registers are Infineon's host-interface map, shared across the
//! `CCGx` controllers on every Framework board.

use frameguin_wire::{
    Cable, CableLatency, CableMarking, CableSpeed, PdContract, PeakCurrent, Platform,
    PortRegisters, SupplyKind,
};

pub(crate) const PD_STATUS: u16 = 0x08;
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
pub(crate) const SPAN: u16 = CABLE_VDO + 4 - PD_STATUS;

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

/// A connector's register block: `0x1000` for a controller's first
/// connector and `0x2000` for its second.
pub(crate) fn block(port: u8) -> u16 {
    (u16::from(port % PORTS_PER_CONTROLLER) + 1) * 0x1000
}

/// Each controller's (EC I2C port, 7-bit address), in the EC's controller
/// order, and empty for a board this table does not cover.
///
/// Copied from `framework_lib`'s `PdPort::i2c_port` and `i2c_address` as of
/// framework-system commit `6439eb0`, newer than the pinned 0.6.5 release by
/// the Laptop 12 (Intel Core Series 3) entry; its `PdController` keeps them
/// private for reads. The Laptop 13 Pro's pair was confirmed by reading each
/// controller's version register.
pub(crate) fn controllers(platform: Platform) -> &'static [(u8, u16)] {
    match platform {
        Platform::Laptop13Gen11 => &[(6, 0x08), (6, 0x40)],
        Platform::Laptop13Gen12 | Platform::Laptop13Gen13 => &[(6, 0x08), (7, 0x40)],
        Platform::Laptop13Ultra1 | Platform::Laptop12Gen13 | Platform::Laptop12Core3 => {
            &[(1, 0x08), (2, 0x40)]
        }
        Platform::Laptop13Amd7040 | Platform::Laptop13AmdAi300 | Platform::Laptop13ProUltra3 => {
            &[(1, 0x42), (2, 0x40)]
        }
        Platform::Laptop16Amd7040 | Platform::Laptop16AmdAi300 => {
            &[(1, 0x42), (2, 0x40), (5, 0x42)]
        }
        Platform::DesktopAmdAiMax300 => &[(1, 0x08)],
        Platform::Unknown => &[],
    }
}

/// A port's readings from one read of [`SPAN`] bytes at `PD_STATUS`.
pub(crate) fn decode(span: &[u8]) -> PortRegisters {
    let word = |register: u16| {
        let at = usize::from(register - PD_STATUS);
        span.get(at..at + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map_or(0, u32::from_le_bytes)
    };
    let pdo = word(CURRENT_PDO);
    let pd = (span.get(1).is_some_and(|flags| flags & CONTRACT != 0) && pdo != 0)
        .then(|| contract(pdo, word(CURRENT_RDO)));
    let voltage = usize::from(BUS_VOLTAGE - PD_STATUS);
    let measured_millivolts = span
        .get(voltage..voltage + 2)
        .and_then(|bytes| bytes.try_into().ok())
        .map(|bytes| u16::from_le_bytes(bytes).saturating_mul(100));
    PortRegisters {
        cable: from_registers(span, word(CABLE_VDO)),
        pd,
        measured_millivolts,
    }
}

fn field(word: u32, shift: u32, width: u32) -> u32 {
    (word >> shift) & ((1 << width) - 1)
}

fn bit(word: u32, at: u32) -> bool {
    (word >> at) & 1 != 0
}

fn peak(code: u32) -> PeakCurrent {
    match code {
        1 => PeakCurrent::Overload150,
        2 => PeakCurrent::Overload200,
        3 => PeakCurrent::Overload200Sustained,
        _ => PeakCurrent::Rated,
    }
}

/// The layouts are the USB PD specification's source PDO and request data
/// object; bit 26 of every RDO flags a capability mismatch.
fn contract(pdo: u32, rdo: u32) -> PdContract {
    let mismatched = PdContract {
        capability_mismatch: bit(rdo, 26),
        ..PdContract::default()
    };
    match pdo >> 30 {
        0 => PdContract {
            kind: SupplyKind::Fixed,
            peak: peak(field(pdo, 20, 2)),
            ..mismatched
        },
        1 => PdContract {
            kind: SupplyKind::Battery,
            ..mismatched
        },
        2 => PdContract {
            kind: SupplyKind::Variable,
            ..mismatched
        },
        _ => match field(pdo, 28, 2) {
            0 => PdContract {
                kind: SupplyKind::Pps,
                power_limited: bit(pdo, 27),
                ..mismatched
            },
            1 => PdContract {
                kind: SupplyKind::EprAvs,
                peak: peak(field(pdo, 26, 2)),
                ..mismatched
            },
            2 => PdContract {
                kind: SupplyKind::SprAvs,
                ..mismatched
            },
            _ => mismatched,
        },
    }
}

/// A zero VDO under a present e-marker is unknown rather than marked: its
/// current code would be the reserved `00`, so the controller has not
/// stored one.
fn from_registers(status: &[u8], vdo: u32) -> Cable {
    let Some(flags) = status.get(1) else {
        return Cable::default();
    };
    if flags & E_MARKER == 0 {
        return Cable {
            marking: CableMarking::Unmarked,
            ..Cable::default()
        };
    }
    if vdo == 0 {
        return Cable::default();
    }
    Cable {
        marking: CableMarking::Marked,
        speed: match field(vdo, 0, 3) {
            0 => CableSpeed::Usb2,
            1 => CableSpeed::Gen1,
            2 => CableSpeed::Gen2,
            3 => CableSpeed::Gen3,
            4 => CableSpeed::Gen4,
            _ => CableSpeed::Unknown,
        },
        milliamps: match field(vdo, 5, 2) {
            1 => 3000,
            2 => 5000,
            _ => 0,
        },
        max_millivolts: match field(vdo, 9, 2) {
            0 => 20_000,
            1 => 30_000,
            2 => 40_000,
            _ => 50_000,
        },
        epr: bit(vdo, 17),
        latency: match field(vdo, 13, 4) {
            1 => CableLatency::Under10Ns,
            2 => CableLatency::Under20Ns,
            3 => CableLatency::Under30Ns,
            4 => CableLatency::Under40Ns,
            5 => CableLatency::Under50Ns,
            6 => CableLatency::Under60Ns,
            7 => CableLatency::Under70Ns,
            8 => CableLatency::Over70Ns,
            _ => CableLatency::Unknown,
        },
        active: status.get(2).is_some_and(|flags| flags & ACTIVE_CABLE != 0),
    }
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{
        CableLatency, CableMarking, CableSpeed, PeakCurrent, Platform, SupplyKind,
    };

    use super::{block, controllers, decode, from_registers};

    const CHARGING_CABLE_STATUS: [u8; 4] = [0x76, 0x9c, 0x25, 0x01];

    #[test]
    fn a_measured_cable_vdo_decodes_to_its_rating() {
        let cable = from_registers(&CHARGING_CABLE_STATUS, 0x000a_6640);
        assert_eq!(cable.marking, CableMarking::Marked);
        assert_eq!(cable.speed, CableSpeed::Usb2);
        assert_eq!(cable.milliamps, 5000);
        assert_eq!(cable.max_millivolts, 50_000);
        assert!(cable.epr);
        assert_eq!(cable.latency, CableLatency::Under30Ns);
    }

    #[test]
    fn a_port_without_an_e_marker_is_unmarked() {
        let cable = from_registers(&[0x76, 0x40], 0);
        assert_eq!(cable.marking, CableMarking::Unmarked);
        assert_eq!(cable.milliamps, 0);
    }

    #[test]
    fn an_e_marker_with_no_vdo_yet_is_unknown() {
        assert_eq!(
            from_registers(&CHARGING_CABLE_STATUS, 0).marking,
            CableMarking::Unknown
        );
    }

    #[test]
    fn a_short_status_read_is_unknown() {
        assert_eq!(
            from_registers(&[], 0x000a_6640).marking,
            CableMarking::Unknown
        );
    }

    #[test]
    fn reserved_codes_leave_their_fields_empty() {
        const RESERVED_SPEED: u32 = 0b111;
        const RESERVED_CURRENT: u32 = 0b11 << 5;
        let cable = from_registers(&CHARGING_CABLE_STATUS, RESERVED_SPEED | RESERVED_CURRENT);
        assert_eq!(cable.marking, CableMarking::Marked);
        assert_eq!(cable.speed, CableSpeed::Unknown);
        assert_eq!(cable.milliamps, 0);
        assert_eq!(cable.latency, CableLatency::Unknown);
    }

    #[test]
    fn one_read_across_both_registers_decodes_as_the_two_did() {
        let span = [
            0x76, 0x9c, 0x25, 0x01, 0x89, 0xc9, 0x00, 0x00, 0xf4, 0x41, 0x06, 0x00, 0xf4, 0xd1,
            0xc7, 0x42, 0x40, 0x66, 0x0a, 0x00,
        ];
        assert_eq!(
            decode(&span).cable,
            from_registers(&CHARGING_CABLE_STATUS, 0x000a_6640)
        );
        assert_eq!(decode(&span).measured_millivolts, Some(20_100));
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

    #[test]
    fn a_contract_decodes_its_kind_from_the_offer() {
        let charger = decode(&CHARGER_SPAN).pd.expect("a contract stands");
        assert_eq!(charger.kind, SupplyKind::Fixed);
        assert!(!charger.capability_mismatch);
        let card = decode(&CARD_SPAN).pd.expect("a contract stands");
        assert_eq!(card.kind, SupplyKind::Fixed);
    }

    #[test]
    fn a_port_without_a_contract_has_none_read() {
        assert_eq!(decode(&TYPE_C_ONLY_SPAN).pd, None);
    }

    #[test]
    fn a_marked_cables_active_bit_is_carried() {
        let mut span = TYPE_C_ONLY_SPAN;
        assert!(!decode(&span).cable.active);
        span[2] |= super::ACTIVE_CABLE;
        assert!(decode(&span).cable.active);
    }

    #[test]
    fn a_programmable_offer_carries_its_power_limit() {
        let pdo = (0b11 << 30) | (1 << 27);
        let pd = super::contract(pdo, 1 << 26);
        assert_eq!(pd.kind, SupplyKind::Pps);
        assert!(pd.power_limited);
        assert!(pd.capability_mismatch);
    }

    #[test]
    fn a_fixed_offer_carries_its_peak_current() {
        let pd = super::contract(0b10 << 20, 0);
        assert_eq!(pd.kind, SupplyKind::Fixed);
        assert_eq!(pd.peak, PeakCurrent::Overload200);
    }

    #[test]
    fn a_span_cut_short_of_the_vdo_is_unknown() {
        assert_eq!(
            decode(&CHARGING_CABLE_STATUS).cable.marking,
            CableMarking::Unknown
        );
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
}
