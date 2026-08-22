//! System D-Bus daemon exposing privileged Framework laptop controls.
//!
//! Owns io.github.valeronm.Frameguin on the system bus and talks to the
//! embedded controller directly via `framework_lib`. Setters require the polkit
//! action io.github.valeronm.frameguin.manage. Exits after 5 idle
//! minutes; D-Bus activation restarts it on demand.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use frameguin_wire::{self as wire, HAPTIC_INTENSITY_LEVELS, NO_CHARGE_CURRENT_LIMIT};
use framework_lib::chromium_ec::command::{EcCommands, EcRequestRaw};
use framework_lib::chromium_ec::commands::{
    EcRequestGetUptimeInfo, EcRequestPwmGetKeyboardBacklight, FpLedBrightnessLevel,
};
use framework_lib::chromium_ec::{CrosEc, EcResult};
use framework_lib::power;
use framework_lib::touchpad::{self, ClickForce};
use zbus::message::Header;
use zbus::{fdo, interface, Connection};
use zbus_polkit::policykit1::{AuthorityProxy, CheckAuthorizationFlags, Subject};

const POLKIT_ACTION: &str = "io.github.valeronm.frameguin.manage";
const IDLE_EXIT: Duration = Duration::from_mins(5);

struct Daemon {
    /// None on non-Framework hardware: `CrosEc::new()` panics outright when
    /// `framework_lib` finds no driver (empty driver list on e.g. aarch64
    /// without `/dev/cros_ec`), so it must not be constructed there.
    ec: Option<Mutex<CrosEc>>,
    authority: AuthorityProxy<'static>,
    last_used: Arc<Mutex<Instant>>,
    /// Probed once per daemon lifetime; the EC feature set can't change
    /// while running.
    capabilities: OnceLock<Vec<wire::Capability>>,
    /// Haptic touchpad controls are write-only (firmware ACKs `GET_FEATURE`
    /// but returns zeros — verified on hardware), and the touchpad persists
    /// them in its own flash across suspend and reboot. So the daemon
    /// mirrors every write to a state file and reloads it at startup —
    /// no re-apply needed, the hardware keeps itself.
    haptic_intensity: AtomicU8,
    click_force: AtomicU8,
    /// The EC will not report its charge current limit back — the command is
    /// write-only in every version (framework-system issue #180) — so the
    /// daemon mirrors what it wrote. Unlike the touchpad's, this mirror
    /// expires: the EC keeps the limit in RAM, which outlives host reboots
    /// but not an EC restart.
    charge_current_limit: Mutex<ChargeCurrentLimit>,
    /// When this daemon last darkened the fingerprint LED, and None when it
    /// has not. The kernel holds the LED state itself, so what is mirrored
    /// here is only the date of the write: an EC restart returns every LED to
    /// the EC's policy without the kernel noticing, and this is what tells
    /// that apart from a LED still dark because nothing has happened.
    fp_off: Mutex<Option<EcStamp>>,
    /// Read once and kept: reaching the design capacity walks the EC's whole
    /// memmap battery block, and a pack cannot change while the daemon runs.
    design_capacity: OnceLock<u32>,
}

const DEFAULT_HAPTIC_INTENSITY: u8 = 75;
const DEFAULT_CLICK_FORCE: u8 = ClickForce::Medium as u8;
const STATE_FILE: &str = "/var/lib/frameguin/state";

// Single source for the state file's keys: the loader matches on them and the
// writer spells them, so a rename can't quietly break the round trip.
const KEY_HAPTIC_INTENSITY: &str = "haptic_intensity";
const KEY_CLICK_FORCE: &str = "click_force";
const KEY_CURRENT_LIMIT: &str = "charge_current_limit";
const KEY_CURRENT_LIMIT_UPTIME: &str = "charge_current_limit_ec_uptime";
const KEY_CURRENT_LIMIT_WRITTEN_AT: &str = "charge_current_limit_written_at";
const KEY_FP_OFF_UPTIME: &str = "fp_off_ec_uptime";
const KEY_FP_OFF_WRITTEN_AT: &str = "fp_off_written_at";

/// A write dated against the EC's own life: seconds the EC had been running
/// when it happened, paired with the wall time of that same moment.
#[derive(Clone, Copy, Default)]
struct EcStamp {
    ec_uptime: u64,
    written_at: u64,
}

impl EcStamp {
    /// Taken against both clocks at once, which is what makes a later reading
    /// of the EC's comparable to the host's.
    fn now(ec: &CrosEc) -> EcResult<Self> {
        Ok(Self {
            ec_uptime: ec_uptime_secs(ec)?,
            written_at: unix_now(),
        })
    }

