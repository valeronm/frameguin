//! System D-Bus daemon exposing privileged Framework laptop controls.
//!
//! Owns io.github.valeronm.Frameguin on the system bus and talks to the
//! embedded controller directly via framework_lib. Setters require the polkit
//! action io.github.valeronm.frameguin.manage. Exits after 5 idle
//! minutes; D-Bus activation restarts it on demand.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use framework_lib::chromium_ec::command::{EcCommands, EcRequestRaw};
use framework_lib::chromium_ec::commands::{
    EcRequestPwmGetKeyboardBacklight, FpLedBrightnessLevel,
};
use framework_lib::chromium_ec::{CrosEc, EcResult};
use framework_lib::touchpad::{self, ClickForce, HAPTIC_INTENSITY_LEVELS};
use zbus::message::Header;
use zbus::{fdo, interface, Connection};
use zbus_polkit::policykit1::{AuthorityProxy, CheckAuthorizationFlags, Subject};

const BUS_NAME: &str = "io.github.valeronm.Frameguin";
const OBJECT_PATH: &str = "/io/github/valeronm/Frameguin";
const POLKIT_ACTION: &str = "io.github.valeronm.frameguin.manage";
const IDLE_EXIT: Duration = Duration::from_secs(300);

struct Daemon {
    /// None on non-Framework hardware: CrosEc::new() panics outright when
    /// framework_lib finds no driver (empty driver list on e.g. aarch64
    /// without /dev/cros_ec), so it must not be constructed there.
    ec: Option<Mutex<CrosEc>>,
    authority: AuthorityProxy<'static>,
    last_used: Arc<Mutex<Instant>>,
    /// Probed once per daemon lifetime; the EC feature set can't change
    /// while running.
    capabilities: OnceLock<Vec<String>>,
    /// Haptic touchpad controls are write-only (firmware ACKs GET_FEATURE
    /// but returns zeros — verified on hardware), and the touchpad persists
    /// them in its own flash across suspend and reboot. So the daemon
    /// mirrors every write to a state file and reloads it at startup —
    /// no re-apply needed, the hardware keeps itself.
    haptic_intensity: AtomicU8,
    click_force: AtomicU8,
}

const DEFAULT_HAPTIC_INTENSITY: u8 = 75;
const DEFAULT_CLICK_FORCE: u8 = ClickForce::Medium as u8;
const STATE_FILE: &str = "/var/lib/frameguin/state";

/// Loads (haptic_intensity, click_force), falling back to the factory
/// defaults. A missing file on a machine whose touchpad was already changed
/// by other means will misreport until the first write — unavoidable, since
/// the hardware can't be read.
fn load_state() -> (u8, u8) {
    let mut intensity = DEFAULT_HAPTIC_INTENSITY;
    let mut force = DEFAULT_CLICK_FORCE;
    if let Ok(content) = std::fs::read_to_string(STATE_FILE) {
        for line in content.lines() {
            match line.split_once('=') {
                Some(("haptic_intensity", v)) => {
                    if let Ok(v) = v.trim().parse()
                        && HAPTIC_INTENSITY_LEVELS.contains(&v)
                    {
                        intensity = v;
                    }
                }
                Some(("click_force", v)) => {
                    if let Ok(v) = v.trim().parse()
                        && click_force_valid(v)
                    {
                        force = v;
                    }
                }
                _ => {}
            }
        }
    }
    (intensity, force)
}

// The directory is provisioned by StateDirectory= in the systemd unit.
fn save_state(intensity: u8, force: u8) {
    if let Err(e) = std::fs::write(
        STATE_FILE,
        format!("haptic_intensity={intensity}\nclick_force={force}\n"),
    ) {
        eprintln!("failed to persist state: {e}");
    }
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
        .map(|(name, _)| *name)
        .unwrap_or("medium")
}

fn click_force_valid(code: u8) -> bool {
    CLICK_FORCES.iter().any(|(_, force)| *force as u8 == code)
}

/// Known haptic touchpad models (PixArt PIDs). A curated device list, per
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

fn fp_level_name(level: Option<FpLedBrightnessLevel>) -> &'static str {
    match level {
        Some(FpLedBrightnessLevel::High) => "high",
        Some(FpLedBrightnessLevel::Medium) => "medium",
        Some(FpLedBrightnessLevel::Low) => "low",
        Some(FpLedBrightnessLevel::UltraLow) => "ultra-low",
        Some(FpLedBrightnessLevel::Auto) => "auto",
        Some(FpLedBrightnessLevel::Custom) | None => "custom",
    }
}

/// Without /dev/cros_ec, framework_lib falls back to raw port I/O; on a
/// non-Framework EC every command spin-waits to a timeout, stalling the
/// first GetCapabilities for tens of seconds. Don't touch the EC unless the
/// firmware says this is Framework hardware.
fn is_framework_hardware() -> bool {
    std::fs::read_to_string("/sys/class/dmi/id/sys_vendor")
        .is_ok_and(|vendor| vendor.trim() == "Framework")
}

