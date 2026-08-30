//! What the pack's reading is called: each figure the report, the window's
//! status row and the tray's line show, spelled once so no two views can
//! render one reading two ways — and the units a preset label borrows, so a
//! preset reads the way a reading of the same value does.

use frameguin_wire::{BatteryAlarm, BatteryState, ChargeFlow};

/// What a row says where the pack answered with nothing. One spelling for
/// every such row, so a value the EC left blank and a figure with no
/// denominator read the same way rather than as two different faults.
const UNKNOWN: &str = "Unknown";

/// Milliamps as the amps a person reads off a charger.
#[must_use]
pub fn amps(milliamps: u32) -> String {
    format!("{:.1} A", f64::from(milliamps) / 1000.0)
}

/// The rate as power, which is the figure a charger and a machine's draw are
/// both rated in. `None` when the pack reports no voltage, for the reason a
/// zero rate is dropped: the number would read as a fault rather than as a
/// reading.
fn watts(state: BatteryState) -> Option<String> {
    (state.millivolts != 0).then(|| {
        let watts = f64::from(state.milliamps) * f64::from(state.millivolts) / 1_000_000.0;
        format!("{watts:.1} W")
    })
}

/// Which way charge is moving, and nothing else — for a reader who has the
/// current, the voltage and the power on their own rows below, where each is
/// exact and this would only be a rounded copy.
///
/// "Plugged in, not charging" carries its own direction rather than naming a
/// rate of nothing, which is why it reads as a sentence where the others read
/// as a word.
///
/// A rate arriving under `Idle` names the charge ending rather than nothing at
/// all. That pairing has one source: the daemon reaches `Idle` with a rate only
/// where the EC claims neither direction, which is the state its charge limiter
/// holds the pack in while the current falls away. A pack simply resting at its
/// ceiling reports a clean zero, so the two never collide.
#[must_use]
pub fn charge_direction(state: BatteryState) -> &'static str {
    match state.flow {
        ChargeFlow::Charging => "Charging",
        ChargeFlow::Discharging => "Discharging",
        ChargeFlow::Idle if state.milliamps > 0 => "Finishing charge",
        ChargeFlow::Idle => "Plugged in, not charging",
    }
}

/// The rate as power, for the row that shows it beside the current and the
/// voltage whose product it is. Unknown rather than zero where the pack
/// reports no voltage: a watt figure computed from a voltage that isn't there
/// would read as a measurement.
#[must_use]
pub fn power_label(state: BatteryState) -> String {
    watts(state).unwrap_or_else(|| UNKNOWN.to_string())
}

/// Which way charge is moving, with the rate where there is one to name — for
/// the window's row and the tray's line, which have nowhere else to put it.
///
/// A rate of zero is dropped rather than rendered: "0.0 A" is what a pack
/// reports in the moment either side of a direction changing, and it reads as
/// a fault.
#[must_use]
pub fn charge_flow_label(state: BatteryState) -> String {
    let direction = charge_direction(state);
    if state.milliamps == 0 {
        return direction.to_string();
    }
    let rate = amps(state.milliamps);
    match watts(state) {
        Some(watts) => format!("{direction} at {rate} ({watts})"),
        None => format!("{direction} at {rate}"),
    }
}

/// A capacity as the energy it holds, taken against the pack's nominal
/// voltage, which is the convention a pack is rated by — and the unit
/// Framework quote their batteries in, so it is the figure a reader can check
/// against the spec. Milliamp-hours alone are only half of it: they say
/// nothing about the voltage the cells deliver them at.
fn watt_hours(milliamp_hours: u32, design_millivolts: u32) -> String {
    let watt_hours = f64::from(milliamp_hours) * f64::from(design_millivolts) / 1_000_000.0;
    format!("{watt_hours:.1} Wh")
}

/// A capacity in both units, energy first because that is what the pack is
/// sold as, with the charge the EC actually reported after it.
#[must_use]
pub fn capacity(milliamp_hours: u32, design_millivolts: u32) -> String {
    format!(
        "{} ({milliamp_hours} mAh)",
        watt_hours(milliamp_hours, design_millivolts)
    )
}

/// Millivolts as the volts a pack is rated in.
#[must_use]
pub fn volts(millivolts: u32) -> String {
    format!("{:.2} V", f64::from(millivolts) / 1000.0)
}

/// A rate at the precision the EC reports it, where [`amps`] rounds to the
/// tenth a charger is labelled with. The report is where the exact figure
/// belongs: "1.2 A" cannot show a current settling, which is most of what
/// there is to watch as a charge ends.
#[must_use]
pub fn milliamps(milliamps: u32) -> String {
    format!("{milliamps} mA")
}

/// A charge as a percentage.
#[must_use]
pub fn percent_label(percent: u8) -> String {
    format!("{percent}%")
}

