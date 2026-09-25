//! The battery's presets and the figures behind them: what a reading is
//! called is [`reading`]'s.
//!
//! Row order is settled here. A control that caps something starts at the
//! setting that caps nothing and tightens down the list; the row that
//! reveals a slider trails the presets it extends.

pub mod reading;

use std::num::NonZeroU32;

use frameguin_contract::ChargeCurrentLimit;
use frameguin_model::control::battery::{CUSTOM_CHARGE_STEP_MA, ChargeSpeeds, custom_charge_ma};

use crate::rows::{Custom, names, row_for};

const CHARGE_PRESETS: [u8; 3] = [100, 80, 60];

/// The ceiling that is no ceiling. A presentation fact rather than a wire
/// one: the daemon takes 100 and writes it to the EC like any other
/// percentage, and it is only here that the value stops being a limit and
/// starts being the absence of one.
pub const NO_CHARGE_LIMIT: u8 = 100;

/// The window's combo carries one row past the presets, for a ceiling the
/// user dials in; the tray offers the presets alone.
pub const CHARGE_LIMIT_CUSTOM: usize = CHARGE_PRESETS.len();

/// The charge speeds the combo offers, each beside the divisor it applies to
/// the battery's 1C design current; `None` is full speed, no limit at all.
const CHARGE_SPEEDS: [(&str, Option<u32>); 3] = [
    ("Full speed", None),
    ("Half", Some(2)),
    ("Quarter", Some(4)),
];

/// The window's combo carries one row past the presets, for a rate the user
/// dials in. The tray offers only the presets: a slider has no menu form, and
/// a preset menu that can't reach every state is the honest half.
pub const CHARGE_SPEED_CUSTOM: usize = CHARGE_SPEEDS.len();

/// The preset names without their rates, which are the same for every pack.
#[must_use]
pub fn charge_speed_names() -> Vec<String> {
    names(&CHARGE_SPEEDS)
}

/// The limit a row asks for; None for a row nothing is listed at, and for a
/// fraction that comes to zero.
#[must_use]
pub fn charge_speed_at(speeds: ChargeSpeeds, row: usize) -> Option<ChargeCurrentLimit> {
    match CHARGE_SPEEDS.get(row)? {
        (_, None) => Some(ChargeCurrentLimit::NoLimit),
        (_, Some(divisor)) => {
            NonZeroU32::new(speeds.design_capacity() / divisor).map(ChargeCurrentLimit::Limit)
        }
    }
}

/// Which preset row a limit sits on, and `None` when it matches no preset —
/// `framework_tool` can set any value, and guessing the nearest would
/// misreport it.
#[must_use]
pub fn charge_speed_preset_row(speeds: ChargeSpeeds, limit: ChargeCurrentLimit) -> Option<usize> {
    (0..CHARGE_SPEEDS.len()).find(|&row| charge_speed_at(speeds, row) == Some(limit))
}

/// Which row a limit shows on, Custom included.
#[must_use]
pub fn charge_speed_row(
    speeds: ChargeSpeeds,
    limit: ChargeCurrentLimit,
    selected: Option<usize>,
    custom: Custom,
) -> Option<usize> {
    row_for(
        charge_speed_preset_row(speeds, limit),
        Some(CHARGE_SPEED_CUSTOM),
        selected,
        custom,
    )
}

/// Labels carrying the rate each fraction works out to — "Half" alone
/// doesn't say half of what.
#[must_use]
pub fn charge_speed_labels(speeds: ChargeSpeeds) -> Vec<String> {
    CHARGE_SPEEDS
        .iter()
        .map(|(name, divisor)| match divisor {
            Some(divisor) => {
                format!(
                    "{name} ({})",
                    reading::amps(speeds.design_capacity() / divisor)
                )
            }
            None => (*name).to_string(),
        })
        .collect()
}

/// 1C is as fast as the pack ever asks, so a limit above it never binds.
/// Floored to the step the custom slider rounds to, so the far end of its
/// track sends what it shows.
#[must_use]
pub fn fastest_custom(speeds: ChargeSpeeds) -> NonZeroU32 {
    custom_charge_ma(speeds.design_capacity() / CUSTOM_CHARGE_STEP_MA * CUSTOM_CHARGE_STEP_MA)
}

/// The ceiling a preset row asks the daemon for; None for a row nothing is
/// listed at.
#[must_use]
pub fn charge_limit_at(row: usize) -> Option<u8> {
    CHARGE_PRESETS.get(row).copied()
}

/// Which preset row a ceiling sits on, and `None` when it matches none —
/// `framework_tool` can set any value, and guessing the nearest preset would
/// misreport it.
#[must_use]
pub fn charge_limit_preset_row(percent: u8) -> Option<usize> {
    CHARGE_PRESETS.iter().position(|preset| *preset == percent)
}

/// Which row the window's combo shows for a ceiling, Custom included.
#[must_use]
pub fn charge_limit_row(percent: u8, selected: Option<usize>, custom: Custom) -> Option<usize> {
    let preset = charge_limit_preset_row(percent);
    row_for(preset, Some(CHARGE_LIMIT_CUSTOM), selected, custom)
}

