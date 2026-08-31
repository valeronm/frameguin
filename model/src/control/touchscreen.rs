//! The touch panel: one switch, named as two states for a menu.

use std::rc::Rc;

use frameguin_wire::{DeviceResult as Result, TouchscreenControl};

use super::{names, present};

pub struct Touchscreen<C> {
    control: Rc<C>,
}

impl<C: TouchscreenControl> Touchscreen<C> {
    pub fn new(control: Rc<C>) -> Self {
        Self { control }
    }

    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        Ok(present(control.enabled().await)?.map(|_| Self::new(control.clone())))
    }

    pub async fn read(&self) -> Result<bool> {
        self.control.enabled().await
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<()> {
        self.control.set_enabled(enabled).await
    }
}

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
    use frameguin_wire::DeviceError;

    use super::{Touchscreen, state_at, state_labels, state_row};
    use crate::testing::{Board, absent, ready};

    #[test]
    fn a_panel_the_hardware_answers_for_is_detected() {
        assert!(ready(Touchscreen::detect(&Board::new())).unwrap().is_some());
    }

    #[test]
    fn a_panel_the_hardware_does_not_serve_is_absent() {
        let board = Board::failing(absent());
        assert!(ready(Touchscreen::detect(&board)).unwrap().is_none());
    }

    /// A refusal from a device that is there says nothing about presence.
    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_panel() {
        let error = DeviceError::Failed("no reply".into());
        let board = Board::failing(error.clone());
        assert_eq!(ready(Touchscreen::detect(&board)).err(), Some(error));
    }

    #[test]
    fn a_write_reaches_the_hardware_and_a_read_sees_it() {
        let board = Board::new();
        let touchscreen = Touchscreen::new(board.clone());
        ready(touchscreen.set_enabled(false)).unwrap();
        assert!(!board.enabled.get());
        assert_eq!(ready(touchscreen.read()), Ok(false));
    }

    #[test]
    fn a_refused_write_carries_the_refusal() {
        let board = Board::new();
        let touchscreen = Touchscreen::new(board.clone());
        board.touchscreen.refuse();
        assert_eq!(
            ready(touchscreen.set_enabled(false)),
            Err(DeviceError::AccessDenied("not authorized".into()))
        );
        assert!(board.enabled.get());
    }

    /// The row a state marks is the row that sends it back. A menu picks by
    /// position, so this is the only thing standing between the mark and the
    /// write — reorder the labels and both move together, or this fails.
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