    fn still_current(self, ec: &CrosEc) -> EcResult<bool> {
        Ok(self.same_boot(ec_uptime_secs(ec)?, unix_now()))
    }

    /// Whether the EC has been running without interruption since. An EC that
    /// has been up for less time than the write implies has restarted, and a
    /// restart drops everything the EC was holding in RAM. The comparison
    /// carries slack because the EC keeps its own time — its firmware
    /// documents 1% or worse frequency error against the host clock.
    ///
    /// EC uptime is a 32-bit millisecond counter, so this reads as a restart
    /// once every 49 days of EC uptime; what was written then shows as gone
    /// until it is set again.
    fn same_boot(self, ec_uptime: u64, now: u64) -> bool {
        let expected = self.ec_uptime + now.saturating_sub(self.written_at);
        expected.saturating_sub(ec_uptime) <= (expected / 20).max(60)
    }
}

/// A charge current limit together with the stamp that dates it.
#[derive(Clone, Copy)]
struct ChargeCurrentLimit {
    milliamps: u32,
    stamp: EcStamp,
}

struct State {
    haptic_intensity: u8,
    click_force: u8,
    charge_current_limit: ChargeCurrentLimit,
    fp_off: Option<EcStamp>,
}

/// Loads the mirrored control state, falling back to the factory defaults.
/// A missing file on a machine whose touchpad was already changed by other
/// means will misreport until the first write — unavoidable, since the
/// hardware can't be read.
fn load_state() -> State {
    let mut state = State {
        haptic_intensity: DEFAULT_HAPTIC_INTENSITY,
        click_force: DEFAULT_CLICK_FORCE,
        charge_current_limit: ChargeCurrentLimit {
            milliamps: NO_CHARGE_CURRENT_LIMIT,
            stamp: EcStamp::default(),
        },
        fp_off: None,
    };
    if let Ok(content) = std::fs::read_to_string(STATE_FILE) {
        for line in content.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key {
                KEY_HAPTIC_INTENSITY => {
                    if let Ok(v) = value.parse()
                        && HAPTIC_INTENSITY_LEVELS.contains(&v)
                    {
                        state.haptic_intensity = v;
                    }
                }
                KEY_CLICK_FORCE => {
                    if let Ok(v) = value.parse()
                        && wire_click_force(v).is_some()
                    {
                        state.click_force = v;
                    }
                }
                KEY_CURRENT_LIMIT => {
                    // A zero here would mirror a limit the setter refuses to
                    // write, so read it as the absence of one.
                    if let Ok(v) = value.parse()
                        && v != 0
                    {
                        state.charge_current_limit.milliamps = v;
                    }
                }
                KEY_CURRENT_LIMIT_UPTIME => {
                    state.charge_current_limit.stamp.ec_uptime = value.parse().unwrap_or(0);
                }
                KEY_CURRENT_LIMIT_WRITTEN_AT => {
                    state.charge_current_limit.stamp.written_at = value.parse().unwrap_or(0);
                }
                KEY_FP_OFF_UPTIME => {
                    state.fp_off.get_or_insert_default().ec_uptime = value.parse().unwrap_or(0);
                }
                KEY_FP_OFF_WRITTEN_AT => {
                    state.fp_off.get_or_insert_default().written_at = value.parse().unwrap_or(0);
                }
                _ => {}
            }
        }
    }
    state
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Seconds since the EC last booted.
fn ec_uptime_secs(ec: &CrosEc) -> EcResult<u64> {
    let uptime_ms = EcRequestGetUptimeInfo {}.send_command(ec)?.time_since_ec_boot;
    Ok(u64::from(uptime_ms) / 1000)
}

fn ec_click_force(force: wire::ClickForce) -> ClickForce {
    match force {
        wire::ClickForce::Low => ClickForce::Low,
        wire::ClickForce::Medium => ClickForce::Medium,
        wire::ClickForce::High => ClickForce::High,
    }
}

/// The EC code the state file carries, back to the wire's name; None for a
/// code no force maps to.
fn wire_click_force(code: u8) -> Option<wire::ClickForce> {
    wire::ClickForce::ALL
        .into_iter()
        .find(|force| ec_click_force(*force) as u8 == code)
}

/// Known haptic touchpad models (`PixArt` PIDs). A curated device list, per
/// the probe rule: the haptic setters have no side-effect-free probe
/// (they're write-only, and every PTP touchpad accepts the open — only
/// haptic ones act on the reports). Keying on the touchpad's own HID
/// identity rather than the board name means a haptic pad retrofitted into
/// an older laptop is recognized. Extend when Framework ships new haptic
/// pads.
const HAPTIC_TOUCHPAD_PIDS: [u16; 1] = [0x1343];