/// Preset names, shared so the window's combo and the tray's menu can't
/// disagree about what a ceiling is called. The window's combo appends
/// "Custom"; the tray's menu takes these as they are.
#[must_use]
pub fn charge_limit_labels() -> Vec<String> {
    CHARGE_PRESETS
        .iter()
        // Named as a state rather than as the absence of one, so a title
        // quoting the selected row still reads.
        .map(|percent| {
            if *percent == NO_CHARGE_LIMIT {
                "Off".to_string()
            } else {
                reading::percent_label(*percent)
            }
        })
        .collect()
}

/// A combo's rows: the presets, then the one that reveals a slider. Both of
/// the battery's combos build their model this way, so neither can leave the
/// extra row off and address it anyway.
#[must_use]
pub fn with_custom_row(mut labels: Vec<String>) -> Vec<String> {
    labels.push("Custom".to_string());
    labels
}

#[cfg(test)]
mod tests {
    use frameguin_contract::ChargeCurrentLimit;
    use frameguin_model::control::battery::{ChargeSpeeds, MIN_CUSTOM_CHARGE_MA};

    use super::{
        CHARGE_LIMIT_CUSTOM, CHARGE_SPEED_CUSTOM, CHARGE_SPEEDS, NO_CHARGE_LIMIT, charge_limit_at,
        charge_limit_labels, charge_limit_preset_row, charge_limit_row, charge_speed_at,
        charge_speed_labels, charge_speed_preset_row, charge_speed_row, fastest_custom,
        with_custom_row,
    };
    use crate::rows::Custom;
    use frameguin_model::fixtures::{SPEEDS, cap};

    #[test]
    fn the_off_row_is_the_one_that_sends_no_limit() {
        let row = charge_limit_preset_row(NO_CHARGE_LIMIT).expect("the off row is a preset");
        assert_eq!(charge_limit_at(row), Some(NO_CHARGE_LIMIT));
        assert_eq!(charge_limit_labels()[row], "Off");
    }

    #[test]
    fn full_speed_lifts_the_limit_rather_than_naming_the_pack_rate() {
        assert_eq!(
            charge_speed_at(SPEEDS, 0),
            Some(ChargeCurrentLimit::NoLimit)
        );
    }

    #[test]
    fn presets_are_fractions_of_the_pack_rate() {
        assert_eq!(charge_speed_at(SPEEDS, 1), Some(cap(2320)));
        assert_eq!(charge_speed_at(SPEEDS, 2), Some(cap(1160)));
    }

    #[test]
    fn a_fraction_that_comes_to_zero_sends_nothing() {
        let tiny = ChargeSpeeds::new(3);
        assert_eq!(charge_speed_at(tiny, 2), None);
    }

    #[test]
    fn the_fastest_custom_speed_is_one_c_floored_to_the_step() {
        assert_eq!(fastest_custom(SPEEDS).get(), 4600);
        let tiny = ChargeSpeeds::new(50);
        assert_eq!(fastest_custom(tiny), MIN_CUSTOM_CHARGE_MA);
    }

    #[test]
    fn a_preset_round_trips_to_its_own_row() {
        for row in 0..CHARGE_SPEEDS.len() {
            let limit = charge_speed_at(SPEEDS, row).expect("every preset has a row");
            assert_eq!(charge_speed_preset_row(SPEEDS, limit), Some(row));
        }
        assert_eq!(charge_speed_at(SPEEDS, CHARGE_SPEEDS.len()), None);
    }

    #[test]
    fn a_dialled_in_value_matches_no_preset() {
        assert_eq!(charge_speed_preset_row(SPEEDS, cap(1500)), None);
    }

    #[test]
    fn each_combo_holds_its_own_custom_row() {
        let preset = charge_limit_at(1).expect("the second row is a preset");
        let on_custom = Some(CHARGE_LIMIT_CUSTOM);
        assert_eq!(charge_limit_row(preset, on_custom, Custom::Keep), on_custom);
        assert_eq!(
            charge_limit_row(preset, on_custom, Custom::Rederive),
            Some(1)
        );

        let half = charge_speed_at(SPEEDS, 1).expect("half is a preset");
        let on_custom = Some(CHARGE_SPEED_CUSTOM);
        assert_eq!(
            charge_speed_row(SPEEDS, half, on_custom, Custom::Keep),
            on_custom
        );
        assert_eq!(
            charge_speed_row(SPEEDS, half, on_custom, Custom::Rederive),
            Some(1)
        );
    }

    #[test]
    fn labels_name_the_rate_and_end_with_the_custom_row() {
        let labels = with_custom_row(charge_speed_labels(SPEEDS));
        assert_eq!(labels.len(), CHARGE_SPEEDS.len() + 1);
        assert_eq!(labels[0], "Full speed");
        assert_eq!(labels[1], "Half (2.3 A)");
        assert_eq!(labels[CHARGE_SPEEDS.len()], "Custom");
    }
}
