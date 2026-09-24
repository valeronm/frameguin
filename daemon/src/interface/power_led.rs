//! `io.github.valeronm.Frameguin1.PowerLed`.

use frameguin_hardware::device::power_led::PowerLed;
use frameguin_wire::{self as wire, PowerLedControl};
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.PowerLed")]
impl Served<PowerLed> {
    async fn get_brightness(&self) -> fdo::Result<(u8, wire::PowerLedLevel)> {
        Ok(self.device().brightness().await?)
    }

    async fn get_levels(&self) -> fdo::Result<Vec<wire::PowerLedLevel>> {
        Ok(self.device().levels().await?)
    }

    async fn set_level(
        &self,
        level: wire::PowerLedLevel,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        Ok(self.authorized(&header).await?.set_level(level).await?)
    }

    async fn set_brightness(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        Ok(self
            .authorized(&header)
            .await?
            .set_brightness(percent)
            .await?)
    }
}