/// The EC's own LED policy, under the name the kernel's LED class gives it.
/// Handing the LED back is done by activating this trigger: the activate
/// handler is what sends the EC its auto flag.
const LED_AUTO_TRIGGER: &str = "chromeos-auto";

/// No policy at all — the LED left to whatever brightness was last written.
const LED_NO_TRIGGER: &str = "none";

/// A `trigger` file's listing: every trigger the kernel offers, each paired
/// with whether it is the one in effect — which the file marks, and marks
/// only, by bracketing it. One decoding of that convention, so the two
/// questions asked of the file cannot come to disagree about it.
fn triggers(listed: &str) -> impl Iterator<Item = (&str, bool)> {
    listed.split_whitespace().map(|token| {
        token
            .strip_prefix('[')
            .and_then(|name| name.strip_suffix(']'))
            .map_or((token, false), |name| (name, true))
    })
}

fn active_in(listed: &str) -> Option<&str> {
    triggers(listed).find_map(|(name, active)| active.then_some(name))
}

/// The kernel's node for the EC's power LED, which is the LED the fingerprint
/// commands dim, and only when it is one this daemon can both darken and hand
/// back. Its name carries the LED's colour, and which colours a power LED has
/// is a board's business, so find it by the function it ends with rather than
/// by one board's spelling of it. A node offering no auto trigger is not a
/// control: it could be darkened and never released.
fn controllable_power_led() -> Option<PathBuf> {
    let dir = std::fs::read_dir("/sys/class/leds").ok()?.find_map(|entry| {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name = name.to_str()?;
        (name.starts_with("chromeos:") && name.ends_with(":power")).then(|| entry.path())
    })?;
    let listed = std::fs::read_to_string(dir.join("trigger")).ok()?;
    (dir.join("brightness").exists()
        && triggers(&listed).any(|(name, _)| name == LED_AUTO_TRIGGER))
    .then_some(dir)
}

/// Whether the kernel is holding the LED dark, in the exact arrangement
/// [`darken_led`] leaves — a LED parked on some third trigger is somebody
/// else's and not ours to read as off.
///
/// This is the kernel's record of what it last commanded rather than a
/// reading: the driver implements no `brightness_get`, and the EC's LED
/// command answers only with which colours exist. So a write that goes
/// straight to the EC (`ectool led`) passes unseen, while a host reboot
/// re-probes the driver and re-attaches the trigger, which reads as on.
fn led_dark_in_kernel(dir: &Path) -> bool {
    let listed = std::fs::read_to_string(dir.join("trigger")).unwrap_or_default();
    active_in(&listed) == Some(LED_NO_TRIGGER)
        && std::fs::read_to_string(dir.join("brightness")).is_ok_and(|value| value.trim() == "0")
}

/// Takes the LED off the EC's policy and darkens it.
///
/// Through the kernel rather than by sending `EC_CMD_LED_CONTROL` to the EC
/// directly, which would work and which the daemon is otherwise equipped to
/// do: the EC keeps no readable record of who owns the LED, so the driver's
/// is the only one there is, and a command issued behind its back would leave
/// it describing a policy the EC had already stopped following. Detaching the
/// trigger before the brightness write is that same argument a level down —
/// the trigger has no deactivate handler and never re-asserts, so a write
/// underneath one leaves the file naming a policy no longer in force.
fn darken_led(dir: &Path) -> std::io::Result<()> {
    std::fs::write(dir.join("trigger"), LED_NO_TRIGGER)?;
    std::fs::write(dir.join("brightness"), "0")
}

/// Gives the LED back to the EC. The brightness goes first and only has to be
/// nonzero: the EC reads it as on-or-off and lights the colour at the level's
/// own duty, so this restores no value — it stops the kernel's record saying
/// dark once nothing is holding the LED dark. Writing it after the trigger
/// instead would be a host command against a LED the EC had just taken back,
/// undoing the handover.
fn release_led(dir: &Path) -> std::io::Result<()> {
    std::fs::write(dir.join("brightness"), "1")?;
    std::fs::write(dir.join("trigger"), LED_AUTO_TRIGGER)
}

/// A settled fingerprint write, resolved from its arguments before anyone is
/// asked to authorize one. `Dark` carries the LED's node because finding it is
/// half of deciding the write is possible at all.
enum FpWrite {
    Level(FpLedBrightnessLevel),
    Percentage(u8),
    Dark(PathBuf),
}

