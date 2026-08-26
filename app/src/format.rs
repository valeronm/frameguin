//! The values the controls offer and the words for them: the presets, the
//! percentages and currents behind them, and how each is named. Nothing here
//! reaches GTK or the bus, which is what lets the window and the tray share
//! it and so keeps them from disagreeing about what "Half" sends or what a
//! ceiling of 100% is called.
//!
//! Row order is settled here, except where the vocabulary already carries an
//! order that is a fact about the values rather than a layout — the click
//! forces and the haptic steps — and `wire` keeps it. A control that caps
//! something starts at the setting that caps nothing and tightens down the
//! list; every other reads as a scale climbing from off. An automatic mode is
//! on no scale and leads; the row that reveals a slider trails the presets it
//! extends.

use frameguin_wire::{
    BatteryAlarm, BatteryState, ChargeFlow, ClickForce, HAPTIC_INTENSITY_LEVELS,
    NO_CHARGE_CURRENT_LIMIT, PowerLedLevel,
};

const CHARGE_PRESETS: [u8; 3] = [100, 80, 60];

/// The ceiling that is no ceiling. A presentation fact rather than a wire
/// one: the daemon takes 100 and writes it to the EC like any other
/// percentage, and it is only here that the value stops being a limit and
/// starts being the absence of one.
pub(crate) const NO_CHARGE_LIMIT: u8 = 100;

/// The window's combo carries one row past the presets, for a ceiling the
/// user dials in; the tray offers the presets alone.
pub(crate) const CHARGE_LIMIT_CUSTOM: usize = CHARGE_PRESETS.len();

/// The lowest ceiling the daemon accepts, and so the slider's floor.
pub(crate) const MIN_CHARGE_LIMIT: f64 = 20.0;

/// The charge speeds the combo offers, as the divisor applied to the
/// battery's 1C design current. `None` is full speed, which the daemon takes
/// as no limit at all.
const CHARGE_SPEEDS: [Option<u32>; 3] = [None, Some(2), Some(4)];
pub(crate) const CHARGE_SPEED_LABELS: [&str; 3] = ["Full speed", "Half", "Quarter"];

/// The window's combo carries one row past the presets, for a rate the user
/// dials in. The tray offers only the presets: a slider has no menu form, and
/// a preset menu that can't reach every state is the honest half.
pub(crate) const CHARGE_SPEED_CUSTOM: usize = CHARGE_SPEEDS.len();

/// The slowest the custom slider will ask for. The EC takes anything above
/// zero, but a limit this side of it charges so slowly that it reads as a
/// fault rather than a setting.
pub(crate) const MIN_CUSTOM_CHARGE_MA: f64 = 100.0;

/// What the custom slider rounds to. A `GtkScale` is continuous while
/// dragged — its step increment reaches only keys and the wheel — so without
/// this a drag lands on a value like 984 mA that the row then displays as
/// "1.0 A", reporting a current nobody chose.
pub(crate) const CUSTOM_CHARGE_STEP_MA: f64 = 100.0;

/// GTK carries adjustment values as f64. The cast alone saturates at 255, so
/// the clamp is what holds the result inside the range the daemon accepts;
/// each control's own floor is enforced by its adjustment.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped into range before the cast"
)]
pub(crate) fn scale_percent(value: f64) -> u8 {
    value.round().clamp(0.0, 100.0) as u8
}

/// GTK carries the slider's value as f64; the clamp is what holds the result
/// inside what the daemon accepts, its floor coming from the adjustment.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped into range before the cast"
)]
pub(crate) fn scale_milliamps(value: f64) -> u32 {
    let snapped = (value / CUSTOM_CHARGE_STEP_MA).round() * CUSTOM_CHARGE_STEP_MA;
    snapped.clamp(MIN_CUSTOM_CHARGE_MA, f64::from(u32::MAX)) as u32
}