/// How much of its rating the pack still holds, or the word for one with
/// nothing to measure it against. A new pack that outperforms its rating
/// reads above 100%, left as it stands: unlike a charge, this has no ceiling
/// that makes more than full meaningless.
#[must_use]
pub fn retention_label(last_full_capacity: u32, design_capacity: u32) -> String {
    match retention_percent(last_full_capacity, design_capacity) {
        Some(percent) => format!("{percent}%"),
        None => UNKNOWN.to_string(),
    }
}

/// What the pack can still hold against what it was built to hold. `None`
/// where the design capacity reads zero — nothing to compare against, and 0%
/// would name a dead pack rather than an unanswered question.
fn retention_percent(last_full_capacity: u32, design_capacity: u32) -> Option<u32> {
    if design_capacity == 0 {
        return None;
    }
    // Widened for the multiply: the product of two plausible mAh figures
    // leaves u32 long before either of them does.
    let retained = u64::from(last_full_capacity) * 100 / u64::from(design_capacity);
    u32::try_from(retained).ok()
}

/// The pack's temperature, to the tenth of a degree its sensor resolves.
#[must_use]
pub fn temperature(decicelsius: i16) -> String {
    format!("{:.1} °C", f64::from(decicelsius) / 10.0)
}

#[must_use]
pub fn charger_label(connected: bool) -> &'static str {
    if connected {
        "Connected"
    } else {
        "Not connected"
    }
}

/// The gap between the pack's highest and lowest cell, which is what four cell
/// voltages are worth knowing. The EC publishes only their sum, and a pack
/// whose total reads healthy can still have one cell drifting away from the
/// rest — that drift is the earliest sign a pack is failing, and the only
/// place it shows. `None` on a pack that reports no cells.
#[must_use]
pub fn cell_spread(cell_millivolts: &[u32]) -> Option<String> {
    let high = cell_millivolts.iter().max()?;
    let low = cell_millivolts.iter().min()?;
    Some(format!("{} mV", high - low))
}

/// Every cell's voltage on one line, to sit under the spread. To three
/// decimals where [`volts`] gives two: the whole point is the millivolts
/// between them, which two decimals would round away.
#[must_use]
pub fn cell_voltages(cell_millivolts: &[u32]) -> String {
    let cells: Vec<String> = cell_millivolts
        .iter()
        .map(|millivolts| format!("{:.3}", f64::from(*millivolts) / 1000.0))
        .collect();
    format!("{} V", cells.join(" · "))
}

fn alarm_label(alarm: BatteryAlarm) -> &'static str {
    match alarm {
        BatteryAlarm::OverCharged => "Charged past its safe limit",
        BatteryAlarm::OverTemperature => "Too hot",
        // What the pack is doing, not the flags it is doing it with: a reader
        // wants to know the battery has stopped, not that two bits are set.
        BatteryAlarm::SafetyFault => "Refusing to charge or discharge",
    }
}