/// None for the levels the EC has no setting for: `Custom`, which it only
/// ever reports, and `Off`, which is not the EC's to give.
fn ec_fp_level(level: wire::FpLevel) -> Option<FpLedBrightnessLevel> {
    Some(match level {
        wire::FpLevel::High => FpLedBrightnessLevel::High,
        wire::FpLevel::Medium => FpLedBrightnessLevel::Medium,
        wire::FpLevel::Low => FpLedBrightnessLevel::Low,
        wire::FpLevel::UltraLow => FpLedBrightnessLevel::UltraLow,
        wire::FpLevel::Auto => FpLedBrightnessLevel::Auto,
        wire::FpLevel::Custom | wire::FpLevel::Off => return None,
    })
}

/// A level the EC does not name is custom: that is what it reports after a
/// raw percentage write.
fn wire_fp_level(level: Option<&FpLedBrightnessLevel>) -> wire::FpLevel {
    match level {
        Some(FpLedBrightnessLevel::High) => wire::FpLevel::High,
        Some(FpLedBrightnessLevel::Medium) => wire::FpLevel::Medium,
        Some(FpLedBrightnessLevel::Low) => wire::FpLevel::Low,
        Some(FpLedBrightnessLevel::UltraLow) => wire::FpLevel::UltraLow,
        Some(FpLedBrightnessLevel::Auto) => wire::FpLevel::Auto,
        Some(FpLedBrightnessLevel::Custom) | None => wire::FpLevel::Custom,
    }
}

/// Without `/dev/cros_ec`, `framework_lib` falls back to raw port I/O; on a
/// non-Framework EC every command spin-waits to a timeout, stalling the
/// first `GetCapabilities` for tens of seconds. Don't touch the EC unless the
/// firmware says this is Framework hardware.
fn is_framework_hardware() -> bool {
    std::fs::read_to_string("/sys/class/dmi/id/sys_vendor")
        .is_ok_and(|vendor| vendor.trim() == "Framework")
}

fn haptic_touchpad_present() -> bool {
    hidapi::HidApi::new()
        .is_ok_and(|api| {
            api.device_list().any(|dev| {
                dev.vendor_id() == touchpad::PIX_VID
                    && HAPTIC_TOUCHPAD_PIDS.contains(&dev.product_id())
            })
        })
}

/// `framework_lib`'s `get_keyboard_backlight()` reads via `PWM_GET_DUTY`, and the
/// percent survives two floor divisions (percent→duty in the EC, then
/// duty→percent in the lib), coming back one low for most values — 5% reads
/// as 4%. This EC command returns the exact stored percent instead.
fn kbd_backlight_percent(ec: &CrosEc) -> EcResult<u8> {
    Ok(EcRequestPwmGetKeyboardBacklight {}.send_command(ec)?.percent)
}

/// The EC's battery block in the wire's terms, and None when no pack
/// answers. Direction and charger presence share one flag byte, so both come
/// from the same read; the rate is unsigned whichever way charge is moving.
fn battery_state(ec: &CrosEc) -> Option<wire::BatteryState> {
    let info = power::power_info(ec)?;
    let battery = info.battery?;
    Some(wire::BatteryState {
        // Against the last full charge, which is the EC's own denominator;
        // a pack reporting more than full is clamped rather than shown.
        percent: u8::try_from(battery.charge_percentage.min(100)).unwrap_or(100),
        flow: charge_flow(battery.charging, info.ac_present, battery.present_rate),
        milliamps: battery.present_rate,
    })
}

/// What the pack is doing, from the EC's charging flag, its charger flag and
/// the rate.
///
/// The discharging flag is deliberately not a parameter: it means "not being
/// charged" rather than "supplying the machine", and a full pack on a
/// connected charger sets it — a smart battery reporting zero charge
/// current. The rate is what separates a pack at rest from one running the
/// machine, and it reads a clean 0 at rest; a charger attached does not by
/// itself mean nothing is draining, since too weak a one leaves the pack
/// covering the difference.
fn charge_flow(charging: bool, ac_present: bool, milliamps: u32) -> wire::ChargeFlow {
    if charging {
        wire::ChargeFlow::Charging
    } else if ac_present && milliamps == 0 {
        wire::ChargeFlow::Idle
    } else {
        wire::ChargeFlow::Discharging
    }
}

