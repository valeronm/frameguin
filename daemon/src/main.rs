//! System D-Bus daemon exposing privileged Framework laptop controls.
//!
//! Owns io.github.valeronm.Frameguin on the system bus and reaches the
//! machine through `frameguin_hardware`. Setters require the polkit
//! action io.github.valeronm.frameguin.manage. Exits after 5 idle
//! minutes; D-Bus activation restarts it on demand.

mod interface;
mod served;
mod service;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use frameguin_hardware::device::{self, Detected};
use frameguin_hardware::part::Identity;
use frameguin_hardware::restore::Restore;
use frameguin_wire as wire;
use zbus::message::Header;
use zbus::object_server::ObjectServer;
use zbus::{Connection, fdo, interface};
use zbus_polkit::policykit1::AuthorityProxy;

use crate::interface::Op;
use crate::service::Service;

const IDLE_EXIT: Duration = Duration::from_mins(5);

struct Daemon {
    service: Arc<Service>,
    /// Every part detection found at startup, which is the one time it looks.
    parts: Vec<Identity>,
    restore: Restore,
}

#[interface(name = "io.github.valeronm.Frameguin1")]
impl Daemon {
    /// The inventory: every device that is a part, whether or not it is
    /// also a control.
    fn get_devices(&self) -> Vec<Identity> {
        self.service.touch();
        self.parts.clone()
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
        self.service.touch();
        if self.restore.enabled() == enabled {
            return Ok(());
        }
        self.service.authorize(&header).await?;
        self.restore.set_enabled(enabled);
        // A device that cannot be read is in the journal and the switch is
        // on regardless: nothing here is the caller's to act on.
        if enabled {
            let _ = interface::each_restorable(server, Op::Remember).await;
        }
        Ok(())
    }

    async fn restore(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> fdo::Result<()> {
        self.service.touch();
        if !self.restore.enabled() {
            return Ok(());
        }
        self.service.authorize(&header).await?;
        Ok(interface::each_restorable(server, Op::Restore).await?)
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

fn main() -> zbus::Result<()> {
    let last_used = Arc::new(Mutex::new(Instant::now()));
    let clock = last_used.clone();
    let Detected {
        devices,
        parts,
        restore,
    } = device::detect();
    // What a bug report about a device that is there and not served has to
    // start from.
    for identity in &parts {
        eprintln!("detected {identity}");
    }
    let _conn = zbus::block_on(async move {
        let conn = Connection::system().await?;
        let authority = AuthorityProxy::new(&conn)
            .await
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        let service = Arc::new(Service::new(authority, last_used));
        let daemon = Daemon {
            service: service.clone(),
            parts,
            restore,
        };
        interface::serve_all(conn.object_server(), daemon, devices).await?;
        // Claim the name only once the objects are served, so an activating
        // client can't call into a not-yet-registered interface.
        conn.request_name(wire::BUS_NAME).await?;
        // The line a start that hung between detection and the bus lacks,
        // which is what tells it apart from one that hung in detection.
        eprintln!("serving {}", wire::BUS_NAME);
        Ok::<_, zbus::Error>(conn)
    })?;
    loop {
        std::thread::sleep(Duration::from_mins(1));
        if clock.lock().unwrap().elapsed() > IDLE_EXIT {
            return Ok(());
        }
    }
}
