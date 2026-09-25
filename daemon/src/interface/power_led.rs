//! `io.github.valeronm.Frameguin1.PowerLed`.

use frameguin_contract::{self as contract, PowerLedControl};
use frameguin_hardware::device::power_led::PowerLed;
use frameguin_wire::fdo_error;
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.PowerLed")]
impl Served<PowerLed> {
    async fn get_brightness(&self) -> fdo::Result<(u8, contract::PowerLedLevel)> {
        self.device().brightness().await.map_err(fdo_error)
    }

    async fn get_levels(&self) -> fdo::Result<Vec<contract::PowerLedLevel>> {
        self.device().levels().await.map_err(fdo_error)
    }

    async fn set_level(
        &self,
        level: contract::PowerLedLevel,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.authorized(&header)
            .await?
            .set_level(level)
            .await
            .map_err(fdo_error)
    }

    async fn set_brightness(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.authorized(&header)
            .await?
            .set_brightness(percent)
            .await
            .map_err(fdo_error)
    }
}
