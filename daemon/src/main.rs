//! System D-Bus daemon exposing privileged Framework laptop controls.
//!
//! Owns io.github.valeronm.Frameguin on the system bus and talks to the
//! embedded controller directly via `framework_lib`. Setters require the polkit
//! action io.github.valeronm.frameguin.manage. Exits after 5 idle
//! minutes; D-Bus activation restarts it on demand.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use framework_lib::chromium_ec::command::{EcCommands, EcRequestRaw};
use framework_lib::chromium_ec::commands::{
    ChargeStateCmd, EcRequestChargeStateGetV0, EcRequestGetUptimeInfo,
    EcRequestPwmGetKeyboardBacklight, FpLedBrightnessLevel,
};
use framework_lib::chromium_ec::{CrosEc, EcResult};
use framework_lib::power;
use framework_lib::touchpad::{self, ClickForce, HAPTIC_INTENSITY_LEVELS};
use zbus::message::Header;
use zbus::{fdo, interface, Connection};
use zbus_polkit::policykit1::{AuthorityProxy, CheckAuthorizationFlags, Subject};

const BUS_NAME: &str = "io.github.valeronm.Frameguin";
const OBJECT_PATH: &str = "/io/github/valeronm/Frameguin";
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
    capabilities: OnceLock<Vec<String>>,
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

/// The EC clamps each requested charge current against its limit, so the
/// largest value is what "no limit" means to it — and 0 means never charge.
const NO_CHARGE_CURRENT_LIMIT: u32 = u32::MAX;

/// A charge current limit together with the EC clock reading that dates it.
#[derive(Clone, Copy)]
struct ChargeCurrentLimit {
    milliamps: u32,
    /// Seconds the EC had been running when the limit was written, paired
    /// with the wall time of that same write.
    ec_uptime: u64,
    written_at: u64,
}

impl ChargeCurrentLimit {
    /// Whether the EC can still be holding this limit. An EC that has been up
    /// for less time than the write implies has restarted since, and a
    /// restart drops the limit. The comparison carries slack because the EC
    /// keeps its own time — its firmware documents 1% or worse frequency
    /// error against the host clock.
    ///
    /// EC uptime is a 32-bit millisecond counter, so this reads as a restart
    /// once every 49 days of EC uptime; the limit then shows as absent until
    /// it is set again.
    fn still_held(self, ec_uptime: u64, now: u64) -> bool {
        let expected = self.ec_uptime + now.saturating_sub(self.written_at);
        expected.saturating_sub(ec_uptime) <= (expected / 20).max(60)
    }
}

struct State {
    haptic_intensity: u8,
    click_force: u8,
    charge_current_limit: ChargeCurrentLimit,
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
            ec_uptime: 0,
            written_at: 0,
        },
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
                        && click_force_valid(v)
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
                    state.charge_current_limit.ec_uptime = value.parse().unwrap_or(0);
                }
                KEY_CURRENT_LIMIT_WRITTEN_AT => {
                    state.charge_current_limit.written_at = value.parse().unwrap_or(0);
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

/// Single source for the click-force name/value mapping.
const CLICK_FORCES: [(&str, ClickForce); 3] = [
    ("low", ClickForce::Low),
    ("medium", ClickForce::Medium),
    ("high", ClickForce::High),
];

fn click_force_from_name(name: &str) -> Option<ClickForce> {
    CLICK_FORCES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, force)| *force)
}

fn click_force_name(code: u8) -> &'static str {
    CLICK_FORCES
        .iter()
        .find(|(_, force)| *force as u8 == code)
        .map_or("medium", |(name, _)| *name)
}

fn click_force_valid(code: u8) -> bool {
    CLICK_FORCES.iter().any(|(_, force)| *force as u8 == code)
}

/// Known haptic touchpad models (`PixArt` PIDs). A curated device list, per
/// the probe rule: the haptic setters have no side-effect-free probe
/// (they're write-only, and every PTP touchpad accepts the open — only
/// haptic ones act on the reports). Keying on the touchpad's own HID
/// identity rather than the board name means a haptic pad retrofitted into
/// an older laptop is recognized. Extend when Framework ships new haptic
/// pads.
const HAPTIC_TOUCHPAD_PIDS: [u16; 1] = [0x1343];

