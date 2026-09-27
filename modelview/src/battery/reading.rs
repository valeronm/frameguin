//! What the pack's reading is called: each figure the report, the window's
//! status row and the tray's line show, spelled once so no two views can
//! render one reading two ways.

use frameguin_contract::{BatteryAlarm, BatteryState, ChargeFlow};

use crate::units::{self, Kind, cell_volts, millivolts, volts, watt_hours, watts};

/// What a row says where the pack answered with nothing. One spelling for
/// every such row, so a value the EC left blank and a figure with no
/// denominator read the same way rather than as two different faults.
const UNKNOWN: &str = "Unknown";

/// The rate as power, which is the figure a charger and a machine's draw are
/// both rated in. `None` when the pack reports no voltage, for the reason a
/// zero rate is dropped: the number would read as a fault rather than as a
/// reading.
fn power(state: BatteryState) -> Option<String> {
    (state.millivolts != 0).then(|| watts(state.millivolts, state.milliamps, Kind::Dynamic))
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
/// would read as a measurement. A zero rate is shown, being the reading
/// either side of a direction change rather than a fault.
#[must_use]
pub fn power_label(state: BatteryState) -> String {
    power(state).unwrap_or_else(|| UNKNOWN.to_string())
}

/// Which way charge is moving, with its power where there is one to name.
///
/// A rate of zero is dropped rather than rendered: "0.0 W" is what a pack
/// reports in the moment either side of a direction changing, and it reads as
/// a fault.
#[must_use]
pub fn charge_flow_label(state: BatteryState) -> String {
    let direction = charge_direction(state);
    match power(state) {
        Some(power) if state.milliamps != 0 => format!("{direction} at {power}"),
        _ => direction.to_string(),
    }
}

/// A capacity as the energy it holds, taken against the pack's nominal
/// voltage, which is the convention a pack is rated by — and the unit
/// Framework quote their batteries in, so it is the figure a reader can check
/// against the spec.
#[must_use]
pub fn capacity(milliamp_hours: u32, design_millivolts: u32) -> String {
    watt_hours(milliamp_hours, design_millivolts)
}

#[must_use]
pub fn voltage_label(millivolts: u32) -> String {
    volts(millivolts, Kind::Dynamic)
}

#[must_use]
pub fn current_label(milliamps: u32) -> String {
    units::milliamps(milliamps)
}

#[must_use]
pub fn cell_voltages(cell_millivolts: &[u32]) -> String {
    cell_volts(cell_millivolts)
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

/// Whether the EC sees a supply at all, which is a different question from
/// what one is negotiating — [`crate::ports::supply_label`] answers that,
/// from the ports rather than from this flag.
#[must_use]
pub fn charger_label(connected: bool) -> &'static str {
    if connected {
        "Connected"
    } else {
        crate::words::NO_SUPPLY
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
    Some(millivolts(high - low))
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

/// The charge and which way it is moving, without the rate.
#[must_use]
pub fn charge_brief(state: BatteryState) -> String {
    format!(
        "{} · {}",
        percent_label(state.percent),
        charge_direction(state)
    )
}

#[cfg(test)]
mod tests {
    use frameguin_contract::{BatteryState, ChargeFlow};

    use super::{
        battery_summary, capacity, charge_brief, charge_direction, charge_flow_label, power_label,
        retention_label,
    };
    use frameguin_model::fixtures::{CAPACITY, NOMINAL_MILLIVOLTS, state};

    #[test]
    fn retention_is_the_last_full_charge_against_the_design_capacity() {
        assert_eq!(retention_label(4176, CAPACITY), "90%");
        assert_eq!(retention_label(CAPACITY, CAPACITY), "100%");
    }

    #[test]
    fn retention_above_the_design_capacity_is_left_as_it_reads() {
        assert_eq!(retention_label(4736, CAPACITY), "102%");
    }

    #[test]
    fn retention_against_no_design_capacity_is_not_a_number() {
        assert_eq!(retention_label(4176, 0), "Unknown");
    }

    #[test]
    fn a_capacity_is_the_energy_it_holds_at_the_nominal_voltage() {
        assert_eq!(capacity(CAPACITY, NOMINAL_MILLIVOLTS), "72.6 Wh");
        assert_eq!(capacity(2843, NOMINAL_MILLIVOLTS), "44.5 Wh");
    }

    #[test]
    fn a_moving_charge_is_named_with_its_power() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Charging, 2320)),
            "Charging at 35.7 W"
        );
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Discharging, 1400)),
            "Discharging at 21.6 W"
        );
    }

    #[test]
    fn a_rate_without_a_voltage_is_named_by_its_direction_alone() {
        let unread = BatteryState {
            millivolts: 0,
            ..state(ChargeFlow::Discharging, 1400)
        };
        assert_eq!(charge_flow_label(unread), "Discharging");
    }

    #[test]
    fn a_direction_without_a_rate_is_named_alone() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Charging, 0)),
            "Charging"
        );
    }

    #[test]
    fn a_pack_resting_on_its_charger_names_neither_a_direction_nor_a_rate() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Idle, 0)),
            "Plugged in, not charging"
        );
    }

    #[test]
    fn a_charge_winding_down_at_the_limit_is_named_with_its_power() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Idle, 2320)),
            "Finishing charge at 35.7 W"
        );
    }

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

    #[test]
    fn power_is_the_rate_against_the_voltage_of_the_moment() {
        assert_eq!(power_label(state(ChargeFlow::Charging, 2320)), "35.7 W");
        assert_eq!(power_label(state(ChargeFlow::Charging, 0)), "0.0 W");
    }

    #[test]
    fn power_without_a_voltage_is_not_a_number() {
        let unread = BatteryState {
            millivolts: 0,
            ..state(ChargeFlow::Discharging, 1400)
        };
        assert_eq!(power_label(unread), "Unknown");
    }

    #[test]
    fn the_trays_line_carries_the_charge_as_well() {
        assert_eq!(
            battery_summary(state(ChargeFlow::Charging, 2320)),
            "62% · Charging at 35.7 W"
        );
    }

    #[test]
    fn the_brief_line_drops_the_rate() {
        assert_eq!(
            charge_brief(state(ChargeFlow::Charging, 2320)),
            "62% · Charging"
        );
    }
}
