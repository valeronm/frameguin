//! The Smart Battery registers the pack is asked for past the EC's block,
//! and what their words mean. No EC call is made here: [`crate::ec`] reads
//! the word, this decodes it.

use frameguin_wire as wire;

/// The gauge's address on the pack's bus, the same on every Framework board
/// — one gauge IC — and the 7-bit form of the 8-bit 0x16 the datasheet
/// names.
pub(crate) const I2C_ADDR: u16 = 0x0b;
/// `SB_CYCLE_COUNT` in the Smart Battery spec, and the register the EC itself
/// reads for the value it publishes.
pub(crate) const CYCLE_COUNT: u16 = 0x17;
/// `SB_MANUFACTURE_DATE`, packed into one word — see [`manufactured_iso`].
pub(crate) const MANUFACTURE_DATE: u16 = 0x1b;
/// The per-cell voltages, in the order the pack numbers its cells. The
/// registers run backwards against that numbering, which is the datasheet's
/// doing rather than a mistake here: 0x3F is cell 1 and 0x3C is cell 4.
pub(crate) const CELL_VOLTAGES: [u16; 4] = [0x3f, 0x3e, 0x3d, 0x3c];
/// `SB_BATTERY_STATUS`, whose alarm bits are decoded by [`alarms`]. The EC
/// reads this register too, but publishes only the two direction flags out of
/// it — the alarms have nowhere in the memmap to go.
pub(crate) const BATTERY_STATUS: u16 = 0x16;
/// `SB_TEMPERATURE`, in tenths of a Kelvin.
///
/// The EC polls this same register and republishes it into its thermal sensor
/// array — its devicetree node is `cros-ec,temp-sensor-battery` at the pack's
/// own I2C address, described as "the last polled battery temperature". So
/// this is not a second opinion but the same sensor read first-hand: at the
/// tenth of a degree it measures in rather than the whole degree the array
/// carries, as freshly as the ask, and on every board rather than only those
/// that wire the relay.
pub(crate) const TEMPERATURE: u16 = 0x08;

/// Tenths of a Kelvin between absolute zero and freezing, for turning the
/// pack's reading into tenths of a degree Celsius. The true offset is 2731.5;
/// rounding it costs at most a twentieth of a degree, which is half of what
/// the sensor resolves, and matches what `framework_tool` prints.
const FREEZING_DECIKELVIN: i32 = 2732;

/// Which bit of the status word raises which alarm. Only the two that mean a
/// fault — [`wire::BatteryAlarm`] carries why the word's other set bits do
/// not, and every one of them is raised by a pack working exactly as it
/// should.
const ALARM_BITS: [(u16, wire::BatteryAlarm); 2] = [
    (1 << 15, wire::BatteryAlarm::OverCharged),
    (1 << 12, wire::BatteryAlarm::OverTemperature),
];

/// The pack asking that charging stop, and that discharging stop. Absent from
/// the table above because neither means anything alone — the gauge raises
/// each at the ordinary end of its direction — and present here because
/// together they cannot be ordinary at all. See [`wire::BatteryAlarm`].
const TERMINATE_CHARGE: u16 = 1 << 14;
const TERMINATE_DISCHARGE: u16 = 1 << 11;

/// The alarms a status word is raising, by name. A word raising none — the
/// ordinary case — gives an empty list rather than a state of its own.
pub(crate) fn alarms(status: u16) -> Vec<wire::BatteryAlarm> {
    let mut raised: Vec<wire::BatteryAlarm> = ALARM_BITS
        .iter()
        .filter(|(bit, _)| status & bit != 0)
        .map(|(_, alarm)| *alarm)
        .collect();
    if status & TERMINATE_CHARGE != 0 && status & TERMINATE_DISCHARGE != 0 {
        raised.push(wire::BatteryAlarm::SafetyFault);
    }
    raised
}

/// The pack's packed manufacturing date as the ISO-8601 the wire carries: day
/// in the low five bits, month in the next four, years since 1980 in the rest.
///
/// None on a word that decodes to no real date, which is what an uninitialized
/// register reads as — a pack that was never given a date reports zeros, and
/// "1980-00-00" is worse than saying nothing.
pub(crate) fn manufactured_iso(packed: u16) -> Option<String> {
    let day = packed & 0x1f;
    let month = (packed >> 5) & 0x0f;
    if day == 0 || month == 0 || month > 12 {
        return None;
    }
    Some(format!("{:04}-{month:02}-{day:02}", 1980 + (packed >> 9)))
}

