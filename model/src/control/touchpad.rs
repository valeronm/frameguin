//! The haptic touchpad: two settings, each picked from a short list.

use std::rc::Rc;

use frameguin_contract::{ClickForce, DeviceResult as Result, TouchpadControl};

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

#[cfg(test)]
mod tests {
    use frameguin_contract::{ClickForce, DeviceError};

    use super::{Snapshot, Touchpad};
    use crate::testing::{Machine, absent, ready};

    #[test]
    fn a_pad_the_hardware_answers_for_is_detected() {
        assert!(ready(Touchpad::detect(&Machine::new())).unwrap().is_some());
    }

    #[test]
    fn a_pad_the_hardware_does_not_serve_is_absent() {
        let machine = Machine::failing(absent());
        assert!(ready(Touchpad::detect(&machine)).unwrap().is_none());
    }

    /// A refusal from a device that is there — the hardware cannot do this,
    /// the daemon did not answer — says nothing about presence.
    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_pad() {
        for error in [
            DeviceError::Failed("no reply".into()),
            DeviceError::NotSupported("no pad on this board".into()),
        ] {
            let machine = Machine::failing(error.clone());
            assert_eq!(ready(Touchpad::detect(&machine)).err(), Some(error));
        }
    }

    #[test]
    fn a_read_takes_both_settings_from_the_hardware() {
        let touchpad = Touchpad::new(Machine::new());
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
        let machine = Machine::new();
        let touchpad = Touchpad::new(machine.clone());
        ready(touchpad.set_click_force(ClickForce::High)).unwrap();
        assert_eq!(machine.click_force.get(), ClickForce::High);
    }

    #[test]
    fn a_refused_write_carries_the_refusal() {
        let machine = Machine::new();
        let touchpad = Touchpad::new(machine.clone());
        machine.touchpad.refuse();
        assert_eq!(
            ready(touchpad.set_haptic_intensity(100)),
            Err(DeviceError::AccessDenied("not authorized".into()))
        );
        assert_eq!(machine.haptic_intensity.get(), 50);
    }
}
