//! `io.github.valeronm.Frameguin1.ChargingLed`.

use frameguin_hardware::device::charging_led::ChargingLed;
use frameguin_wire::{ChargingLedControl, ChargingLedFeature, ChargingLedSide};
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.ChargingLed")]
impl Served<ChargingLed> {
    async fn get_enabled(&self) -> fdo::Result<bool> {
        Ok(self.device().enabled().await?)
    }

    async fn set_enabled(
        &self,
        enabled: bool,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        Ok(self.authorized(&header).await?.set_enabled(enabled).await?)
    }

    async fn get_features(&self) -> fdo::Result<Vec<ChargingLedFeature>> {
        Ok(self.device().features().await?)
    }

    async fn get_side(&self) -> fdo::Result<ChargingLedSide> {
        Ok(self.device().side().await?)
    }
}
