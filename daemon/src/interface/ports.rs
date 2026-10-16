//! `io.github.valeronm.Frameguin1.Ports`.

use frameguin_contract::{PortSet, PortState, PortsControl};
use frameguin_hardware::device::ports::Ports;
use frameguin_wire::fdo_error;
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;

use crate::served::Served;

#[interface(name = "io.github.valeronm.Frameguin1.Ports")]
impl Served<Ports> {
    async fn get_ports(&self, controller_ports: PortSet) -> fdo::Result<Vec<PortState>> {
        self.device()
            .ports(controller_ports)
            .await
            .map_err(fdo_error)
    }

    async fn get_switchable(&self) -> fdo::Result<PortSet> {
        self.device().switchable().await.map_err(fdo_error)
    }

    async fn set_enabled(
        &self,
        port: u8,
        enabled: bool,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.authorized(&header)
            .await?
            .set_enabled(port, enabled)
            .await
            .map_err(fdo_error)
    }
}
