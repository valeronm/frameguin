//! The haptic touchpad's two settings, each picked from a short list.

use frameguin_contract::{ClickForce, HAPTIC_INTENSITY_LEVELS};

use crate::battery::reading::percent_label;

/// The haptic combo's rows, derived from the steps they select rather than
/// kept in step with them by hand, which a step added upstream would break
/// silently.
#[must_use]
pub fn haptic_labels() -> Vec<String> {
    HAPTIC_INTENSITY_LEVELS
        .iter()
        .map(|&percent| {
            if percent == 0 {
                "Off".to_string()
            } else {
                percent_label(percent)
            }
        })
        .collect()
}

/// Which row an intensity sits on; None for one not among the steps.
#[must_use]
pub fn haptic_row(percent: u8) -> Option<usize> {
    HAPTIC_INTENSITY_LEVELS.iter().position(|&p| p == percent)
}

/// The intensity a row sends; None for a row nothing is listed at.
#[must_use]
pub fn haptic_at(row: usize) -> Option<u8> {
    HAPTIC_INTENSITY_LEVELS.get(row).copied()
}

#[must_use]
pub fn click_force_label(force: ClickForce) -> &'static str {
    match force {
        ClickForce::Low => "Low",
        ClickForce::Medium => "Medium",
        ClickForce::High => "High",
    }
}

/// The click force combo's rows, lightest to firmest.
#[must_use]
pub fn click_force_labels() -> Vec<String> {
    ClickForce::ALL
        .iter()
        .map(|&force| click_force_label(force).to_string())
        .collect()
}

#[must_use]
pub fn click_force_row(force: ClickForce) -> Option<usize> {
    ClickForce::ALL.iter().position(|&f| f == force)
}

#[must_use]
pub fn click_force_at(row: usize) -> Option<ClickForce> {
    ClickForce::ALL.get(row).copied()
}

#[cfg(test)]
mod tests {
    use frameguin_contract::HAPTIC_INTENSITY_LEVELS;

    use super::{haptic_at, haptic_row};

    #[test]
    fn the_haptic_steps_climb() {
        assert!(HAPTIC_INTENSITY_LEVELS.is_sorted_by(|low, high| low < high));
    }

    #[test]
    fn a_haptic_row_sends_the_step_it_is_marked_for() {
        for &percent in &HAPTIC_INTENSITY_LEVELS {
            let row = haptic_row(percent).expect("every step has a row");
            assert_eq!(haptic_at(row), Some(percent));
        }
        assert_eq!(haptic_row(33), None);
    }
}