/// What both battery readings answer with when no pack does. One spelling,
/// because a client that matches on the text sees it from either.
fn no_battery() -> fdo::Error {
    fdo::Error::Failed("no battery present".into())
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

    // The directory is provisioned by StateDirectory= in the systemd unit.
    fn save_state(&self) {
        let limit = *self.charge_current_limit.lock().unwrap();
        // Absent rather than zeroed while the LED is lit: the stamp only ever
        // withdraws a claim, and a zeroed one would withdraw every time.
        let fp_off = match *self.fp_off.lock().unwrap() {
            Some(stamp) => format!(
                "{KEY_FP_OFF_UPTIME}={}\n{KEY_FP_OFF_WRITTEN_AT}={}\n",
                stamp.ec_uptime, stamp.written_at
            ),
            None => String::new(),
        };
        let content = format!(
            "{KEY_HAPTIC_INTENSITY}={}\n{KEY_CLICK_FORCE}={}\n{KEY_CURRENT_LIMIT}={}\n\
             {KEY_CURRENT_LIMIT_UPTIME}={}\n{KEY_CURRENT_LIMIT_WRITTEN_AT}={}\n{fp_off}",
            self.haptic_intensity.load(Ordering::Relaxed),
            self.click_force.load(Ordering::Relaxed),
            limit.milliamps,
            limit.stamp.ec_uptime,
            limit.stamp.written_at,
        );
        if let Err(e) = std::fs::write(STATE_FILE, content) {
            eprintln!("failed to persist state: {e}");
        }
    }

    /// The battery's design capacity in mAh, `None` when no pack answers.
    /// Only successful reads are cached, so a battery that is momentarily
    /// unreadable isn't remembered as absent for the daemon's lifetime.
    fn read_design_capacity(&self, ec: &CrosEc) -> Option<u32> {
        if let Some(capacity) = self.design_capacity.get() {
            return Some(*capacity);
        }
        let capacity = power::power_info(ec)?.battery?.design_capacity;
        let _ = self.design_capacity.set(capacity);
        Some(capacity)
    }

    /// The mirrored charge current limit, or `NO_CHARGE_CURRENT_LIMIT` once
    /// the EC has restarted and dropped whatever was written.
    fn held_charge_current_limit(&self, ec: &CrosEc) -> EcResult<u32> {
        let limit = *self.charge_current_limit.lock().unwrap();
        // Nothing mirrored is already the answer, so don't spend an EC round
        // trip dating it.
        if limit.milliamps == NO_CHARGE_CURRENT_LIMIT {
            return Ok(NO_CHARGE_CURRENT_LIMIT);
        }
        Ok(if limit.stamp.still_current(ec)? {
            limit.milliamps
        } else {
            NO_CHARGE_CURRENT_LIMIT
        })
    }

    /// The LED's node when the fingerprint LED is off — the kernel holding it
    /// dark, on an EC that has not restarted since it was darkened — and None
    /// whenever it is lit. Answering with the node rather than a bool is what
    /// lets the caller that acts on it skip looking the LED up again.
    fn fp_off_led(&self, ec: &CrosEc) -> Option<PathBuf> {
        let dir = controllable_power_led()?;
        if !led_dark_in_kernel(&dir) {
            return None;
        }
        // The stamp can only ever withdraw the kernel's account, never supply
        // one: a LED this daemon did not darken has no stamp to date, and the
        // kernel's record is then the only account of it there is.
        match *self.fp_off.lock().unwrap() {
            Some(stamp) => stamp.still_current(ec).unwrap_or(false).then_some(dir),
            None => Some(dir),
        }
    }

    /// The one path to the fingerprint LED. An EC-driven write has to have the
    /// LED back before it goes out, or it lands on one the host is holding and
    /// the EC no longer lights — so that release belongs here, in the write
    /// itself, rather than in each caller's memory of it.
    fn write_fingerprint(&self, write: FpWrite) -> fdo::Result<()> {
        let ec = self.ec_guard()?;
        match write {
            FpWrite::Dark(dir) => self.darken_fp_led(&dir, &ec),
            FpWrite::Level(level) => {
                self.release_fp_led(&ec);
                ec.set_fp_led_level(level).map_err(ec_err)
            }
            FpWrite::Percentage(percent) => {
                self.release_fp_led(&ec);
                ec.set_fp_led_percentage(percent).map_err(ec_err)
            }
        }
    }

    fn darken_fp_led(&self, dir: &Path, ec: &CrosEc) -> fdo::Result<()> {
        // Dated before the write rather than after it, so a restart between
        // the two is read as having dropped it.
        let stamp = EcStamp::now(ec).map_err(ec_err)?;
        darken_led(dir).map_err(internal_err)?;
        *self.fp_off.lock().unwrap() = Some(stamp);
        self.save_state();
        Ok(())
    }

    /// Returns the LED to the EC if this daemon is holding it dark, so that a
    /// write of a level or a percentage is visible rather than swallowed by
    /// an LED the EC no longer drives. Only what the daemon itself arranged
    /// is undone.
    fn release_fp_led(&self, ec: &CrosEc) {
        let Some(dir) = self.fp_off_led(ec) else {
            return;
        };
        let _ = release_led(&dir);
        *self.fp_off.lock().unwrap() = None;
        self.save_state();
    }

    fn ec_guard(&self) -> fdo::Result<std::sync::MutexGuard<'_, CrosEc>> {
        self.ec
            .as_ref()
            .map(|ec| ec.lock().unwrap())
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
    /// Which controls this board actually supports. One capability per
    /// exposed operation, and each probe must be a side-effect-free exercise
    /// of the same code path the operation uses — never a related-but-easier
    /// check (a board can support reading a subsystem's version while its
    /// enable command works only on other hardware). Where no harmless
    /// same-path probe exists, hardcode the support condition here instead of
    /// probing something adjacent. The get-side probes below stand in for
    /// their setters only because those EC command pairs ship together in
    /// every firmware.
    fn get_capabilities(&self) -> Vec<wire::Capability> {
        self.touch();
        let caps = self.capabilities.get_or_init(|| {
            let mut caps = Vec::new();
            if let Some(ec) = &self.ec {
                let ec = ec.lock().unwrap();
                // The getter's own read, run for its answer rather than for
                // a version or a neighbouring command: a pack that reports
                // nothing here is exactly the one whose state cannot be
                // shown.
                if battery_state(&ec).is_some() {
                    caps.push(wire::Capability::BatteryState);
                }
                if ec.get_charge_limit().is_ok() {
                    caps.push(wire::Capability::ChargeLimit);
                }
                // No same-path probe exists: the charge current limit is
                // write-only, with no readback in any command version
                // (framework-system issue #180). GET_CMD_VERSIONS is the
                // closest harmless stand-in — it is side-effect-free and asks
                // about the very command the setter sends. The battery read
                // joins it because a limit is only ever expressed as a share
                // of what the pack asks for: without its capacity the control
                // has no rate to offer, so claiming it would offer a dead one.
                if ec
                    .cmd_version_supported(EcCommands::ChargeCurrentLimit as u32, 0)
                    .unwrap_or(false)
                    && self.read_design_capacity(&ec).is_some()
                {
                    caps.push(wire::Capability::ChargeCurrentLimit);
                }
                if kbd_backlight_percent(&ec).is_ok() {
                    caps.push(wire::Capability::KeyboardBacklight);
                }
                if ec.get_fp_led_level().is_ok() {
                    caps.push(wire::Capability::FpBrightness);
                    // Older EC firmware implements only command v0 of
                    // FpLedLevelControl: presets high/medium/low. V1 added
                    // the raw-percentage write, and the same firmware
                    // generation added the ultra-low and auto levels
                    // (framework-system issue #211) — so V1 support gates
                    // all of them. GET_CMD_VERSIONS is side-effect-free and
                    // asks about the exact command the setters use.
                    if ec
                        .cmd_version_supported(EcCommands::FpLedLevelControl as u32, 1)
                        .unwrap_or(false)
                    {
                        caps.push(wire::Capability::FpBrightnessCustom);
                    }
                    // Nested under the EC's own fingerprint control, which
                    // this one needs even though it never commands the LED
                    // through it: the setter dates its write against the EC,
                    // and off is offered as one level among that control's
                    // rest rather than as a control of its own. The probe is
                    // the same lookup the setter makes, so the two cannot
                    // come to different answers about what is reachable.
                    if controllable_power_led().is_some() {
                        caps.push(wire::Capability::FpOff);
                    }
                }
            }
            // One name for both haptic controls: they share the identical
            // support condition (same device, same firmware feature set).
            if haptic_touchpad_present() {
                caps.push(wire::Capability::HapticTouchpad);
            }
            caps
        });
        caps.clone()
    }

    fn get_charge_limit(&self) -> fdo::Result<u8> {
        self.touch();
        let (_min, max) = self.ec_guard()?.get_charge_limit().map_err(ec_err)?;
        Ok(max)
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
            return Err(fdo::Error::InvalidArgs("charge limit must be 20-100".into()));
        }
        // Already there: nothing to write, and nothing worth an authorization
        // prompt either — same reason arguments are validated before asking.
        // Checked here rather than by the caller, so the answer comes from
        // the hardware and no client can act on a stale idea of it.
        if self.get_charge_limit()? == percent {
            return Ok(false);
        }
        self.authorize(&header).await?;
        self.ec_guard()?
            .set_charge_limit(0, percent)
            .map_err(ec_err)?;
        Ok(true)
    }

    /// How fast the battery may charge, in mA, or `NO_CHARGE_CURRENT_LIMIT`
    /// when nothing caps it. The EC cannot be asked what it holds, so this is
    /// what the daemon last wrote, and it reports no limit once the EC has
    /// restarted and dropped the value.
    fn get_charge_current_limit(&self) -> fdo::Result<u32> {
        self.touch();
        let ec = self.ec_guard()?;
        self.held_charge_current_limit(&ec).map_err(ec_err)
    }

    /// The battery's design capacity in mAh, which is numerically also the
    /// current that charges it at 1C — what turns a charge speed expressed as
    /// a fraction of full rate into the milliamps the EC wants.
    fn get_battery_design_capacity(&self) -> fdo::Result<u32> {
        self.touch();
        let ec = self.ec_guard()?;
        self.read_design_capacity(&ec)
            .ok_or_else(no_battery)
    }

    /// The pack's charge, which way it is moving and how fast. The only
    /// value here that changes without anyone setting it, so a caller
    /// showing it has to re-read; every other getter answers with what was
    /// last written or configured.
    ///
    /// It also carries the only observable effect a charge current limit
    /// has, the limit itself being unreadable.
    fn get_battery_state(&self) -> fdo::Result<wire::BatteryState> {
        self.touch();
        battery_state(&*self.ec_guard()?)
            .ok_or_else(no_battery)
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
        let stamp = {
            let ec = self.ec_guard()?;
            // Always the unconditional form. The command's state-of-charge
            // variant latches inside the EC: once applied it is never
            // re-evaluated, so a later threshold cannot lift it
            // (framework-system issue #342).
            ec.set_charge_current_limit(milliamps, None)
                .map_err(ec_err)?;
            EcStamp::now(&ec).map_err(ec_err)?
        };
        *self.charge_current_limit.lock().unwrap() = ChargeCurrentLimit { milliamps, stamp };
        self.save_state();
        Ok(true)
    }

    fn get_ec_version(&self) -> fdo::Result<String> {
        self.touch();
        self.ec_guard()?.version_info().map_err(ec_err)
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
        let ec = self.ec_guard()?;
        let (percent, level) = ec.get_fp_led_level().map_err(ec_err)?;
        if self.fp_off_led(&ec).is_some() {
            return Ok((percent, wire::FpLevel::Off));
        }
        Ok((percent, wire_fp_level(level.as_ref())))
    }

    async fn set_fingerprint_level(
        &self,
        level: wire::FpLevel,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        let write = if level == wire::FpLevel::Off {
            FpWrite::Dark(controllable_power_led().ok_or_else(|| {
                fdo::Error::NotSupported("no kernel LED node for the power LED".into())
            })?)
        } else {
            // Off is answered above, so the level left without an EC setting
            // is the one the EC only ever reports.
            FpWrite::Level(ec_fp_level(level).ok_or_else(|| {
                fdo::Error::InvalidArgs(
                    "custom is what the EC reports after a percentage write, not a level to set"
                        .into(),
                )
            })?)
        };
        self.authorize(&header).await?;
        self.write_fingerprint(write)
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
        self.write_fingerprint(FpWrite::Percentage(percent))
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
        wire_click_force(self.click_force.load(Ordering::Relaxed))
            .unwrap_or(wire::ClickForce::Medium)
    }

    async fn set_touchpad_click_force(
        &self,
        force: wire::ClickForce,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        let force = ec_click_force(force);
        self.authorize(&header).await?;
        touchpad::set_click_force(force).map_err(internal_err)?;
        self.click_force.store(force as u8, Ordering::Relaxed);
        self.save_state();
        Ok(())
    }

    fn get_keyboard_backlight(&self) -> fdo::Result<u8> {
        self.touch();
        kbd_backlight_percent(&*self.ec_guard()?).map_err(ec_err)
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
        self.ec_guard()?.set_keyboard_backlight(percent);
        Ok(())
    }
}

