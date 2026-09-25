//! `io.github.valeronm.Frameguin1.Touchscreen`.

use frameguin_contract::TouchscreenControl;
use frameguin_hardware::device::touchscreen::Touchscreen;
use frameguin_wire::fdo_error;
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Touchscreen")]
impl Served<Touchscreen> {
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
}