fn fp_level_from_name(name: &str) -> Option<FpLedBrightnessLevel> {
    Some(match name {
        "high" => FpLedBrightnessLevel::High,
        "medium" => FpLedBrightnessLevel::Medium,
        "low" => FpLedBrightnessLevel::Low,
        "ultra-low" => FpLedBrightnessLevel::UltraLow,
        "auto" => FpLedBrightnessLevel::Auto,
        _ => return None,
    })
}

fn fp_level_name(level: Option<&FpLedBrightnessLevel>) -> &'static str {
    match level {
        Some(FpLedBrightnessLevel::High) => "high",
        Some(FpLedBrightnessLevel::Medium) => "medium",
        Some(FpLedBrightnessLevel::Low) => "low",
        Some(FpLedBrightnessLevel::UltraLow) => "ultra-low",
        Some(FpLedBrightnessLevel::Auto) => "auto",
        Some(FpLedBrightnessLevel::Custom) | None => "custom",
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
        let content = format!(
            "{KEY_HAPTIC_INTENSITY}={}\n{KEY_CLICK_FORCE}={}\n{KEY_CURRENT_LIMIT}={}\n\
             {KEY_CURRENT_LIMIT_UPTIME}={}\n{KEY_CURRENT_LIMIT_WRITTEN_AT}={}\n",
            self.haptic_intensity.load(Ordering::Relaxed),
            self.click_force.load(Ordering::Relaxed),
            limit.milliamps,
            limit.ec_uptime,
            limit.written_at,
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
    fn get_capabilities(&self) -> Vec<String> {
        self.touch();
        let caps = self.capabilities.get_or_init(|| {
            let mut caps = Vec::new();
            if let Some(ec) = &self.ec {
                let ec = ec.lock().unwrap();
                if ec.get_charge_limit().is_ok() {
                    caps.push("charge-limit".to_string());
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
                    caps.push("charge-current-limit".to_string());
                }
                if kbd_backlight_percent(&ec).is_ok() {
                    caps.push("keyboard-backlight".to_string());
                }
                if ec.get_fp_led_level().is_ok() {
                    caps.push("fp-brightness".to_string());
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
                        caps.push("fp-brightness-custom".to_string());
                    }
                }
            }
            // One name for both haptic controls: they share the identical
            // support condition (same device, same firmware feature set).
            if haptic_touchpad_present() {
                caps.push("haptic-touchpad".to_string());
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

    async fn set_charge_limit(
        &self,
        percent: u8,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        if !(20..=100).contains(&percent) {
            return Err(fdo::Error::InvalidArgs("charge limit must be 20-100".into()));
        }
        self.authorize(&header).await?;
        self.ec_guard()?
            .set_charge_limit(0, percent)
            .map_err(ec_err)
    }

    /// How fast the battery may charge, in mA, or `NO_CHARGE_CURRENT_LIMIT`
    /// when nothing caps it. The EC cannot be asked what it holds, so this is
    /// what the daemon last wrote, and it reports no limit once the EC has
    /// restarted and dropped the value.
    fn get_charge_current_limit(&self) -> fdo::Result<u32> {
        self.touch();
        let ec = self.ec_guard()?;
        let limit = *self.charge_current_limit.lock().unwrap();
        // Nothing mirrored is already the answer, so don't spend an EC round
        // trip dating it.
        if limit.milliamps == NO_CHARGE_CURRENT_LIMIT {
            return Ok(NO_CHARGE_CURRENT_LIMIT);
        }
        let ec_uptime = ec_uptime_secs(&ec).map_err(ec_err)?;
        if limit.still_held(ec_uptime, unix_now()) {
            Ok(limit.milliamps)
        } else {
            Ok(NO_CHARGE_CURRENT_LIMIT)
        }
    }

    /// The battery's design capacity in mAh, which is numerically also the
    /// current that charges it at 1C — what turns a charge speed expressed as
    /// a fraction of full rate into the milliamps the EC wants.
    fn get_battery_design_capacity(&self) -> fdo::Result<u32> {
        self.touch();
        let ec = self.ec_guard()?;
        self.read_design_capacity(&ec)
            .ok_or_else(|| fdo::Error::Failed("no battery present".into()))
    }

    /// What the charger is pushing into the battery right now, in mA, and 0
    /// whenever it isn't charging. This is the only observable effect a
    /// charge current limit has, the limit itself being unreadable.
    fn get_charge_current(&self) -> fdo::Result<u32> {
        self.touch();
        let state = EcRequestChargeStateGetV0 {
            cmd: ChargeStateCmd::GetState as u8,
            param: 0,
        }
        .send_command(&*self.ec_guard()?)
        .map_err(ec_err)?;
        Ok(state.chg_current)
    }

    /// Caps how fast the battery charges, in mA; `NO_CHARGE_CURRENT_LIMIT`
    /// lifts the cap. Zero is refused: the EC clamps its requested current
    /// against this value, so zero stops charging altogether rather than
    /// meaning "unrestricted", and nothing would report that back.
    async fn set_charge_current_limit(
        &self,
        milliamps: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        if milliamps == 0 {
            return Err(fdo::Error::InvalidArgs(format!(
                "0 stops charging; pass {NO_CHARGE_CURRENT_LIMIT} to remove the limit"
            )));
        }
        self.authorize(&header).await?;
        let ec_uptime = {
            let ec = self.ec_guard()?;
            // Always the unconditional form. The command's state-of-charge
            // variant latches inside the EC: once applied it is never
            // re-evaluated, so a later threshold cannot lift it
            // (framework-system issue #342).
            ec.set_charge_current_limit(milliamps, None)
                .map_err(ec_err)?;
            ec_uptime_secs(&ec).map_err(ec_err)?
        };
        *self.charge_current_limit.lock().unwrap() = ChargeCurrentLimit {
            milliamps,
            ec_uptime,
            written_at: unix_now(),
        };
        self.save_state();
        Ok(())
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

    /// Returns the brightness percentage and the level preset it came from:
    /// "high", "medium", "low", "ultra-low", "auto", or "custom" (the EC
    /// reports custom after any raw percentage write; it can't be set).
    fn get_fingerprint_brightness(&self) -> fdo::Result<(u8, String)> {
        self.touch();
        let (percent, level) = self.ec_guard()?.get_fp_led_level().map_err(ec_err)?;
        Ok((percent, fp_level_name(level.as_ref()).to_string()))
    }

    async fn set_fingerprint_level(
        &self,
        level: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        let level = fp_level_from_name(&level).ok_or_else(|| {
            fdo::Error::InvalidArgs(format!(
                "unknown level {level:?}; expected high/medium/low/ultra-low/auto"
            ))
        })?;
        self.authorize(&header).await?;
        self.ec_guard()?.set_fp_led_level(level).map_err(ec_err)
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
        self.ec_guard()?
            .set_fp_led_percentage(percent)
            .map_err(ec_err)
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

    fn get_touchpad_click_force(&self) -> String {
        self.touch();
        click_force_name(self.click_force.load(Ordering::Relaxed)).to_string()
    }

    async fn set_touchpad_click_force(
        &self,
        force: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        let force = click_force_from_name(&force).ok_or_else(|| {
            fdo::Error::InvalidArgs(format!(
                "unknown click force {force:?}; expected low/medium/high"
            ))
        })?;
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
            design_capacity: OnceLock::new(),
        };
        conn.object_server().at(OBJECT_PATH, daemon).await?;
        // Claim the name only once the object is served, so an activating
        // client can't call into a not-yet-registered path.
        conn.request_name(BUS_NAME).await?;
        Ok::<_, zbus::Error>(conn)
    })?;
    loop {
        std::thread::sleep(Duration::from_mins(1));
        if clock.lock().unwrap().elapsed() > IDLE_EXIT {
            return Ok(());
        }
    }
}
