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

use frameguin_hardware::device::battery::Battery;
use frameguin_hardware::device::mainboard::Mainboard;
use frameguin_hardware::device::memory::Module;
use frameguin_hardware::device::power_led::PowerLed;
use frameguin_hardware::device::touchpad::Touchpad;
use frameguin_hardware::device::touchscreen::Touchscreen;
use frameguin_hardware::ec::Ec;
use frameguin_hardware::lifetime::{self, Holders};
use frameguin_hardware::mirror::Mirrors;
use frameguin_hardware::part::{Identity, Part};
use frameguin_hardware::state::{StateFile, Store};
use frameguin_wire as wire;
use zbus::{Connection, interface};
use zbus_polkit::policykit1::AuthorityProxy;

use crate::interface::Devices;
use crate::service::Service;

const IDLE_EXIT: Duration = Duration::from_mins(5);

struct Daemon {
    service: Arc<Service>,
    /// Every part detection found at startup, which is the one time it looks.
    parts: Vec<Identity>,
}

#[interface(name = "io.github.valeronm.Frameguin1")]
impl Daemon {
    /// The inventory: every device that is a part, whether or not it is
    /// also a control.
    fn get_devices(&self) -> Vec<Identity> {
        self.service.touch();
        self.parts.clone()
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
    let store: Arc<dyn Store> = Arc::new(StateFile::load());
    let ec = Ec::open().map(Arc::new);
    let holders = Holders::new(
        ec.as_ref().and_then(|ec| ec.boot().ok()),
        lifetime::host_boot(),
    );
    let mirrors = Mirrors::new(store, holders);
    // One walk of the HID bus for every device asked about: building an
    // `HidApi` enumerates the lot.
    let hid = hidapi::HidApi::new().ok();
    let touchpad = hid.as_ref().and_then(|hid| Touchpad::detect(hid, &mirrors));
    let touchscreen = hid
        .as_ref()
        .and_then(|hid| Touchscreen::detect(hid, &mirrors));
    let power_led = ec.as_ref().and_then(PowerLed::detect);
    let battery = ec.as_ref().and_then(|ec| Battery::detect(ec, &mirrors));
    let mainboard = Mainboard::detect(ec.as_deref());
    let memory = Module::detect();
    let parts: Vec<Identity> = [
        mainboard.as_ref().map(Part::identity),
        battery.as_ref().map(Part::identity),
        touchpad.as_ref().map(Part::identity),
        touchscreen.as_ref().map(Part::identity),
    ]
    .into_iter()
    .flatten()
    .chain(memory.iter().map(Part::identity))
    .cloned()
    .collect();
    // One journal line per part found, which is what a bug report about a
    // device that is there and not served has to start from.
    for identity in &parts {
        eprintln!("detected {identity}");
    }
    let devices = Devices {
        battery,
        touchpad,
        touchscreen,
        power_led,
    };
    let _conn = zbus::block_on(async move {
        let conn = Connection::system().await?;
        let authority = AuthorityProxy::new(&conn)
            .await
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        let service = Arc::new(Service::new(authority, last_used));
        let daemon = Daemon {
            service: service.clone(),
            parts,
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