/// Milliamps as the amps a person reads off a charger.
pub(crate) fn amps(milliamps: u32) -> String {
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
pub(crate) fn charge_direction(state: BatteryState) -> &'static str {
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
pub(crate) fn power_label(state: BatteryState) -> String {
    watts(state).unwrap_or_else(|| UNKNOWN.to_string())
}

/// Which way charge is moving, with the rate where there is one to name — for
/// the window's row and the tray's line, which have nowhere else to put it.
///
/// A rate of zero is dropped rather than rendered: "0.0 A" is what a pack
/// reports in the moment either side of a direction changing, and it reads as
/// a fault.
pub(crate) fn charge_flow_label(state: BatteryState) -> String {
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
pub(crate) fn capacity(milliamp_hours: u32, design_millivolts: u32) -> String {
    format!(
        "{} ({milliamp_hours} mAh)",
        watt_hours(milliamp_hours, design_millivolts)
    )
}

/// Millivolts as the volts a pack is rated in.
pub(crate) fn volts(millivolts: u32) -> String {
    format!("{:.2} V", f64::from(millivolts) / 1000.0)
}

/// A rate at the precision the EC reports it, where [`amps`] rounds to the
/// tenth a charger is labelled with. The report is where the exact figure
/// belongs: "1.2 A" cannot show a current settling, which is most of what
/// there is to watch as a charge ends.
pub(crate) fn milliamps(milliamps: u32) -> String {
    format!("{milliamps} mA")
}

/// A charge as a percentage: what the window's row and the report's first line
/// both show, spelled once so the two windows cannot render one reading two
/// ways.
pub(crate) fn percent_label(percent: u8) -> String {
    format!("{percent}%")
}

/// How much of its rating the pack still holds, or the word for one with
/// nothing to measure it against. A new pack that outperforms its rating
/// reads above 100%, left as it stands: unlike a charge, this has no ceiling
/// that makes more than full meaningless.
pub(crate) fn retention_label(last_full_capacity: u32, design_capacity: u32) -> String {
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
pub(crate) fn temperature(decicelsius: i16) -> String {
    format!("{:.1} °C", f64::from(decicelsius) / 10.0)
}

pub(crate) fn charger_label(connected: bool) -> &'static str {
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
pub(crate) fn cell_spread(cell_millivolts: &[u32]) -> Option<String> {
    let high = cell_millivolts.iter().max()?;
    let low = cell_millivolts.iter().min()?;
    Some(format!("{} mV", high - low))
}

/// Every cell's voltage on one line, to sit under the spread. To three
/// decimals where [`volts`] gives two: the whole point is the millivolts
/// between them, which two decimals would round away.
pub(crate) fn cell_voltages(cell_millivolts: &[u32]) -> String {
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
pub(crate) fn alarms_label(alarms: &[BatteryAlarm]) -> String {
    alarms
        .iter()
        .map(|alarm| alarm_label(*alarm))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// What the report says where the pack answered with nothing. One spelling for
/// every such row, so a value the EC left blank and a figure with no
/// denominator read the same way rather than as two different faults.
const UNKNOWN: &str = "Unknown";

/// A name the pack left blank, which some do for the serial. Named rather than
/// left empty: a blank value reads as a row that failed to fill.
pub(crate) fn text_or_unknown(text: &str) -> &str {
    if text.is_empty() { UNKNOWN } else { text }
}

/// The whole state on one line, for the tray, which has no second line to
/// put the charge on.
pub(crate) fn battery_summary(state: BatteryState) -> String {
    format!(
        "{} · {}",
        percent_label(state.percent),
        charge_flow_label(state)
    )
}

/// The milliamps a charge speed asks the daemon for. Shared by the window and
/// the tray so the two can't disagree about what "Half" sends.
pub(crate) fn charge_speed_milliamps(design_capacity: u32, index: usize) -> u32 {
    match CHARGE_SPEEDS.get(index).copied().flatten() {
        Some(divisor) => design_capacity / divisor,
        None => NO_CHARGE_CURRENT_LIMIT,
    }
}

/// Which speed a limit corresponds to, and `None` when it matches no preset —
/// `framework_tool` can set any value, and guessing the nearest would
/// misreport it.
pub(crate) fn charge_speed_position(design_capacity: u32, milliamps: u32) -> Option<usize> {
    (0..CHARGE_SPEEDS.len())
        .find(|&index| charge_speed_milliamps(design_capacity, index) == milliamps)
}

/// Combo labels carrying the rate each fraction works out to — "Half" alone
/// doesn't say half of what.
pub(crate) fn charge_speed_labels(design_capacity: u32) -> Vec<String> {
    CHARGE_SPEEDS
        .iter()
        .zip(CHARGE_SPEED_LABELS)
        .map(|(divisor, label)| match divisor {
            Some(divisor) => format!("{label} ({})", amps(design_capacity / divisor)),
            None => label.to_string(),
        })
        .collect()
}

/// The ceiling a preset row asks the daemon for. Rows are addressed by
/// position in the labels built below, so an index from anywhere else can be
/// out of range.
pub(crate) fn charge_limit_percent(row: usize) -> u8 {
    CHARGE_PRESETS[row]
}

/// Which preset a ceiling sits on, and `None` when it matches none — the EC's
/// own battery extender lowers the limit unasked, and guessing the nearest
/// preset would misreport it.
pub(crate) fn charge_limit_position(percent: u8) -> Option<usize> {
    CHARGE_PRESETS.iter().position(|preset| *preset == percent)
}

/// Preset names, shared so the window's combo and the tray's menu can't
/// disagree about what a ceiling is called. The window's combo appends
/// "Custom"; the tray's menu takes these as they are.
pub(crate) fn charge_limit_labels() -> Vec<String> {
    CHARGE_PRESETS
        .iter()
        // Named as a state rather than as the absence of one, so a title
        // quoting the selected row still reads.
        .map(|percent| {
            if *percent == NO_CHARGE_LIMIT {
                "Off".to_string()
            } else {
                format!("{percent}%")
            }
        })
        .collect()
}

/// A combo's rows: the presets, then the one that reveals a slider. Both
/// preset-plus-custom controls build their model this way, so neither can
/// leave the extra row off and address it anyway.
pub(crate) fn with_custom_row(mut labels: Vec<String>) -> Vec<String> {
    labels.push("Custom".to_string());
    labels
}

/// Where a level's row sits. A match rather than a second list of the levels,
/// so a level added to the vocabulary fails to build here rather than landing
/// wherever it happened to be declared.
pub(crate) fn power_led_row_rank(level: PowerLedLevel) -> u8 {
    match level {
        PowerLedLevel::Auto => 0,
        PowerLedLevel::Off => 1,
        PowerLedLevel::UltraLow => 2,
        PowerLedLevel::Low => 3,
        PowerLedLevel::Medium => 4,
        PowerLedLevel::High => 5,
        PowerLedLevel::Custom => 6,
    }
}

fn power_led_level_label(level: PowerLedLevel) -> &'static str {
    match level {
        PowerLedLevel::Auto => "Auto",
        PowerLedLevel::Off => "Off",
        PowerLedLevel::UltraLow => "Ultra-low",
        PowerLedLevel::Low => "Low",
        PowerLedLevel::Medium => "Medium",
        PowerLedLevel::High => "High",
        PowerLedLevel::Custom => "Custom",
    }
}

pub(crate) fn power_led_level_labels(levels: &[PowerLedLevel]) -> Vec<String> {
    levels
        .iter()
        .map(|level| power_led_level_label(*level).into())
        .collect()
}

/// The haptic combo's rows, derived from the steps they select rather than
/// kept in step with them by hand, which a step added upstream would break
/// silently.
pub(crate) fn haptic_labels() -> Vec<String> {
    HAPTIC_INTENSITY_LEVELS
        .iter()
        .map(|&percent| {
            if percent == 0 {
                "Off".to_string()
            } else {
                format!("{percent}%")
            }
        })
        .collect()
}

/// The touchscreen's two states as the tray names them, each beside the state
/// its row means.
///
/// One array rather than a list of labels and an index constant beside it: a
/// menu row is picked by position, so the pairing is what a click depends on,
/// and spelling it in two places is what lets a reordering mark one row while
/// writing the other.
const TOUCHSCREEN_STATES: [(&str, bool); 2] = [("Off", false), ("On", true)];

pub(crate) fn touchscreen_labels() -> Vec<String> {
    TOUCHSCREEN_STATES
        .iter()
        .map(|(label, _)| (*label).to_string())
        .collect()
}

/// Which row a state sits on, for marking the group.
pub(crate) fn touchscreen_position(enabled: bool) -> Option<usize> {
    TOUCHSCREEN_STATES
        .iter()
        .position(|(_, state)| *state == enabled)
}

/// What a row means, for sending it. None for a row nothing is listed at,
/// which a group drawn from these labels cannot produce.
pub(crate) fn touchscreen_state(row: usize) -> Option<bool> {
    TOUCHSCREEN_STATES.get(row).map(|(_, state)| *state)
}

pub(crate) fn click_force_label(force: ClickForce) -> &'static str {
    match force {
        ClickForce::Low => "Low",
        ClickForce::Medium => "Medium",
        ClickForce::High => "High",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BatteryState, CHARGE_SPEEDS, ChargeFlow, HAPTIC_INTENSITY_LEVELS, MIN_CUSTOM_CHARGE_MA,
        NO_CHARGE_CURRENT_LIMIT, NO_CHARGE_LIMIT, PowerLedLevel, battery_summary, capacity,
        charge_direction, charge_flow_label, charge_limit_labels, charge_limit_percent,
        charge_limit_position, charge_speed_labels, charge_speed_milliamps, charge_speed_position,
        power_label, power_led_row_rank, retention_label, scale_milliamps, touchscreen_labels,
        touchscreen_position, touchscreen_state, volts, watt_hours, with_custom_row,
    };

    /// One row is the off row, and it is the one that sends the ceiling that
    /// isn't one. The label and the toast branch on the same constant, so
    /// what this catches is a preset list where that constant sits on no row
    /// at all — the two would then agree with each other and with nothing on
    /// screen.
    #[test]
    fn the_off_row_is_the_one_that_sends_no_limit() {
        let row = charge_limit_position(NO_CHARGE_LIMIT).expect("the off row is a preset");
        assert_eq!(charge_limit_percent(row), NO_CHARGE_LIMIT);
        assert_eq!(charge_limit_labels()[row], "Off");
    }

    /// The match is exhaustive, so being ranked at all is the compiler's
    /// business; being ranked apart is this. Two levels sharing a rank would
    /// fall back to whichever the vocabulary lists first, which is the
    /// inheritance the rank exists to stop.
    #[test]
    fn no_two_power_led_levels_share_a_row() {
        let mut ranks = PowerLedLevel::ALL.map(power_led_row_rank).to_vec();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(ranks.len(), PowerLedLevel::ALL.len());
    }

    /// The steps are the touchpad's list, not this module's, and the rows are
    /// those steps in order — so a scale that stopped climbing would be drawn
    /// as one anyway. What this catches is `wire`'s copy being updated to a
    /// reordered upstream list; that the copy matches upstream at all is the
    /// daemon's test, one boundary over.
    #[test]
    fn the_haptic_steps_climb() {
        assert!(HAPTIC_INTENSITY_LEVELS.is_sorted_by(|low, high| low < high));
    }

    /// The row a state marks is the row that sends it back. A menu picks by
    /// position, so this is the only thing standing between the mark and the
    /// write — reorder the labels and both move together, or this fails.
    #[test]
    fn a_touchscreen_row_sends_the_state_it_is_marked_for() {
        for enabled in [true, false] {
            let row = touchscreen_position(enabled).expect("both states are listed");
            assert_eq!(touchscreen_state(row), Some(enabled));
        }
        assert_eq!(touchscreen_labels().len(), 2);
        assert_eq!(touchscreen_state(2), None);
    }

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

    /// A 4640 mAh pack, the Laptop 13's.
    const CAPACITY: u32 = 4640;

    /// Mid-charge on the same pack's four cells.
    const MILLIVOLTS: u32 = 15_400;

    /// What that pack is rated at, which is what its energy is measured
    /// against however charged it happens to be.
    const NOMINAL_MILLIVOLTS: u32 = 15_640;

    fn state(flow: ChargeFlow, milliamps: u32) -> BatteryState {
        BatteryState {
            percent: 62,
            flow,
            milliamps,
            millivolts: MILLIVOLTS,
        }
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

    #[test]
    fn full_speed_lifts_the_limit_rather_than_naming_the_pack_rate() {
        // Sending the capacity would install a real cap at 1C; the EC only
        // stops clamping when the limit is the maximum.
        assert_eq!(charge_speed_milliamps(CAPACITY, 0), NO_CHARGE_CURRENT_LIMIT);
    }

    #[test]
    fn presets_are_fractions_of_the_pack_rate() {
        assert_eq!(charge_speed_milliamps(CAPACITY, 1), 2320);
        assert_eq!(charge_speed_milliamps(CAPACITY, 2), 1160);
    }

    #[test]
    fn a_preset_round_trips_to_its_own_row() {
        for index in 0..CHARGE_SPEEDS.len() {
            let milliamps = charge_speed_milliamps(CAPACITY, index);
            assert_eq!(charge_speed_position(CAPACITY, milliamps), Some(index));
        }
    }

    #[test]
    fn a_dialled_in_value_matches_no_preset() {
        assert_eq!(charge_speed_position(CAPACITY, 1500), None);
    }

    /// A `GtkScale` is continuous while dragged, so without snapping a drag
    /// lands on values like 984 mA that the row then displays as "1.0 A".
    #[test]
    fn the_slider_snaps_to_whole_steps() {
        assert_eq!(scale_milliamps(984.0), 1000);
        assert_eq!(scale_milliamps(1049.0), 1000);
        assert_eq!(scale_milliamps(1050.0), 1100);
    }

    #[test]
    fn the_slider_never_asks_for_a_current_that_stops_charging() {
        let floor = scale_milliamps(MIN_CUSTOM_CHARGE_MA);
        assert!(floor > 0);
        assert_eq!(scale_milliamps(0.0), floor);
        assert_eq!(scale_milliamps(-50.0), floor);
    }

    #[test]
    fn labels_name_the_rate_and_end_with_the_custom_row() {
        let labels = with_custom_row(charge_speed_labels(CAPACITY));
        assert_eq!(labels.len(), CHARGE_SPEEDS.len() + 1);
        assert_eq!(labels[0], "Full speed");
        assert_eq!(labels[1], "Half (2.3 A)");
        assert_eq!(labels[CHARGE_SPEEDS.len()], "Custom");
    }
}
