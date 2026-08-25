//! The values the controls offer and the words for them: the presets, the
//! percentages and currents behind them, and how each is named. Nothing here
//! reaches GTK or the bus, which is what lets the window and the tray share
//! it and so keeps them from disagreeing about what "Half" sends or what a
//! ceiling of 100% is called.

use frameguin_wire::{
    BatteryState, ChargeFlow, ClickForce, FpLevel, HAPTIC_INTENSITY_LEVELS, NO_CHARGE_CURRENT_LIMIT,
};

const CHARGE_PRESETS: [u8; 3] = [60, 80, 100];

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

/// Which way charge is moving, with the rate where there is one to name. A
/// rate of zero is dropped rather than rendered: "0.0 A" is what a pack
/// reports in the moment either side of a direction changing, and it reads
/// as a fault.
///
/// A rate arriving under `Idle` names the charge ending rather than nothing
/// at all. That pairing has one source: the daemon reaches `Idle` with a rate
/// only where the EC claims neither direction, which is the state its charge
/// limiter holds the pack in while the current falls away. A pack simply
/// resting at its ceiling reports a clean zero, so the two never collide.
pub(crate) fn charge_flow_label(state: BatteryState) -> String {
    let direction = match state.flow {
        ChargeFlow::Charging => "Charging",
        ChargeFlow::Discharging => "Discharging",
        ChargeFlow::Idle if state.milliamps > 0 => "Finishing charge",
        ChargeFlow::Idle => return "Plugged in, not charging".to_string(),
    };
    if state.milliamps == 0 {
        return direction.to_string();
    }
    let rate = amps(state.milliamps);
    match watts(state) {
        Some(watts) => format!("{direction} at {rate} ({watts})"),
        None => format!("{direction} at {rate}"),
    }
}

/// The whole state on one line, for the tray, which has no second line to
/// put the charge on.
pub(crate) fn battery_summary(state: BatteryState) -> String {
    format!("{}% · {}", state.percent, charge_flow_label(state))
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
        // A 100% ceiling is no limit at all — say so.
        .map(|percent| {
            if *percent == 100 {
                "No limit".to_string()
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

fn fp_level_label(level: FpLevel) -> &'static str {
    match level {
        FpLevel::Auto => "Auto",
        FpLevel::High => "High",
        FpLevel::Medium => "Medium",
        FpLevel::Low => "Low",
        FpLevel::UltraLow => "Ultra-low",
        FpLevel::Off => "Off",
        FpLevel::Custom => "Custom",
    }
}

pub(crate) fn fp_level_labels(levels: &[FpLevel]) -> Vec<String> {
    levels
        .iter()
        .map(|level| fp_level_label(*level).into())
        .collect()
}

/// The haptic combo's rows, derived from the steps they select rather than
/// kept in step with them by hand. The steps live in `wire` now, so the two
/// lists can no longer be edited together, and a firmware generation that
/// adds one would leave it unlabelled and unreachable.
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
        BatteryState, CHARGE_SPEEDS, ChargeFlow, MIN_CUSTOM_CHARGE_MA, NO_CHARGE_CURRENT_LIMIT,
        battery_summary, charge_flow_label, charge_speed_labels, charge_speed_milliamps,
        charge_speed_position, scale_milliamps, with_custom_row,
    };

    /// A 4640 mAh pack, the Laptop 13's.
    const CAPACITY: u32 = 4640;

    /// Mid-charge on the same pack's four cells.
    const MILLIVOLTS: u32 = 15_400;

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
