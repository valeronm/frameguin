//! The privacy switches: one read.

use std::rc::Rc;

use frameguin_contract::{DeviceResult as Result, PrivacyState, PrivacySwitchesControl};

use super::present;

pub struct PrivacySwitches<C> {
    control: Rc<C>,
}

impl<C: PrivacySwitchesControl> PrivacySwitches<C> {
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        Ok(present(control.switches().await)?.map(|_| Self {
            control: control.clone(),
        }))
    }

    pub async fn read(&self) -> Result<PrivacyState> {
        self.control.switches().await
    }
}
