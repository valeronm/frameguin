//! The root interface, and what puts a detected machine on a connection.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use frameguin_contract::Board;
use frameguin_hardware::device::Detected;
use frameguin_hardware::part::Identity;
use frameguin_hardware::restore::Restore;
use frameguin_wire::{BUS_NAME, fdo_error};
use zbus::message::Header;
use zbus::object_server::ObjectServer;
use zbus::{Connection, fdo, interface};
use zbus_polkit::policykit1::AuthorityProxy;

use crate::interface::{self, Op};
use crate::service::Service;

pub(crate) struct Daemon {
    pub(crate) service: Arc<Service>,
    pub(crate) board: Board,
    /// Every part detection found at startup, which is the one time it looks.
    pub(crate) parts: Vec<Identity>,
    pub(crate) restore: Restore,
}

#[interface(name = "io.github.valeronm.Frameguin1")]
impl Daemon {
    /// The inventory: every device that is a part, whether or not it is
    /// also a control.
    fn get_devices(&self) -> Vec<Identity> {
        self.service.touch();
        self.parts.clone()
    }

    /// Answers on any hardware, the vendor saying whether it is this
    /// hardware at all.
    fn get_board(&self) -> Board {
        self.service.touch();
        self.board.clone()
    }

    fn get_restore(&self) -> bool {
        self.service.touch();
        self.restore.enabled()
    }

    async fn set_restore(
        &self,
        enabled: bool,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> fdo::Result<()> {
        self.service.authorize(&header).await?;
        if self.restore.enabled() == enabled {
            return Ok(());
        }
        self.restore.set_enabled(enabled);
        // A device that cannot be read is in the journal and the switch is
        // on regardless: nothing here is the caller's to act on.
        if enabled {
            let _ = interface::each_restorable(server, Op::Remember).await;
        }
        Ok(())
    }

    /// A mirror is a claim about the hardware rather than a wanted value,
    /// which the switch does not govern.
    async fn restore(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> fdo::Result<()> {
        self.service.authorize(&header).await?;
        let resent = interface::resend_mirrors(server).await;
        let restored = if self.restore.enabled() {
            interface::each_restorable(server, Op::Restore).await
        } else {
            Ok(())
        };
        resent.and(restored).map_err(fdo_error)
    }

    /// The daemon's version and the path it was started from. The path is the
    /// diagnostic: two install trees can hold the same version, and which
    /// daemon runs is decided by the D-Bus activation file rather than by
    /// PATH. Answers without touching the EC, so it works on any hardware.
    fn get_build(&self) -> (String, String) {
        self.service.touch();
        let exe = std::fs::read_link("/proc/self/exe").unwrap_or_else(|_| "unknown".into());
        (
            env!("CARGO_PKG_VERSION").to_string(),
            exe.display().to_string(),
        )
    }
}

/// The name is claimed only once the objects are served, so an activating
/// client can't call into a not-yet-registered interface.
pub(crate) async fn serve(
    conn: &Connection,
    last_used: Arc<Mutex<Instant>>,
    detected: Detected,
) -> zbus::Result<()> {
    let Detected {
        board,
        devices,
        parts,
        restore,
    } = detected;
    let authority = AuthorityProxy::new(conn)
        .await
        .map_err(|e| zbus::Error::Failure(e.to_string()))?;
    let daemon = Daemon {
        service: Arc::new(Service::new(authority, last_used)),
        board,
        parts,
        restore,
    };
    interface::serve_all(conn.object_server(), daemon, devices).await?;
    conn.request_name(BUS_NAME).await?;
    // The line a start that hung between detection and the bus lacks,
    // which is what tells it apart from one that hung in detection.
    eprintln!("serving {BUS_NAME}");
    Ok(())
}
