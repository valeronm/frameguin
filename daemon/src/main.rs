//! System D-Bus daemon exposing privileged Framework laptop controls.
//!
//! Owns io.github.valeronm.Frameguin on the system bus and reaches the
//! machine through `frameguin_hardware`. Setters require the polkit
//! action io.github.valeronm.frameguin.manage. Exits after 5 idle
//! minutes; D-Bus activation restarts it on demand.

mod interface;
mod served;
mod service;

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use frameguin_hardware::device::battery::Battery;
use frameguin_hardware::device::mainboard::Mainboard;
use frameguin_hardware::device::memory::Module;
use frameguin_hardware::device::power_led::PowerLed;
use frameguin_hardware::device::touchpad::Touchpad;
use frameguin_hardware::device::touchscreen::Touchscreen;
use frameguin_hardware::ec::Ec;
use frameguin_hardware::part::{Identity, Part};
use frameguin_hardware::probe;
use frameguin_hardware::state::{StateFile, Store};
use frameguin_wire as wire;
use zbus::message::Header;
use zbus::{Connection, fdo, interface};
use zbus_polkit::policykit1::AuthorityProxy;

use crate::served::Served;
use crate::service::Service;

const IDLE_EXIT: Duration = Duration::from_mins(5);

struct Daemon {
    /// None on hardware with no Framework EC — see [`Ec::open`]. Shared
    /// with the devices the EC is a transport for.
    ec: Option<Arc<Ec>>,
    service: Arc<Service>,
    /// Probed once per daemon lifetime; the EC feature set can't change
    /// while running.
    capabilities: OnceLock<Vec<wire::Capability>>,
    /// Every part detection found at startup, which is the one time it looks.
    parts: Vec<Identity>,
}

fn ec_err(e: impl std::fmt::Debug) -> fdo::Error {
    fdo::Error::Failed(format!("EC error: {e:?}"))
}

fn internal_err(e: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::Failed(e.to_string())
}

impl Daemon {
    fn ec(&self) -> fdo::Result<&Ec> {
        self.ec
            .as_deref()
            // NotSupported (not Failed): lets a caller distinguish "wrong
            // hardware, permanently" from a transient EC error.
            .ok_or_else(|| fdo::Error::NotSupported("no Framework EC on this hardware".into()))
    }
}

#[interface(name = "io.github.valeronm.Frameguin1")]
impl Daemon {
    /// Which controls this board actually supports — see [`probe`] for the
    /// rule each answer has to meet.
    fn get_capabilities(&self) -> Vec<wire::Capability> {
        self.service.touch();
        self.capabilities
            .get_or_init(|| probe::capabilities(self.ec.as_deref()))
            .clone()
    }

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

    fn get_keyboard_backlight(&self) -> fdo::Result<u8> {
        self.service.touch();
        self.ec()?.keyboard_backlight().map_err(ec_err)
    }

    async fn set_keyboard_backlight(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.service.touch();
        if percent > 100 {
            return Err(fdo::Error::InvalidArgs("backlight must be 0-100".into()));
        }
        self.service.authorize(&header).await?;
        self.ec()?.set_keyboard_backlight(percent);
        Ok(())
    }
}

fn main() -> zbus::Result<()> {
    let last_used = Arc::new(Mutex::new(Instant::now()));
    let clock = last_used.clone();
    let store: Arc<dyn Store> = Arc::new(StateFile::load());
    // One walk of the HID bus for every device asked about: building an
    // `HidApi` enumerates the lot.
    let hid = hidapi::HidApi::new().ok();
    let touchpad = hid
        .as_ref()
        .and_then(|hid| Touchpad::detect(hid, store.clone()));
    let touchscreen = hid
        .as_ref()
        .and_then(|hid| Touchscreen::detect(hid, store.clone()));
    let ec = Ec::open().map(Arc::new);
    let power_led = ec
        .as_ref()
        .and_then(|ec| PowerLed::detect(ec, store.clone()));
    let battery = ec
        .as_ref()
        .and_then(|ec| Battery::detect(ec, store.clone()));
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
    let _conn = zbus::block_on(async move {
        let conn = Connection::system().await?;
        let authority = AuthorityProxy::new(&conn)
            .await
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        let service = Arc::new(Service::new(authority, last_used));
        let daemon = Daemon {
            ec,
            service: service.clone(),
            capabilities: OnceLock::new(),
            parts,
        };
        let server = conn.object_server();
        server.at(wire::OBJECT_PATH, daemon).await?;
        // A device not detected is not on the bus: the interfaces present at
        // the path are the inventory.
        if let Some(touchpad) = touchpad {
            server
                .at(wire::OBJECT_PATH, Served::new(touchpad, service.clone()))
                .await?;
        }
        if let Some(touchscreen) = touchscreen {
            server
                .at(wire::OBJECT_PATH, Served::new(touchscreen, service.clone()))
                .await?;
        }
        if let Some(power_led) = power_led {
            server
                .at(wire::OBJECT_PATH, Served::new(power_led, service.clone()))
                .await?;
        }
        if let Some(battery) = battery {
            server
                .at(wire::OBJECT_PATH, Served::new(battery, service))
                .await?;
        }
        // Claim the name only once the objects are served, so an activating
        // client can't call into a not-yet-registered interface.
        conn.request_name(wire::BUS_NAME).await?;
        Ok::<_, zbus::Error>(conn)
    })?;
    loop {
        std::thread::sleep(Duration::from_mins(1));
        if clock.lock().unwrap().elapsed() > IDLE_EXIT {
            return Ok(());
        }
    }
}
