//! `io.github.valeronm.Frameguin1.PrivacySwitches`.

use frameguin_hardware::device::privacy_switches::PrivacySwitches;
use frameguin_wire::{PrivacyState, PrivacySwitchesControl};
use zbus::fdo;
use zbus::interface;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.PrivacySwitches")]
impl Served<PrivacySwitches> {
    /// No polkit check: reading the switches sets nothing.
    async fn get_switches(&self) -> fdo::Result<PrivacyState> {
        Ok(self.device().switches().await?)
    }
}
