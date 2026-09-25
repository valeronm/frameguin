//! How a reading picks its row in a combo that has a Custom row.

/// What a reading landing on a preset should do to a combo sitting on its
/// Custom row. `Keep` is for a value the user is dialling in, where a number
/// that happens to equal a preset would fold the slider away under them;
/// `Rederive` takes the row the reading itself names.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Custom {
    Keep,
    Rederive,
}

/// Which row a reading belongs on, given where the combo sits now. A reading
/// no preset covers can only be shown by the slider, so it lands on the
/// Custom row of a control that has one.
#[must_use]
pub fn row_for(
    preset: Option<usize>,
    custom_row: Option<usize>,
    selected: Option<usize>,
    custom: Custom,
) -> Option<usize> {
    let Some(custom_row) = custom_row else {
        return preset;
    };
    if custom == Custom::Keep && selected == Some(custom_row) {
        return Some(custom_row);
    }
    preset.or(Some(custom_row))
}

/// The names off a table of rows kept as (name, value) pairs, in row order.
pub fn names<T>(rows: &[(&str, T)]) -> Vec<String> {
    rows.iter().map(|(name, _)| (*name).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::{Custom, row_for};

    const PRESET: usize = 1;
    const CUSTOM: usize = 3;

    #[test]
    fn a_value_dialled_in_onto_a_preset_stays_on_the_custom_row() {
        let on_custom = Some(CUSTOM);
        assert_eq!(
            row_for(Some(PRESET), on_custom, on_custom, Custom::Keep),
            Some(CUSTOM)
        );
        assert_eq!(
            row_for(Some(PRESET), on_custom, on_custom, Custom::Rederive),
            Some(PRESET)
        );
    }

    #[test]
    fn a_combo_on_a_preset_is_never_held_there_by_a_keep() {
        assert_eq!(
            row_for(Some(PRESET), Some(CUSTOM), Some(0), Custom::Keep),
            Some(PRESET)
        );
    }

    #[test]
    fn a_reading_no_preset_covers_lands_on_the_custom_row() {
        assert_eq!(
            row_for(None, Some(CUSTOM), Some(0), Custom::Rederive),
            Some(CUSTOM)
        );
    }

    #[test]
    fn a_control_with_no_custom_row_keeps_nothing() {
        assert_eq!(
            row_for(Some(PRESET), None, None, Custom::Keep),
            Some(PRESET)
        );
        assert_eq!(row_for(None, None, None, Custom::Keep), None);
    }
}
