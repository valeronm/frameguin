//! `io.github.valeronm.Frameguin1.PrivacySwitches`.

use frameguin_contract::{PrivacyState, PrivacySwitchesControl};
use frameguin_hardware::device::privacy_switches::PrivacySwitches;
use frameguin_wire::fdo_error;
use zbus::fdo;
use zbus::interface;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.PrivacySwitches")]
impl Served<PrivacySwitches> {
    /// No polkit check: reading the switches sets nothing.
    async fn get_switches(&self) -> fdo::Result<PrivacyState> {
        self.device().switches().await.map_err(fdo_error)
    }
}
