//! System D-Bus daemon exposing privileged Framework laptop controls.
//!
//! Owns io.github.valeronm.Frameguin on the system bus and talks to the
//! embedded controller directly via `framework_lib`. Setters require the polkit
//! action io.github.valeronm.frameguin.manage. Exits after 5 idle
//! minutes; D-Bus activation restarts it on demand.

mod board;
mod ec;
mod fp;
mod led;
mod probe;
mod state;
mod touchpad;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use frameguin_wire::{self as wire, HAPTIC_INTENSITY_LEVELS, NO_CHARGE_CURRENT_LIMIT};
use zbus::message::Header;
use zbus::{Connection, fdo, interface};
use zbus_polkit::policykit1::{AuthorityProxy, CheckAuthorizationFlags, Subject};

use crate::ec::{Ec, EcStamp};
use crate::fp::FpWrite;
use crate::state::{ChargeCurrentLimit, State};

const POLKIT_ACTION: &str = "io.github.valeronm.frameguin.manage";
const IDLE_EXIT: Duration = Duration::from_mins(5);

struct Daemon {
    /// None on hardware with no Framework EC — see [`Ec::open`].
    ec: Option<Ec>,
    authority: AuthorityProxy<'static>,
    last_used: Arc<Mutex<Instant>>,
    /// Probed once per daemon lifetime; the EC feature set can't change
    /// while running.
    capabilities: OnceLock<Vec<wire::Capability>>,
    /// Mirrored rather than read back — see [`state`].
    haptic_intensity: AtomicU8,
    click_force: AtomicU8,
    /// Mirrored like the touchpad's, but this one expires: the EC keeps the
    /// limit in RAM, which outlives host reboots but not an EC restart.
    charge_current_limit: Mutex<ChargeCurrentLimit>,
    /// When this daemon last darkened the fingerprint LED, and None when it
    /// has not. The kernel holds the LED state itself, so what is mirrored
    /// here is only the date of the write — see [`fp`] for what that dating
    /// settles.
    fp_off: Mutex<Option<EcStamp>>,
}

fn ec_err(e: impl std::fmt::Debug) -> fdo::Error {
    fdo::Error::Failed(format!("EC error: {e:?}"))
}

fn internal_err(e: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::Failed(e.to_string())
}

impl Daemon {
    fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    fn save_state(&self) {
        // Bound before the write so both guards drop here rather than being
        // held across the file I/O.
        let state = State {
            haptic_intensity: self.haptic_intensity.load(Ordering::Relaxed),
            click_force: self.click_force.load(Ordering::Relaxed),
            charge_current_limit: *self.charge_current_limit.lock().unwrap(),
            fp_off: *self.fp_off.lock().unwrap(),
        };
        state::save(&state);
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

    /// Call only once the arguments have been validated: this can raise a
    /// password prompt, and a caller that authorizes first makes the user
    /// answer one for a request that can only end in `InvalidArgs`.
    async fn authorize(&self, header: &Header<'_>) -> fdo::Result<()> {
        let subject = Subject::new_for_message_header(header).map_err(internal_err)?;
        let result = self
            .authority
            .check_authorization(
                &subject,
                POLKIT_ACTION,
                &HashMap::new(),
                CheckAuthorizationFlags::AllowUserInteraction.into(),
                "",
            )
            .await
            .map_err(internal_err)?;
        if result.is_authorized {
            Ok(())
        } else {
            Err(fdo::Error::AccessDenied("not authorized".into()))
        }
    }
}

#[interface(name = "io.github.valeronm.Frameguin1")]
impl Daemon {
    /// Which controls this board actually supports — see [`probe`] for the
    /// rule each answer has to meet.
    fn get_capabilities(&self) -> Vec<wire::Capability> {
        self.touch();
        self.capabilities
            .get_or_init(|| probe::capabilities(self.ec.as_ref()))
            .clone()
    }

    fn get_charge_limit(&self) -> fdo::Result<u8> {
        self.touch();
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
        self.touch();
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
        self.authorize(&header).await?;
        self.ec()?.set_charge_limit(percent).map_err(ec_err)?;
        Ok(true)
    }

    /// How fast the battery may charge, in mA, or `NO_CHARGE_CURRENT_LIMIT`
    /// when nothing caps it. The EC cannot be asked what it holds, so this is
    /// what the daemon last wrote, and it reports no limit once the EC has
    /// restarted and dropped the value.
    fn get_charge_current_limit(&self) -> fdo::Result<u32> {
        self.touch();
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
        self.touch();
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
        self.touch();
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
        self.touch();
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
        self.authorize(&header).await?;
        let stamp = self
            .ec()?
            .set_charge_current_limit(milliamps)
            .map_err(ec_err)?;
        *self.charge_current_limit.lock().unwrap() = ChargeCurrentLimit { milliamps, stamp };
        self.save_state();
        Ok(true)
    }

