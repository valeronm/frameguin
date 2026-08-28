//! System D-Bus daemon exposing privileged Framework laptop controls.
//!
//! Owns io.github.valeronm.Frameguin on the system bus and talks to the
//! embedded controller directly via `framework_lib`. Setters require the polkit
//! action io.github.valeronm.frameguin.manage. Exits after 5 idle
//! minutes; D-Bus activation restarts it on demand.

mod interface;
mod power_led;
mod served;
mod service;
mod state;
mod touchscreen;

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use frameguin_hardware::device::touchpad::Touchpad;
use frameguin_hardware::ec::Ec;
use frameguin_hardware::lifetime::{EcStamp, HostStamp};
use frameguin_hardware::probe;
use frameguin_hardware::state::{StateFile, Store};
use frameguin_wire::{self as wire, NO_CHARGE_CURRENT_LIMIT};
use zbus::message::Header;
use zbus::{Connection, fdo, interface};
use zbus_polkit::policykit1::AuthorityProxy;

use crate::power_led::PowerLedWrite;
use crate::served::Served;
use crate::service::Service;
use crate::state::{ChargeCurrentLimit, State};

const IDLE_EXIT: Duration = Duration::from_mins(5);

struct Daemon {
    /// None on hardware with no Framework EC — see [`Ec::open`].
    ec: Option<Ec>,
    service: Arc<Service>,
    store: Arc<dyn Store>,
    /// The one walk of the HID bus this run makes, kept for the probe: the
    /// devices detected at startup came off it, and the ones the probe still
    /// answers for are asked of the same enumeration.
    hid: Option<hidapi::HidApi>,
    /// Probed once per daemon lifetime; the EC feature set can't change
    /// while running.
    capabilities: OnceLock<Vec<wire::Capability>>,
    /// Mirrored rather than read back — see [`state`]. This one expires: the
    /// EC keeps the limit in RAM, which outlives host reboots but not an EC
    /// restart.
    charge_current_limit: Mutex<ChargeCurrentLimit>,
    /// When this daemon last darkened the power LED, and None when it has not.
    /// The kernel holds the LED state itself, so what is mirrored here is only
    /// the date of the write — see [`power_led`] for what that dating settles.
    power_led_off: Mutex<Option<EcStamp>>,
    /// When this daemon switched the touch panel off, and None while it is
    /// reporting. Read only on the panel route, the pad route carrying the
    /// level itself.
    touchscreen_off: Mutex<Option<HostStamp>>,
}

fn ec_err(e: impl std::fmt::Debug) -> fdo::Error {
    fdo::Error::Failed(format!("EC error: {e:?}"))
}

fn internal_err(e: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::Failed(e.to_string())
}

impl Daemon {
    fn save_state(&self) {
        // Bound before the write so both guards drop here rather than being
        // held across the file I/O.
        let state = State {
            charge_current_limit: *self.charge_current_limit.lock().unwrap(),
            power_led_off: *self.power_led_off.lock().unwrap(),
            touchscreen_off: self.touchscreen_off.lock().unwrap().clone(),
        };
        state::save(&*self.store, &state);
    }

    /// The mirrored charge current limit, or `NO_CHARGE_CURRENT_LIMIT` once
    /// the EC has restarted and dropped whatever was written.
    fn held_charge_current_limit(&self) -> fdo::Result<u32> {
        // Asked for before the mirror is read: a board with no EC has no
        // limit to report, and answering the sentinel would call that "no
        // limit set" rather than "no such control".
        let ec = self.ec()?;
        let limit = *self.charge_current_limit.lock().unwrap();
        // Nothing mirrored is already the answer, so don't spend an EC round
        // trip dating it.
        if limit.milliamps == NO_CHARGE_CURRENT_LIMIT {
            return Ok(NO_CHARGE_CURRENT_LIMIT);
        }
        Ok(if ec.same_boot_as(limit.stamp).map_err(ec_err)? {
            limit.milliamps
        } else {
            NO_CHARGE_CURRENT_LIMIT
        })
    }

    fn ec(&self) -> fdo::Result<&Ec> {
        self.ec
            .as_ref()
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
            .get_or_init(|| probe::capabilities(self.ec.as_ref(), self.hid.as_ref()))
            .clone()
    }

    fn get_charge_limit(&self) -> fdo::Result<u8> {
        self.service.touch();
        self.ec()?.charge_limit().map_err(ec_err)
    }

