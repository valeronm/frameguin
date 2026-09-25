//! `io.github.valeronm.Frameguin1.ChargingLed`.

use frameguin_contract::{ChargingLedControl, ChargingLedFeature, ChargingLedSide};
use frameguin_hardware::device::charging_led::ChargingLed;
use frameguin_wire::fdo_error;
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.ChargingLed")]
impl Served<ChargingLed> {
    async fn get_enabled(&self) -> fdo::Result<bool> {
        self.device().enabled().await.map_err(fdo_error)
    }

    async fn set_enabled(
        &self,
        enabled: bool,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.authorized(&header)
            .await?
            .set_enabled(enabled)
            .await
            .map_err(fdo_error)
    }

    async fn get_features(&self) -> fdo::Result<Vec<ChargingLedFeature>> {
        self.device().features().await.map_err(fdo_error)
    }

    async fn get_side(&self) -> fdo::Result<ChargingLedSide> {
        self.device().side().await.map_err(fdo_error)
    }
}
