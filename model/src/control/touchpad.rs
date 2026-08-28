//! The haptic touchpad: two settings, each picked from a short list.

use std::rc::Rc;

use frameguin_wire::{
    ClickForce, DeviceResult as Result, HAPTIC_INTENSITY_LEVELS, TouchpadControl,
};

use super::present;

/// What the pad is set to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub haptic_intensity: u8,
    pub click_force: ClickForce,
}

pub struct Touchpad<C> {
    control: Rc<C>,
}

impl<C: TouchpadControl> Touchpad<C> {
    pub fn new(control: Rc<C>) -> Self {
        Self { control }
    }

    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        Ok(present(control.haptic_intensity().await)?.map(|_| Self::new(control.clone())))
    }

    /// Both settings, or neither: a front-end fills its rows from one answer.
    pub async fn read(&self) -> Result<Snapshot> {
        Ok(Snapshot {
            haptic_intensity: self.control.haptic_intensity().await?,
            click_force: self.control.click_force().await?,
        })
    }

    pub async fn set_haptic_intensity(&self, percent: u8) -> Result<()> {
        self.control.set_haptic_intensity(percent).await
    }

    pub async fn set_click_force(&self, force: ClickForce) -> Result<()> {
        self.control.set_click_force(force).await
    }
}

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
                format!("{percent}%")
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
    use std::cell::Cell;
    use std::rc::Rc;

    use frameguin_wire::{
        ClickForce, DeviceError, DeviceResult as Result, HAPTIC_INTENSITY_LEVELS, TouchpadControl,
    };

    use super::{Snapshot, Touchpad, haptic_at, haptic_row};
    use crate::testing::{Fault, ready};

    /// A pad that answers what it was built with.
    struct Stub {
        haptic_intensity: Cell<u8>,
        click_force: Cell<ClickForce>,
        fault: Fault,
    }

    impl Stub {
        fn new() -> Rc<Self> {
            Self::with(Fault::default())
        }

        fn failing(error: DeviceError) -> Rc<Self> {
            Self::with(Fault::failing(error))
        }

        fn with(fault: Fault) -> Rc<Self> {
            Rc::new(Self {
                haptic_intensity: Cell::new(50),
                click_force: Cell::new(ClickForce::Low),
                fault,
            })
        }
    }

    impl TouchpadControl for Stub {
        async fn haptic_intensity(&self) -> Result<u8> {
            self.fault.read(self.haptic_intensity.get())
        }

        async fn set_haptic_intensity(&self, percent: u8) -> Result<()> {
            self.fault.write()?;
            self.haptic_intensity.set(percent);
            Ok(())
        }

        async fn click_force(&self) -> Result<ClickForce> {
            self.fault.read(self.click_force.get())
        }

        async fn set_click_force(&self, force: ClickForce) -> Result<()> {
            self.fault.write()?;
            self.click_force.set(force);
            Ok(())
        }
    }

    #[test]
    fn a_pad_the_hardware_answers_for_is_detected() {
        let stub = Stub::new();
        assert!(ready(Touchpad::detect(&stub)).unwrap().is_some());
    }

    #[test]
    fn a_pad_the_hardware_does_not_serve_is_absent() {
        let stub = Stub::failing(DeviceError::Absent("no such interface".into()));
        assert!(ready(Touchpad::detect(&stub)).unwrap().is_none());
    }

    /// A refusal from a device that is there — the hardware cannot do this,
    /// the daemon did not answer — says nothing about presence.
    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_pad() {
        for error in [
            DeviceError::Failed("no reply".into()),
            DeviceError::NotSupported("no pad on this board".into()),
        ] {
            let stub = Stub::failing(error.clone());
            assert_eq!(ready(Touchpad::detect(&stub)).err(), Some(error));
        }
    }

    #[test]
    fn a_read_takes_both_settings_from_the_hardware() {
        let touchpad = Touchpad::new(Stub::new());
        assert_eq!(
            ready(touchpad.read()),
            Ok(Snapshot {
                haptic_intensity: 50,
                click_force: ClickForce::Low
            })
        );
    }

    #[test]
    fn a_write_reaches_the_hardware() {
        let stub = Stub::new();
        let touchpad = Touchpad::new(stub.clone());
        ready(touchpad.set_click_force(ClickForce::High)).unwrap();
        assert_eq!(stub.click_force.get(), ClickForce::High);
    }

    #[test]
    fn a_refused_write_carries_the_refusal() {
        let stub = Stub::new();
        let touchpad = Touchpad::new(stub.clone());
        stub.fault.refuse();
        assert_eq!(
            ready(touchpad.set_haptic_intensity(100)),
            Err(DeviceError::AccessDenied("not authorized".into()))
        );
        assert_eq!(stub.haptic_intensity.get(), 50);
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

    #[test]
    fn a_haptic_row_sends_the_step_it_is_marked_for() {
        for &percent in &HAPTIC_INTENSITY_LEVELS {
            let row = haptic_row(percent).expect("every step has a row");
            assert_eq!(haptic_at(row), Some(percent));
        }
        assert_eq!(haptic_row(33), None);
    }
}