/// The pack's reading as tenths of a degree Celsius. Signed, because a machine
/// left in the cold reads below freezing; saturating, because the arithmetic
/// is only unbounded in a word the pack could not have produced.
pub(crate) fn decicelsius(decikelvin: u16) -> i16 {
    i16::try_from(i32::from(decikelvin) - FREEZING_DECIKELVIN).unwrap_or(i16::MAX)
}

#[cfg(test)]
mod tests {
    use super::{
        TERMINATE_CHARGE, TERMINATE_DISCHARGE, alarms, decicelsius, manufactured_iso, wire,
    };

    /// The reading `framework_tool` prints as 34.2 C for the same word.
    #[test]
    fn the_packs_decikelvin_reads_as_tenths_of_a_degree() {
        assert_eq!(decicelsius(3074), 342);
    }

    /// The offset puts freezing at 2732, so a pack below it is a temperature
    /// rather than an underflow.
    #[test]
    fn a_cold_pack_reads_below_zero() {
        assert_eq!(decicelsius(2732), 0);
        assert_eq!(decicelsius(2632), -100);
    }

    /// The pack's own packed form, taken from a real label: 2026-05-14.
    #[test]
    fn a_packed_date_unpacks_to_the_day_the_pack_was_built() {
        let packed = ((2026 - 1980) << 9) | (5 << 5) | 0x0e;
        assert_eq!(manufactured_iso(packed).as_deref(), Some("2026-05-14"));
    }

    /// A register nobody wrote reads as zeros, which is not the first of
    /// January 1980.
    #[test]
    fn an_unwritten_date_register_is_no_date_at_all() {
        assert_eq!(manufactured_iso(0), None);
        // A year with a day but no month, and a month past December.
        assert_eq!(manufactured_iso(1), None);
        assert_eq!(manufactured_iso((13 << 5) | 1), None);
    }

    /// The ordinary reading: initialized and discharging, both states rather
    /// than alarms.
    #[test]
    fn a_pack_running_normally_raises_no_alarm() {
        assert!(alarms(0x00c0).is_empty());
    }

    #[test]
    fn each_alarm_bit_is_named() {
        assert_eq!(alarms(1 << 15), vec![wire::BatteryAlarm::OverCharged]);
        assert_eq!(alarms(1 << 12), vec![wire::BatteryAlarm::OverTemperature]);
    }

    /// The bits a healthy pack sets in the course of its work: full, empty,
    /// asking that charging or discharging end. Reading any of them as a fault
    /// would put a red row on a battery that had merely finished charging.
    #[test]
    fn the_states_a_working_pack_sets_are_not_faults() {
        for bit in [1 << 14, 1 << 11, 1 << 9, 1 << 8, 1 << 5, 1 << 4] {
            assert!(alarms(bit).is_empty(), "bit {bit:#x} read as an alarm");
        }
    }

    /// Neither terminate alarm means anything alone, and the gauge raises each
    /// only in the direction it applies to — one while charging, the other
    /// while discharging. Both at once is the safety alert or permanent
    /// failure that is the only other way either is set.
    #[test]
    fn asking_to_stop_both_ways_at_once_is_a_fault() {
        let both = TERMINATE_CHARGE | TERMINATE_DISCHARGE;
        assert_eq!(alarms(both), vec![wire::BatteryAlarm::SafetyFault]);
        // And still a fault beside the state bits a pack sets while it holds.
        assert_eq!(alarms(both | 0x00c0), vec![wire::BatteryAlarm::SafetyFault]);
    }

    /// Alarms are a set, not a state: a pack in trouble raises several at
    /// once, and reporting only the first would hide the rest.
    #[test]
    fn a_pack_in_trouble_raises_every_alarm_it_has() {
        let status = (1 << 15) | (1 << 12) | 0x00c0;
        assert_eq!(
            alarms(status),
            vec![
                wire::BatteryAlarm::OverCharged,
                wire::BatteryAlarm::OverTemperature
            ]
        );
    }
}