/// Every alarm the pack is raising, on one line. Empty for a pack raising
/// none, which is the caller's cue to show nothing at all rather than a row
/// announcing that nothing is wrong.
#[must_use]
pub fn alarms_label(alarms: &[BatteryAlarm]) -> String {
    alarms
        .iter()
        .map(|alarm| alarm_label(*alarm))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A name the pack left blank, which some do for the serial. Named rather than
/// left empty: a blank value reads as a row that failed to fill.
#[must_use]
pub fn text_or_unknown(text: &str) -> &str {
    if text.is_empty() { UNKNOWN } else { text }
}

/// The whole state on one line, for the tray, which has no second line to
/// put the charge on.
#[must_use]
pub fn battery_summary(state: BatteryState) -> String {
    format!(
        "{} · {}",
        percent_label(state.percent),
        charge_flow_label(state)
    )
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{BatteryState, ChargeFlow};

    use super::{
        battery_summary, capacity, charge_direction, charge_flow_label, power_label,
        retention_label, volts, watt_hours,
    };
    use crate::testing::{CAPACITY, MILLIVOLTS, NOMINAL_MILLIVOLTS, state};

    /// A pack that has lost some of what it was built to hold, which is what
    /// the report's retention row exists to say.
    #[test]
    fn retention_is_the_last_full_charge_against_the_design_capacity() {
        assert_eq!(retention_label(4176, CAPACITY), "90%");
        assert_eq!(retention_label(CAPACITY, CAPACITY), "100%");
    }

    /// A new pack can hold more than it was rated for, and the row says so
    /// rather than clamping — nothing about retention above 100% is nonsense
    /// the way a charge above full would be.
    #[test]
    fn retention_above_the_design_capacity_is_left_as_it_reads() {
        assert_eq!(retention_label(4736, CAPACITY), "102%");
    }

    /// A pack reporting no design capacity leaves nothing to divide by, and
    /// 0% would name a dead pack rather than a missing answer.
    #[test]
    fn retention_against_no_design_capacity_is_not_a_number() {
        assert_eq!(retention_label(4176, 0), "Unknown");
    }

    #[test]
    fn millivolts_read_as_the_volts_a_pack_is_rated_in() {
        assert_eq!(volts(MILLIVOLTS), "15.40 V");
    }

    /// The figure Framework quote a pack by, which milliamp-hours alone cannot
    /// be checked against.
    #[test]
    fn a_capacity_leads_with_the_energy_it_holds() {
        assert_eq!(capacity(CAPACITY, NOMINAL_MILLIVOLTS), "72.6 Wh (4640 mAh)");
    }

    /// Taken against the pack's rating, so the same charge reads the same
    /// whatever the terminal voltage is doing at the time.
    #[test]
    fn energy_is_the_charge_against_the_nominal_voltage() {
        assert_eq!(watt_hours(2843, NOMINAL_MILLIVOLTS), "44.5 Wh");
        assert_eq!(watt_hours(0, NOMINAL_MILLIVOLTS), "0.0 Wh");
    }

    #[test]
    fn a_moving_charge_is_named_with_its_rate_and_its_power() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Charging, 2320)),
            "Charging at 2.3 A (35.7 W)"
        );
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Discharging, 1400)),
            "Discharging at 1.4 A (21.6 W)"
        );
    }

    /// Power is the rate against the voltage of that same moment, so a pack
    /// that answers with no voltage names the rate alone rather than 0.0 W.
    #[test]
    fn a_rate_without_a_voltage_is_named_without_its_power() {
        let unread = BatteryState {
            millivolts: 0,
            ..state(ChargeFlow::Discharging, 1400)
        };
        assert_eq!(charge_flow_label(unread), "Discharging at 1.4 A");
    }

    /// A pack reports no rate for a moment either side of a direction
    /// changing, and "at 0.0 A" reads as a fault rather than as a reading.
    #[test]
    fn a_direction_without_a_rate_is_named_alone() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Charging, 0)),
            "Charging"
        );
    }

    /// A full pack on a charger is the state the EC's own flags describe as
    /// discharging, and the one a rate would say nothing useful about.
    #[test]
    fn a_pack_resting_on_its_charger_names_neither_a_direction_nor_a_rate() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Idle, 0)),
            "Plugged in, not charging"
        );
    }

    /// The charge limiter holds the pack with the current still falling away,
    /// and the watts are the point: saying only "not charging" there hides a
    /// reading the pack is still giving.
    #[test]
    fn a_charge_winding_down_at_the_limit_is_named_with_its_rate() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Idle, 2320)),
            "Finishing charge at 2.3 A (35.7 W)"
        );
    }

    /// The report says the same states in one word, because the rows under it
    /// carry the rate exactly. Every state the full label names has to survive
    /// the shortening — the resting pack above most of all, whose sentence is
    /// its direction rather than an omission.
    #[test]
    fn the_report_names_a_direction_without_the_rate_beneath_it() {
        assert_eq!(
            charge_direction(state(ChargeFlow::Charging, 2320)),
            "Charging"
        );
        assert_eq!(
            charge_direction(state(ChargeFlow::Discharging, 1400)),
            "Discharging"
        );
        assert_eq!(
            charge_direction(state(ChargeFlow::Idle, 2320)),
            "Finishing charge"
        );
        assert_eq!(
            charge_direction(state(ChargeFlow::Idle, 0)),
            "Plugged in, not charging"
        );
    }

    /// The row the rate was taken out of the subtitle for.
    #[test]
    fn power_is_the_rate_against_the_voltage_of_the_moment() {
        assert_eq!(power_label(state(ChargeFlow::Charging, 2320)), "35.7 W");
        // A pack either side of a direction change, where zero is the reading
        // rather than a fault — the row beside it says 0 mA.
        assert_eq!(power_label(state(ChargeFlow::Charging, 0)), "0.0 W");
    }

    /// No voltage means no product to report, and 0.0 W would read as one.
    #[test]
    fn power_without_a_voltage_is_not_a_number() {
        let unread = BatteryState {
            millivolts: 0,
            ..state(ChargeFlow::Discharging, 1400)
        };
        assert_eq!(power_label(unread), "Unknown");
    }

    /// The window splits charge and direction across a row's two halves; the
    /// tray has one line for both.
    #[test]
    fn the_trays_line_carries_the_charge_as_well() {
        assert_eq!(
            battery_summary(state(ChargeFlow::Charging, 2320)),
            "62% · Charging at 2.3 A (35.7 W)"
        );
    }
}
