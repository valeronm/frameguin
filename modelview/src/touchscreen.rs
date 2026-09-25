//! The touch panel's switch, named as two states for a menu.

use crate::rows::names;

/// The two states as a menu names them, each beside the state its row means.
///
/// One array rather than a list of labels and an index constant beside it: a
/// menu row is picked by position, so the pairing is what a click depends on,
/// and spelling it in two places is what lets a reordering mark one row while
/// writing the other.
const STATES: [(&str, bool); 2] = [("Off", false), ("On", true)];

#[must_use]
pub fn state_labels() -> Vec<String> {
    names(&STATES)
}

/// Which row a state sits on, for marking the group.
#[must_use]
pub fn state_row(enabled: bool) -> Option<usize> {
    STATES.iter().position(|(_, state)| *state == enabled)
}

/// What a row means, for sending it. None for a row nothing is listed at,
/// which a group drawn from these labels cannot produce.
#[must_use]
pub fn state_at(row: usize) -> Option<bool> {
    STATES.get(row).map(|(_, state)| *state)
}

#[cfg(test)]
mod tests {
    use super::{state_at, state_labels, state_row};

    #[test]
    fn a_row_sends_the_state_it_is_marked_for() {
        for enabled in [true, false] {
            let row = state_row(enabled).expect("both states are listed");
            assert_eq!(state_at(row), Some(enabled));
        }
        assert_eq!(state_labels().len(), 2);
        assert_eq!(state_at(2), None);
    }
}
