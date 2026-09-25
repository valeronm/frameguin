//! The camera and microphone privacy switches, read and never set.
//!
//! Detection vouches for the command and not for what the pins it reads carry:
//! the EC answers a level whatever a board wires to them.

use std::sync::Arc;

use frameguin_contract::{DeviceResult, PrivacyState, PrivacySwitchesControl};

use crate::ec::{Ec, PrivacyEc};

pub struct PrivacySwitches {
    ec: Arc<dyn PrivacyEc>,
}

impl PrivacySwitches {
    pub(crate) fn detect(ec: &Arc<Ec>) -> Option<Self> {
        Self::new(ec.clone())
    }

    /// None where the EC refuses the read, which is how firmware without the
    /// command answers.
    pub fn new(ec: Arc<dyn PrivacyEc>) -> Option<Self> {
        ec.privacy_switches().ok().map(|_| Self { ec })
    }
}

impl PrivacySwitchesControl for PrivacySwitches {
    async fn switches(&self) -> DeviceResult<PrivacyState> {
        self.ec.privacy_switches()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::PrivacySwitches;
    use crate::testing::Sliders;

    #[test]
    fn an_ec_refusing_the_command_has_no_switches() {
        let ec = Sliders {
            refusing: true,
            ..Sliders::default()
        };
        assert!(PrivacySwitches::new(Arc::new(ec)).is_none());
    }
}