    /// Returns whether the EC was written, so a caller can tell a change from
    /// a request for the ceiling already in place and not announce the two
    /// the same way.
    async fn set_charge_limit(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<bool> {
        self.service.touch();
        if !(20..=100).contains(&percent) {
            return Err(fdo::Error::InvalidArgs(
                "charge limit must be 20-100".into(),
            ));
        }
        // Already there: nothing to write, and nothing worth an authorization
        // prompt either — same reason arguments are validated before asking.
        // Checked here rather than by the caller, so the answer comes from
        // the hardware and no client can act on a stale idea of it.
        if self.get_charge_limit()? == percent {
            return Ok(false);
        }
        self.service.authorize(&header).await?;
        self.ec()?.set_charge_limit(percent).map_err(ec_err)?;
        Ok(true)
    }

    /// How fast the battery may charge, in mA, or `NO_CHARGE_CURRENT_LIMIT`
    /// when nothing caps it. The EC cannot be asked what it holds, so this is
    /// what the daemon last wrote, and it reports no limit once the EC has
    /// restarted and dropped the value.
    fn get_charge_current_limit(&self) -> fdo::Result<u32> {
        self.service.touch();
        self.held_charge_current_limit()
    }

    /// Everything the EC's battery block says about the pack, for a reader
    /// looking at the pack rather than at the controls that shape it. One walk
    /// of the block, so the reading it carries cannot disagree with the rest
    /// of what it reports.
    ///
    /// The charge is the one value here that changes without anyone setting
    /// it, so a caller showing it has to re-read; it is also the only
    /// observable effect a charge current limit has, the limit itself being
    /// unreadable.
    fn get_battery_info(&self) -> fdo::Result<wire::BatteryInfo> {
        self.service.touch();
        // Spelled here rather than shared with the reading below, which fails
        // for a different reason and says so: a passthrough that stays silent
        // is not an absent pack, and can happen with one fitted.
        self.ec()?
            .battery_info()
            .ok_or_else(|| fdo::Error::Failed("no battery present".into()))
    }

    /// What the pack says about itself past the EC's summary: its temperature,
    /// its cell voltages, and any alarms it is raising. Separate from the
    /// report above because it reaches the pack over the EC's I2C passthrough
    /// rather than reading the EC's own block, so a board can answer one and
    /// not the other — and because a transfer per cell plus two, to a device
    /// the EC is also driving, is not something to spend on a caller that
    /// shows none of it.
    fn get_battery_condition(&self) -> fdo::Result<wire::BatteryCondition> {
        self.service.touch();
        self.ec()?
            .battery_condition()
            .ok_or_else(|| fdo::Error::Failed("the battery did not answer".into()))
    }

    /// Caps how fast the battery charges, in mA; `NO_CHARGE_CURRENT_LIMIT`
    /// lifts the cap. Zero is refused: the EC clamps its requested current
    /// against this value, so zero stops charging altogether rather than
    /// meaning "unrestricted", and nothing would report that back.
    /// Returns whether the EC was written, as `set_charge_limit` does.
    async fn set_charge_current_limit(
        &self,
        milliamps: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<bool> {
        self.service.touch();
        if milliamps == 0 {
            return Err(fdo::Error::InvalidArgs(format!(
                "0 stops charging; pass {NO_CHARGE_CURRENT_LIMIT} to remove the limit"
            )));
        }
        // Already there: nothing to write, and nothing worth an authorization
        // prompt either, as in `set_charge_limit` — except that the closest
        // thing to the truth here is the daemon's own mirror, the EC having
        // no readback to offer.
        if self.get_charge_current_limit()? == milliamps {
            return Ok(false);
        }
        self.service.authorize(&header).await?;
        let stamp = self
            .ec()?
            .set_charge_current_limit(milliamps)
            .map_err(ec_err)?;
        *self.charge_current_limit.lock().unwrap() = ChargeCurrentLimit { milliamps, stamp };
        self.save_state();
        Ok(true)
    }

    fn get_ec_version(&self) -> fdo::Result<String> {
        self.service.touch();
        self.ec()?.version().map_err(ec_err)
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

    /// Returns the brightness percentage and the preset it came from. That
    /// can be `Custom`, which the EC reports after any raw percentage write
    /// and which no setter accepts, or `Off`, which the EC cannot report at
    /// all — it is the host holding the LED, and the percentage alongside it
    /// is the one the EC will light it at when the host lets go.
    fn get_power_led_brightness(&self) -> fdo::Result<(u8, wire::PowerLedLevel)> {
        self.service.touch();
        let ec = self.ec()?;
        let (percent, level) = ec.power_led_level().map_err(ec_err)?;
        if self.power_led_off_node(ec).is_some() {
            return Ok((percent, wire::PowerLedLevel::Off));
        }
        Ok((percent, level))
    }

    async fn set_power_led_level(
        &self,
        level: wire::PowerLedLevel,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.service.touch();
        let write = PowerLedWrite::for_level(level)?;
        self.service.authorize(&header).await?;
        self.write_power_led(write).await
    }

    async fn set_power_led_brightness(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.service.touch();
        // The EC accepts 1-100; 0 is rejected (it will not let the host
        // extinguish the indicator) and 0xFF is the protocol's read sentinel.
        if !(1..=100).contains(&percent) {
            return Err(fdo::Error::InvalidArgs("brightness must be 1-100".into()));
        }
        let write = PowerLedWrite::Percentage(percent);
        self.service.authorize(&header).await?;
        self.write_power_led(write).await
    }

    /// Whether the touch panel is on, from whichever account this machine's
    /// route keeps — see [`touchscreen`] for why one of them reads the
    /// hardware and the other this daemon's own record.
    fn get_touchscreen_enabled(&self) -> fdo::Result<bool> {
        self.service.touch();
        Ok(touchscreen::route()?
            .reading()?
            .unwrap_or_else(|| self.touchscreen_off.lock().unwrap().is_none()))
    }

    /// Switches the touch panel on or off, by cutting the controller off or
    /// by telling it to stop reporting — whichever this machine's panel is
    /// reached by. Nothing re-applies it afterwards: the panel is put back on
    /// behind whoever asked for it off, by a resume or a lid opening on one
    /// route and by whatever the controller does not keep on the other, and a
    /// daemon that re-asserted a switch on those events would be enforcing a
    /// policy nobody asked it to hold.
    ///
    /// Returns nothing where the charge setters return whether they wrote:
    /// they report a skip so a caller can word its announcement differently,
    /// and this control makes no announcement — the switch's own position is
    /// the answer, and it is already where the caller asked.
    async fn set_touchscreen_enabled(
        &self,
        enabled: bool,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.service.touch();
        // Resolved before the prompt for the reason arguments are checked
        // before it: hardware with neither route can only end in an error,
        // and nobody should answer for a write that cannot happen.
        let route = touchscreen::route()?;
        // Already there: nothing to write, and nothing worth a prompt, as in
        // the charge setters. Which matters more here than there rather than
        // less: the panel comes back on behind whoever asked for it off, so a
        // client acting on what it last saw is the ordinary case rather than
        // the careless one. Only a reading can say so, so the route with none
        // never skips — see [`touchscreen::Route::reading`].
        if route.reading()? == Some(enabled) {
            return Ok(());
        }
        self.service.authorize(&header).await?;
        self.write_touchscreen(&route, enabled)
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
    let state = state::load(&*store);
    // One walk of the HID bus for every device asked about, here and in the
    // probe: building an `HidApi` enumerates the lot.
    let hid = hidapi::HidApi::new().ok();
    let touchpad = hid
        .as_ref()
        .and_then(|hid| Touchpad::detect(hid, store.clone()));
    let _conn = zbus::block_on(async move {
        let conn = Connection::system().await?;
        let authority = AuthorityProxy::new(&conn)
            .await
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        let service = Arc::new(Service::new(authority, last_used));
        let daemon = Daemon {
            ec: Ec::open(),
            service: service.clone(),
            store,
            hid,
            capabilities: OnceLock::new(),
            charge_current_limit: Mutex::new(state.charge_current_limit),
            power_led_off: Mutex::new(state.power_led_off),
            touchscreen_off: Mutex::new(state.touchscreen_off),
        };
        let server = conn.object_server();
        server.at(wire::OBJECT_PATH, daemon).await?;
        // A device not detected is not on the bus: the interfaces present at
        // the path are the inventory.
        if let Some(touchpad) = touchpad {
            server
                .at(wire::OBJECT_PATH, Served::new(touchpad, service))
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