    fn get_ec_version(&self) -> fdo::Result<String> {
        self.touch();
        self.ec()?.version().map_err(ec_err)
    }

    /// The daemon's version and the path it was started from. The path is the
    /// diagnostic: two install trees can hold the same version, and which
    /// daemon runs is decided by the D-Bus activation file rather than by
    /// PATH. Answers without touching the EC, so it works on any hardware.
    fn get_build(&self) -> (String, String) {
        self.touch();
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
    fn get_fingerprint_brightness(&self) -> fdo::Result<(u8, wire::FpLevel)> {
        self.touch();
        let ec = self.ec()?;
        let (percent, level) = ec.fp_level().map_err(ec_err)?;
        if self.fp_off_led(ec).is_some() {
            return Ok((percent, wire::FpLevel::Off));
        }
        Ok((percent, level))
    }

    async fn set_fingerprint_level(
        &self,
        level: wire::FpLevel,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        let write = FpWrite::for_level(level)?;
        self.authorize(&header).await?;
        self.write_fingerprint(write).await
    }

    async fn set_fingerprint_brightness(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        // The EC accepts 1-100; 0 is rejected (the LED doubles as the power
        // indicator) and 0xFF is the protocol's read sentinel.
        if !(1..=100).contains(&percent) {
            return Err(fdo::Error::InvalidArgs("brightness must be 1-100".into()));
        }
        self.authorize(&header).await?;
        self.write_fingerprint(FpWrite::Percentage(percent)).await
    }

    fn get_haptic_intensity(&self) -> u8 {
        self.touch();
        self.haptic_intensity.load(Ordering::Relaxed)
    }

    async fn set_haptic_intensity(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        if !HAPTIC_INTENSITY_LEVELS.contains(&percent) {
            return Err(fdo::Error::InvalidArgs(format!(
                "intensity must be one of {HAPTIC_INTENSITY_LEVELS:?}"
            )));
        }
        self.authorize(&header).await?;
        touchpad::set_haptic_intensity(percent).map_err(internal_err)?;
        self.haptic_intensity.store(percent, Ordering::Relaxed);
        self.save_state();
        Ok(())
    }

    fn get_touchpad_click_force(&self) -> wire::ClickForce {
        self.touch();
        // A code no force maps to reads as the factory default, the same
        // answer this gives before anything has been written.
        touchpad::wire_click_force(self.click_force.load(Ordering::Relaxed))
            .unwrap_or(touchpad::DEFAULT_CLICK_FORCE)
    }

    async fn set_touchpad_click_force(
        &self,
        force: wire::ClickForce,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        let force = touchpad::click_force(force);
        self.authorize(&header).await?;
        touchpad::set_click_force(force).map_err(internal_err)?;
        self.click_force.store(force as u8, Ordering::Relaxed);
        self.save_state();
        Ok(())
    }

    fn get_keyboard_backlight(&self) -> fdo::Result<u8> {
        self.touch();
        self.ec()?.keyboard_backlight().map_err(ec_err)
    }

    async fn set_keyboard_backlight(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        if percent > 100 {
            return Err(fdo::Error::InvalidArgs("backlight must be 0-100".into()));
        }
        self.authorize(&header).await?;
        self.ec()?.set_keyboard_backlight(percent);
        Ok(())
    }
}

fn main() -> zbus::Result<()> {
    let last_used = Arc::new(Mutex::new(Instant::now()));
    let clock = last_used.clone();
    let state = state::load();
    let _conn = zbus::block_on(async move {
        let conn = Connection::system().await?;
        let authority = AuthorityProxy::new(&conn)
            .await
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        let daemon = Daemon {
            ec: Ec::open(),
            authority,
            last_used,
            capabilities: OnceLock::new(),
            haptic_intensity: AtomicU8::new(state.haptic_intensity),
            click_force: AtomicU8::new(state.click_force),
            charge_current_limit: Mutex::new(state.charge_current_limit),
            fp_off: Mutex::new(state.fp_off),
        };
        conn.object_server().at(wire::OBJECT_PATH, daemon).await?;
        // Claim the name only once the object is served, so an activating
        // client can't call into a not-yet-registered path.
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