fn main() -> zbus::Result<()> {
    let last_used = Arc::new(Mutex::new(Instant::now()));
    let clock = last_used.clone();
    let state = load_state();
    let _conn = zbus::block_on(async move {
        let conn = Connection::system().await?;
        let authority = AuthorityProxy::new(&conn)
            .await
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        let daemon = Daemon {
            ec: is_framework_hardware().then(|| Mutex::new(CrosEc::new())),
            authority,
            last_used,
            capabilities: OnceLock::new(),
            haptic_intensity: AtomicU8::new(state.haptic_intensity),
            click_force: AtomicU8::new(state.click_force),
            charge_current_limit: Mutex::new(state.charge_current_limit),
            fp_off: Mutex::new(state.fp_off),
            design_capacity: OnceLock::new(),
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

#[cfg(test)]
mod tests {
    use super::{
        active_in, charge_flow, triggers, wire, EcStamp, HAPTIC_INTENSITY_LEVELS,
        LED_AUTO_TRIGGER,
    };

    /// The state a full laptop sits in all day, and the one the EC's own
    /// flags describe as discharging. Reading that flag put "Discharging" on
    /// a machine that was plugged in and full.
    #[test]
    fn a_full_pack_on_its_charger_is_not_discharging() {
        assert_eq!(charge_flow(false, true, 0), wire::ChargeFlow::Idle);
    }

    /// A charger too weak for the load leaves the pack covering the
    /// difference, which the rate is the only witness to.
    #[test]
    fn a_pack_draining_under_a_weak_charger_is_discharging() {
        assert_eq!(charge_flow(false, true, 900), wire::ChargeFlow::Discharging);
    }

    #[test]
    fn nothing_attached_leaves_the_pack_running_the_machine() {
        assert_eq!(charge_flow(false, false, 1400), wire::ChargeFlow::Discharging);
        // Between two readings a pack can report no rate at all; with no
        // charger it is still the only thing powering the machine.
        assert_eq!(charge_flow(false, false, 0), wire::ChargeFlow::Discharging);
    }

    #[test]
    fn charge_arriving_outranks_the_rest() {
        assert_eq!(charge_flow(true, true, 2320), wire::ChargeFlow::Charging);
    }

    /// A `trigger` file as the kernel writes it, shortened. Which trigger is
    /// in effect is carried by brackets and nothing else, so the parsing is
    /// all that stands between a LED handed back to the EC and one only
    /// believed to be.
    const LISTED: &str = "none default rfkill-any panic chromeos-auto phy0rx";

    #[test]
    fn the_active_trigger_is_the_bracketed_one() {
        assert_eq!(
            active_in(&LISTED.replace("chromeos-auto", "[chromeos-auto]")),
            Some(LED_AUTO_TRIGGER)
        );
        assert_eq!(
            active_in(&LISTED.replace("none", "[none]")),
            Some(super::LED_NO_TRIGGER)
        );
    }

    /// Nothing bracketed means the kernel named no trigger, which is not the
    /// same as it naming the one called "none".
    #[test]
    fn a_listing_marking_nothing_has_no_active_trigger() {
        assert_eq!(active_in(LISTED), None);
    }

    #[test]
    fn a_trigger_is_offered_whether_or_not_it_is_the_active_one() {
        let active = LISTED.replace("chromeos-auto", "[chromeos-auto]");
        assert!(triggers(&active).any(|(name, _)| name == LED_AUTO_TRIGGER));
        assert!(triggers(LISTED).any(|(name, _)| name == LED_AUTO_TRIGGER));
    }

    /// The app offers these steps but cannot link `framework_lib` to learn
    /// them, so `wire` carries the list and this is what keeps the copy
    /// honest. A firmware generation that changes the steps should fail here
    /// rather than in a combo that silently offers the wrong ones.
    #[test]
    fn the_wire_haptic_steps_are_the_ones_the_touchpad_implements() {
        assert_eq!(
            HAPTIC_INTENSITY_LEVELS,
            framework_lib::touchpad::HAPTIC_INTENSITY_LEVELS
        );
    }

    fn taken(ec_uptime: u64, written_at: u64) -> EcStamp {
        EcStamp {
            ec_uptime,
            written_at,
        }
    }

    #[test]
    fn a_write_moments_ago_is_still_the_same_boot() {
        let stamp = taken(500_000, 1_000_000);
        assert!(stamp.same_boot(500_002, 1_000_002));
    }

    #[test]
    fn an_ec_that_has_run_the_elapsed_time_is_still_the_same_boot() {
        // A day passes with the EC up throughout.
        let stamp = taken(500_000, 1_000_000);
        assert!(stamp.same_boot(586_400, 1_086_400));
    }

    #[test]
    fn an_ec_that_restarted_is_a_different_boot() {
        // An hour of wall clock, but the EC reports a minute of uptime.
        let stamp = taken(500_000, 1_000_000);
        assert!(!stamp.same_boot(60, 1_003_600));
    }

    /// The EC's own clock is documented as 1% or worse against the host's, so
    /// a tolerance that didn't scale would call a long-standing write expired.
    #[test]
    fn clock_drift_over_a_long_uptime_is_not_a_restart() {
        let stamp = taken(0, 1_000_000);
        // Ten days later the EC is 1% short of the elapsed wall time.
        let elapsed = 10 * 86_400;
        assert!(stamp.same_boot(elapsed - elapsed / 100, 1_000_000 + elapsed));
    }

    #[test]
    fn a_recent_write_gets_the_floor_not_the_percentage() {
        // Seconds after the write, five percent of nothing is nothing, so the
        // 60s floor is what keeps a fresh one from reading as expired.
        let stamp = taken(10, 1_000_000);
        assert!(stamp.same_boot(10, 1_000_030));
        assert!(!stamp.same_boot(10, 1_000_200));
    }
}
