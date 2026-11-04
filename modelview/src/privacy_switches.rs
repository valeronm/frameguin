//! The privacy switches: the words for their read.

use frameguin_contract::PrivacyState;

use crate::words::joined;

/// Framework's own words for the two positions of a slider.
#[must_use]
pub fn switch_label(connected: bool) -> &'static str {
    if connected {
        "Connected"
    } else {
        "Disconnected"
    }
}

#[must_use]
pub fn switches_summary(switches: PrivacyState) -> String {
    let on = |connected| if connected { "on" } else { "off" };
    joined([
        format!("Camera {}", on(switches.camera)),
        format!("Microphone {}", on(switches.microphone)),
    ])
}

#[cfg(test)]
mod tests {
    use frameguin_contract::PrivacyState;

    use super::{switch_label, switches_summary};

    #[test]
    fn a_switch_is_worded_as_the_device_it_leaves_connected() {
        assert_eq!(switch_label(true), "Connected");
        assert_eq!(switch_label(false), "Disconnected");
    }

    #[test]
    fn the_summary_names_both_devices_camera_first() {
        let switches = PrivacyState {
            camera: true,
            microphone: false,
        };
        assert_eq!(switches_summary(switches), "Camera on · Microphone off");
    }
}