fn haptic_touchpad_present() -> bool {
    hidapi::HidApi::new()
        .map(|api| {
            api.device_list().any(|dev| {
                dev.vendor_id() == touchpad::PIX_VID
                    && HAPTIC_TOUCHPAD_PIDS.contains(&dev.product_id())
            })
        })
        .unwrap_or(false)
}

/// framework_lib's get_keyboard_backlight() reads via PWM_GET_DUTY, and the
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

    fn ec_guard(&self) -> fdo::Result<std::sync::MutexGuard<'_, CrosEc>> {
        self.ec
            .as_ref()
            .map(|ec| ec.lock().unwrap())
            // NotSupported (not Failed): lets a caller distinguish "wrong
            // hardware, permanently" from a transient EC error.
            .ok_or_else(|| fdo::Error::NotSupported("no Framework EC on this hardware".into()))
    }

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
    async fn get_capabilities(&self) -> fdo::Result<Vec<String>> {
        self.touch();
        let caps = self.capabilities.get_or_init(|| {
            let mut caps = Vec::new();
            if let Some(ec) = &self.ec {
                let ec = ec.lock().unwrap();
                if ec.get_charge_limit().is_ok() {
                    caps.push("charge-limit".to_string());
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
        Ok(caps.clone())
    }

    async fn get_charge_limit(&self) -> fdo::Result<i32> {
        self.touch();
        let (_min, max) = self.ec_guard()?.get_charge_limit().map_err(ec_err)?;
        Ok(max as i32)
    }

    async fn set_charge_limit(
        &self,
        percent: i32,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        if !(20..=100).contains(&percent) {
            return Err(fdo::Error::InvalidArgs("charge limit must be 20-100".into()));
        }
        self.authorize(&header).await?;
        self.ec_guard()?
            .set_charge_limit(0, percent as u8)
            .map_err(ec_err)
    }

    async fn get_ec_version(&self) -> fdo::Result<String> {
        self.touch();
        self.ec_guard()?.version_info().map_err(ec_err)
    }

    /// Returns the brightness percentage and the level preset it came from:
    /// "high", "medium", "low", "ultra-low", "auto", or "custom" (the EC
    /// reports custom after any raw percentage write; it can't be set).
    async fn get_fingerprint_brightness(&self) -> fdo::Result<(i32, String)> {
        self.touch();
        let (percent, level) = self.ec_guard()?.get_fp_led_level().map_err(ec_err)?;
        Ok((percent as i32, fp_level_name(level).to_string()))
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
        percent: i32,
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
            .set_fp_led_percentage(percent as u8)
            .map_err(ec_err)
    }

    async fn get_haptic_intensity(&self) -> fdo::Result<i32> {
        self.touch();
        Ok(self.haptic_intensity.load(Ordering::Relaxed) as i32)
    }

    async fn set_haptic_intensity(
        &self,
        percent: i32,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        if !u8::try_from(percent).is_ok_and(|p| HAPTIC_INTENSITY_LEVELS.contains(&p)) {
            return Err(fdo::Error::InvalidArgs(format!(
                "intensity must be one of {HAPTIC_INTENSITY_LEVELS:?}"
            )));
        }
        self.authorize(&header).await?;
        touchpad::set_haptic_intensity(percent as u8).map_err(internal_err)?;
        self.haptic_intensity.store(percent as u8, Ordering::Relaxed);
        save_state(percent as u8, self.click_force.load(Ordering::Relaxed));
        Ok(())
    }

    async fn get_touchpad_click_force(&self) -> fdo::Result<String> {
        self.touch();
        Ok(click_force_name(self.click_force.load(Ordering::Relaxed)).to_string())
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
        save_state(self.haptic_intensity.load(Ordering::Relaxed), force as u8);
        Ok(())
    }

    async fn get_keyboard_backlight(&self) -> fdo::Result<i32> {
        self.touch();
        let percent = kbd_backlight_percent(&*self.ec_guard()?).map_err(ec_err)?;
        Ok(percent as i32)
    }

    async fn set_keyboard_backlight(
        &self,
        percent: i32,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.touch();
        if !(0..=100).contains(&percent) {
            return Err(fdo::Error::InvalidArgs("backlight must be 0-100".into()));
        }
        self.authorize(&header).await?;
        self.ec_guard()?.set_keyboard_backlight(percent as u8);
        Ok(())
    }
}

fn main() -> zbus::Result<()> {
    let last_used = Arc::new(Mutex::new(Instant::now()));
    let clock = last_used.clone();
    let (haptic_intensity, click_force) = load_state();
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
            haptic_intensity: AtomicU8::new(haptic_intensity),
            click_force: AtomicU8::new(click_force),
        };
        conn.object_server().at(OBJECT_PATH, daemon).await?;
        // Claim the name only once the object is served, so an activating
        // client can't call into a not-yet-registered path.
        conn.request_name(BUS_NAME).await?;
        Ok::<_, zbus::Error>(conn)
    })?;
    loop {
        std::thread::sleep(Duration::from_secs(60));
        if clock.lock().unwrap().elapsed() > IDLE_EXIT {
            return Ok(());
        }
    }
}
