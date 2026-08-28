//! `io.github.valeronm.Frameguin1.Touchscreen`.

use frameguin_hardware::device::touchscreen::Touchscreen;
use frameguin_wire::TouchscreenControl;
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Touchscreen")]
impl Served<Touchscreen> {
    async fn get_enabled(&self) -> fdo::Result<bool> {
        Ok(self.device().enabled().await?)
    }

    async fn set_enabled(
        &self,
        enabled: bool,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        // Skipped only where a reading can say the value is already there;
        // the route with none never skips.
        if self.device().reading()? == Some(enabled) {
            return Ok(());
        }
        self.authorize(&header).await?;
        Ok(self.device().set_enabled(enabled).await?)
    }
}
