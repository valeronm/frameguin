//! The touch panel: one switch.

use std::rc::Rc;

use frameguin_contract::{DeviceResult as Result, TouchscreenControl};

use super::present;

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

#[cfg(test)]
mod tests {
    use frameguin_contract::DeviceError;

    use super::Touchscreen;
    use crate::testing::{Machine, absent, ready};

    #[test]
    fn a_panel_the_hardware_answers_for_is_detected() {
        assert!(
            ready(Touchscreen::detect(&Machine::new()))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn a_panel_the_hardware_does_not_serve_is_absent() {
        let machine = Machine::failing(absent());
        assert!(ready(Touchscreen::detect(&machine)).unwrap().is_none());
    }

    /// A refusal from a device that is there says nothing about presence.
    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_panel() {
        let error = DeviceError::Failed("no reply".into());
        let machine = Machine::failing(error.clone());
        assert_eq!(ready(Touchscreen::detect(&machine)).err(), Some(error));
    }

    #[test]
    fn a_write_reaches_the_hardware_and_a_read_sees_it() {
        let machine = Machine::new();
        let touchscreen = Touchscreen::new(machine.clone());
        ready(touchscreen.set_enabled(false)).unwrap();
        assert!(!machine.enabled.get());
        assert_eq!(ready(touchscreen.read()), Ok(false));
    }

    #[test]
    fn a_refused_write_carries_the_refusal() {
        let machine = Machine::new();
        let touchscreen = Touchscreen::new(machine.clone());
        machine.touchscreen.refuse();
        assert_eq!(
            ready(touchscreen.set_enabled(false)),
            Err(DeviceError::AccessDenied("not authorized".into()))
        );
        assert!(machine.enabled.get());
    }
}
