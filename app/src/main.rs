//! GTK4/libadwaita front-end for Framework laptop controls.
//!
//! Hardware controls go through the frameguin daemon on the system
//! bus; board/BIOS info is read directly from world-readable DMI sysfs.

use std::cell::{Cell, RefCell};
use std::fs;
use std::ops::ControlFlow;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;

const APP_ID: &str = "io.github.valeronm.Frameguin";
const SLIDER_DEBOUNCE: Duration = Duration::from_millis(200);
/// Keys and the wheel on a slider that otherwise writes only when a drag
/// ends. Longer than the live sliders wait, for the same reason that one
/// writes on release: nothing shows the values passed through, and each of
/// them would be another authorized EC write.
const SETTLE_DEBOUNCE: Duration = Duration::from_millis(700);
const KBD_SYNC_SECONDS: u32 = 2;
const CHARGE_CURRENT_SECONDS: u32 = 2;

#[zbus::proxy(
    interface = "io.github.valeronm.Frameguin1",
    default_service = "io.github.valeronm.Frameguin",
    default_path = "/io/github/valeronm/Frameguin"
)]
trait Frameguin {
    async fn get_charge_limit(&self) -> zbus::Result<u8>;
    async fn set_charge_limit(&self, percent: u8) -> zbus::Result<()>;
    async fn get_charge_current_limit(&self) -> zbus::Result<u32>;
    async fn set_charge_current_limit(&self, milliamps: u32) -> zbus::Result<()>;
    async fn get_battery_design_capacity(&self) -> zbus::Result<u32>;
    async fn get_charge_current(&self) -> zbus::Result<u32>;
    async fn get_keyboard_backlight(&self) -> zbus::Result<u8>;
    async fn set_keyboard_backlight(&self, percent: u8) -> zbus::Result<()>;
    async fn get_capabilities(&self) -> zbus::Result<Vec<String>>;
    async fn get_ec_version(&self) -> zbus::Result<String>;
    async fn get_build(&self) -> zbus::Result<(String, String)>;
    async fn get_fingerprint_brightness(&self) -> zbus::Result<(u8, String)>;
    async fn set_fingerprint_brightness(&self, percent: u8) -> zbus::Result<()>;
    async fn set_fingerprint_level(&self, level: &str) -> zbus::Result<()>;
    async fn get_haptic_intensity(&self) -> zbus::Result<u8>;
    async fn set_haptic_intensity(&self, percent: u8) -> zbus::Result<()>;
    async fn get_touchpad_click_force(&self) -> zbus::Result<String>;
    async fn set_touchpad_click_force(&self, force: &str) -> zbus::Result<()>;
}

/// GTK carries adjustment values as f64. The cast alone saturates at 255, so
/// the clamp is what holds the result inside the range the daemon accepts;
/// each control's own floor is enforced by its adjustment.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped into range before the cast"
)]
fn scale_percent(value: f64) -> u8 {
    value.round().clamp(0.0, 100.0) as u8
}

/// Milliamps as the amps a person reads off a charger.
fn amps(milliamps: u32) -> String {
    format!("{:.1} A", f64::from(milliamps) / 1000.0)
}

/// The milliamps a charge speed asks the daemon for. Shared by the window and
/// the tray so the two can't disagree about what "Half" sends.
fn charge_speed_milliamps(design_capacity: u32, index: usize) -> u32 {
    match CHARGE_SPEEDS.get(index).copied().flatten() {
        Some(divisor) => design_capacity / divisor,
        None => NO_CHARGE_CURRENT_LIMIT,
    }
}

/// Which speed a limit corresponds to, and `None` when it matches no preset —
/// `framework_tool` can set any value, and guessing the nearest would
/// misreport it.
fn charge_speed_position(design_capacity: u32, milliamps: u32) -> Option<usize> {
    (0..CHARGE_SPEEDS.len()).find(|&index| charge_speed_milliamps(design_capacity, index) == milliamps)
}

/// Combo labels carrying the rate each fraction works out to — "Half" alone
/// doesn't say half of what.
fn charge_speed_labels(design_capacity: u32) -> Vec<String> {
    CHARGE_SPEEDS
        .iter()
        .zip(CHARGE_SPEED_LABELS)
        .map(|(divisor, label)| match divisor {
            Some(divisor) => format!("{label} ({})", amps(design_capacity / divisor)),
            None => label.to_string(),
        })
        .collect()
}

/// GTK carries the slider's value as f64; the clamp is what holds the result
/// inside what the daemon accepts, its floor coming from the adjustment.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped into range before the cast"
)]
fn scale_milliamps(value: f64) -> u32 {
    let snapped = (value / CUSTOM_CHARGE_STEP_MA).round() * CUSTOM_CHARGE_STEP_MA;
    snapped.clamp(MIN_CUSTOM_CHARGE_MA, f64::from(u32::MAX)) as u32
}

/// Which row a value belongs on. A value matching no preset can only be shown
/// by the slider, so it lands on Custom whatever the caller asked for.
/// Otherwise `Custom::Keep` leaves a combo that is already on Custom there, so
/// dialling in a number that happens to equal a preset doesn't fold the slider
/// away mid-drag.
fn custom_or(
    combo: &adw::ComboRow,
    custom_index: usize,
    preset: Option<usize>,
    custom: Custom,
) -> usize {
    let Some(preset) = preset else {
        return custom_index;
    };
    if custom == Custom::Keep && combo.selected() == combo_index(custom_index) {
        custom_index
    } else {
        preset
    }
}

/// The chrome every slider in this window shares, so a change to how one
/// reads doesn't have to be made four times. `format` renders the value in
/// the control's own unit, which is the only part that differs.
fn build_scale(
    adjustment: &gtk::Adjustment,
    format: impl Fn(f64) -> String + 'static,
) -> gtk::Scale {
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(adjustment));
    scale.set_size_request(180, -1);
    scale.set_valign(gtk::Align::Center);
    scale.set_draw_value(true);
    scale.set_value_pos(gtk::PositionType::Left);
    scale.set_format_value_func(move |_, value| format(value));
    scale.set_sensitive(false);
    scale
}

/// A `StringList` from owned labels, which every combo here builds from a
/// `Vec<String>` that GTK will only take as borrowed strs.
fn string_list(labels: &[String]) -> gtk::StringList {
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    gtk::StringList::new(&labels)
}

/// A combo's rows: the presets, then the one that reveals a slider. Both
/// preset-plus-custom controls build their model this way, so neither can
/// leave the extra row off and address it anyway.
fn with_custom_row(mut labels: Vec<String>) -> Vec<String> {
    labels.push("Custom".to_string());
    labels
}

/// Names a combo row for GTK, which addresses rows by u32. Positions come
/// from fixed arrays of a handful of entries, so the fallback is unreachable
/// — it is what keeps the conversion total without a cast.
fn combo_index(position: usize) -> u32 {
    u32::try_from(position).unwrap_or(gtk::INVALID_LIST_POSITION)
}

/// The charge speeds the combo offers, as the divisor applied to the
/// battery's 1C design current. `None` is full speed, which the daemon takes
/// as no limit at all.
const CHARGE_SPEEDS: [Option<u32>; 3] = [None, Some(2), Some(4)];
const CHARGE_SPEED_LABELS: [&str; 3] = ["Full speed", "Half", "Quarter"];

/// The window's combo carries one row past the presets, for a rate the user
/// dials in. The tray offers only the presets: a slider has no menu form, and
/// a preset menu that can't reach every state is the honest half.
const CHARGE_SPEED_CUSTOM: usize = CHARGE_SPEEDS.len();

/// The slowest the custom slider will ask for. The EC takes anything above
/// zero, but a limit this side of it charges so slowly that it reads as a
/// fault rather than a setting.
const MIN_CUSTOM_CHARGE_MA: f64 = 100.0;

/// What the custom slider rounds to. A `GtkScale` is continuous while
/// dragged — its step increment reaches only keys and the wheel — so without
/// this a drag lands on a value like 984 mA that the row then displays as
/// "1.0 A", reporting a current nobody chose.
const CUSTOM_CHARGE_STEP_MA: f64 = 100.0;

/// The daemon's "charge as fast as the battery asks".
const NO_CHARGE_CURRENT_LIMIT: u32 = u32::MAX;

/// The steps the Boreas haptic firmware implements, and the click-force
/// names the daemon accepts.
const HAPTIC_LEVELS: [u8; 5] = [0, 25, 50, 75, 100];
const HAPTIC_LABELS: [&str; 5] = ["Off", "25%", "50%", "75%", "100%"];
const CLICK_FORCES: [&str; 3] = ["low", "medium", "high"];
const CLICK_FORCE_LABELS: [&str; 3] = ["Low", "Medium", "High"];

/// What the connected board supports, per the daemon's probe. The default
/// (all false) doubles as "not yet known".
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per wire capability, and a board can carry any subset \
              — independent answers, not the exclusive states an enum models"
)]
#[derive(Clone, Copy, Default)]
struct Capabilities {
    charge_limit: bool,
    charge_current_limit: bool,
    keyboard_backlight: bool,
    fp_brightness: bool,
    /// V1 of the EC command: raw percentage plus the ultra-low/auto levels.
    /// Old firmware supports only high/medium/low (framework-system #211).
    fp_custom: bool,
    haptic_touchpad: bool,
}

impl Capabilities {
    fn from_names(names: &[String]) -> Self {
        let has = |name: &str| names.iter().any(|n| n == name);
        Capabilities {
            charge_limit: has("charge-limit"),
            charge_current_limit: has("charge-current-limit"),
            keyboard_backlight: has("keyboard-backlight"),
            fp_brightness: has("fp-brightness"),
            fp_custom: has("fp-brightness-custom"),
            haptic_touchpad: has("haptic-touchpad"),
        }
    }
}

// --- tray icon (StatusNotifierItem; shown by GNOME's AppIndicator extension) ---

enum TrayEvent {
    Show,
    Refresh,
    SetChargeLimit(u8),
    /// Already resolved to milliamps against the capacity the menu was drawn
    /// from, so applying it needs nothing the window has to supply.
    SetChargeSpeed(u32),
    SetFingerprintLevel(&'static str),
    Quit,
}

const CHARGE_PRESETS: [u8; 3] = [60, 80, 100];

/// The window's combo carries one row past the presets, for a ceiling the
/// user dials in; the tray offers the presets alone.
const CHARGE_LIMIT_CUSTOM: usize = CHARGE_PRESETS.len();

/// The lowest ceiling the daemon accepts, and so the slider's floor.
const MIN_CHARGE_LIMIT: f64 = 20.0;

/// What a value landing on a preset should do to a combo sitting on Custom.
/// Only a slider write keeps it: the user is dialling a number in, and a
/// number that happens to equal a preset shouldn't fold the slider away
/// under them. Everything else — a preset picked here or in the tray, a
/// reload of what the hardware actually holds — re-derives the row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Custom {
    Keep,
    Rederive,
}

/// Preset names, shared so the window's combo and the tray's menu can't
/// disagree about what a ceiling is called. The window's combo appends
/// "Custom"; the tray's menu takes these as they are.
fn charge_limit_labels() -> Vec<String> {
    CHARGE_PRESETS
        .iter()
        // A 100% ceiling is no limit at all — say so.
        .map(|percent| {
            if *percent == 100 {
                "No limit".to_string()
            } else {
                format!("{percent}%")
            }
        })
        .collect()
}

/// Daemon-side level names, in the order the fingerprint combo shows them.
/// "custom" is get-only: the EC reports it after a raw percentage write.
/// Firmware without the fp-brightness-custom capability supports only the
/// BASIC slice (high/medium/low).
const FP_LEVELS: [&str; 6] = ["auto", "high", "medium", "low", "ultra-low", "custom"];
const FP_LEVEL_LABELS: [&str; 6] = ["Auto", "High", "Medium", "Low", "Ultra-low", "Custom"];
const FP_BASIC: std::ops::Range<usize> = 1..4;

/// Combo levels and labels for a firmware generation; tray presets are the
/// same minus the trailing "custom" in the full set.
fn fp_levels_labels(custom: bool) -> (&'static [&'static str], &'static [&'static str]) {
    if custom {
        (&FP_LEVELS, &FP_LEVEL_LABELS)
    } else {
        (&FP_LEVELS[FP_BASIC], &FP_LEVEL_LABELS[FP_BASIC])
    }
}

struct TrayIcon {
    tx: async_channel::Sender<TrayEvent>,
    /// Currently applied charge limit, pushed in from the app so the radio
    /// group can mark it; None until the first daemon read.
    charge_limit: Option<u8>,
    /// The charge current limit in mA and the battery capacity that names the
    /// speeds, both pushed in from the app. Without the capacity the submenu
    /// stays out: a fraction then has no rate to show or to send.
    charge_current_limit: Option<u32>,
    design_capacity: Option<u32>,
    /// Current fingerprint LED level name, pushed in from the app; "custom"
    /// marks no radio option.
    fp_level: Option<String>,
    /// Pushed in once the app reads the daemon's probe, and fixed for the
    /// daemon's run thereafter. None until then, which leaves the menu at
    /// Open/Quit.
    caps: Option<Capabilities>,
}

impl TrayIcon {
    fn send(&self, event: TrayEvent) {
        let _ = self.tx.send_blocking(event);
    }
}

impl ksni::Tray for TrayIcon {
    fn id(&self) -> String {
        APP_ID.into()
    }

    fn icon_name(&self) -> String {
        format!("{APP_ID}-symbolic")
    }

    fn title(&self) -> String {
        "Frameguin".into()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayEvent::Show);
    }

    /// The menu renders from values pushed in earlier, which the EC and other
    /// tools can invalidate at any time, so opening it asks for fresh ones.
    /// They arrive a moment later and ksni re-renders once they land.
    fn menu_about_to_show(&mut self) {
        self.send(TrayEvent::Refresh);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let mut items: Vec<ksni::MenuItem<Self>> = vec![
            StandardItem {
                label: "Open".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::Show)),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
        ];
        // Each control decides for itself whether this board can offer it, so
        // the trailing separator asks the list rather than restating the
        // conditions.
        let controls = [
            self.charge_limit_item(),
            self.charge_speed_item(),
            self.fp_level_item(),
        ];
        let any_control = controls.iter().any(Option::is_some);
        items.extend(controls.into_iter().flatten());
        if any_control {
            items.push(ksni::MenuItem::Separator);
        }
        items.push(
            StandardItem {
                label: "Quit Frameguin".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::Quit)),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}

/// One shape for every preset menu the tray offers: a submenu named after the
/// active option, holding a radio group over all of them. `selected` is None
/// when the hardware sits on no preset, which leaves the group unmarked.
fn radio_submenu(
    title: String,
    selected: Option<usize>,
    labels: Vec<String>,
    select: impl Fn(&mut TrayIcon, usize) + Send + 'static,
) -> ksni::MenuItem<TrayIcon> {
    use ksni::menu::{RadioGroup, RadioItem, SubMenu};
    SubMenu {
        label: title,
        submenu: vec![RadioGroup {
            selected: selected.unwrap_or(usize::MAX),
            select: Box::new(select),
            options: labels
                .into_iter()
                .map(|label| RadioItem {
                    label,
                    ..Default::default()
                })
                .collect(),
        }
        .into()],
        ..Default::default()
    }
    .into()
}

impl TrayIcon {
    fn charge_limit_item(&self) -> Option<ksni::MenuItem<Self>> {
        if !self.caps?.charge_limit {
            return None;
        }
        let labels = charge_limit_labels();
        let title = match self.charge_limit {
            Some(100) => "Charge limit (off)".into(),
            Some(limit) => format!("Charge limit ({limit}%)"),
            None => "Charge limit".into(),
        };
        let selected = CHARGE_PRESETS
            .iter()
            .position(|p| Some(*p) == self.charge_limit);
        Some(radio_submenu(title, selected, labels, |tray, index| {
            tray.send(TrayEvent::SetChargeLimit(CHARGE_PRESETS[index]));
        }))
    }

    fn charge_speed_item(&self) -> Option<ksni::MenuItem<Self>> {
        if !self.caps?.charge_current_limit {
            return None;
        }
        // Still needed to turn the chosen speed into the milliamps the daemon
        // takes, even though the menu names speeds rather than currents.
        let design_capacity = self.design_capacity?;
        // Bare preset names, not the window's `charge_speed_labels`: those
        // carry the rate in brackets, which would nest inside the submenu
        // title's own brackets.
        let labels = CHARGE_SPEED_LABELS.iter().map(|l| (*l).to_string()).collect();
        let selected = self
            .charge_current_limit
            .and_then(|milliamps| charge_speed_position(design_capacity, milliamps));
        // Named by its preset where there is one, and by the current itself
        // where there isn't — a menu that can only show presets would say
        // nothing at all about a limit dialled in from the window.
        let title = match (selected, self.charge_current_limit) {
            (Some(index), _) => format!("Charge speed ({})", CHARGE_SPEED_LABELS[index]),
            (None, Some(milliamps)) => format!("Charge speed ({})", amps(milliamps)),
            (None, None) => "Charge speed".into(),
        };
        Some(radio_submenu(title, selected, labels, move |tray, index| {
            tray.send(TrayEvent::SetChargeSpeed(charge_speed_milliamps(
                design_capacity,
                index,
            )));
        }))
    }

    fn fp_level_item(&self) -> Option<ksni::MenuItem<Self>> {
        let caps = self.caps?;
        if !caps.fp_brightness {
            return None;
        }
        // Presets only — "custom" is a state the EC reports, not an action,
        // so the tray offers just the settable levels of this firmware
        // generation.
        let (mut levels, mut labels) = fp_levels_labels(caps.fp_custom);
        if let Some(stripped) = levels.strip_suffix(&["custom"]) {
            levels = stripped;
            labels = &labels[..levels.len()];
        }
        let selected = self
            .fp_level
            .as_deref()
            .and_then(|level| levels.iter().position(|l| *l == level));
        let title = match selected {
            Some(index) => format!("Fingerprint LED ({})", labels[index]),
            None => "Fingerprint LED".into(),
        };
        let options = labels.iter().map(|label| (*label).to_string()).collect();
        Some(radio_submenu(title, selected, options, move |tray, index| {
            tray.send(TrayEvent::SetFingerprintLevel(levels[index]));
        }))
    }
}

// --- about ---

fn dmi(file: &str) -> String {
    fs::read_to_string(format!("/sys/class/dmi/id/{file}"))
        .map_or_else(|_| "unknown".into(), |s| s.trim().to_string())
}

/// What a hardware report needs, behind the About window's copy button, so
/// filing one does not require busctl. Both binaries report where they ran
/// from: a mixed install has the app under one prefix and the daemon under
/// another, and no version comparison would show it when the two trees hold
/// the same release.
#[expect(
    clippy::format_push_string,
    reason = "the allocation is immaterial in a report built once to fill a dialog"
)]
async fn debug_info() -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| "unknown".into());
    let mut out = format!(
        "Frameguin {} ({})\n",
        env!("CARGO_PKG_VERSION"),
        exe.display()
    );

    let line = |name: &str, value: zbus::Result<String>| match value {
        Ok(v) => format!("{name}: {v}\n"),
        Err(e) => format!("{name}: unavailable ({e})\n"),
    };

    // The two binaries first and adjacent, since comparing their paths is
    // what the report is read for; the hardware they found follows.
    let proxy = daemon_proxy().await;
    match &proxy {
        Ok(p) => out.push_str(&line(
            "daemon",
            p.get_build().await.map(|(v, path)| format!("{v} ({path})")),
        )),
        Err(e) => out.push_str(&format!("daemon: unreachable ({e})\n")),
    }

    out.push_str(&format!(
        "board: {}\nBIOS: {}\n",
        dmi("product_name"),
        dmi("bios_version")
    ));

    if let Ok(p) = &proxy {
        out.push_str(&line("EC", p.get_ec_version().await));
        out.push_str(&line(
            "capabilities",
            p.get_capabilities().await.map(|c| c.join(" ")),
        ));
    }
    out
}

fn show_about(parent: Option<&gtk::Window>) {
    let about = adw::AboutWindow::builder()
        .application_icon(APP_ID)
        .application_name("Frameguin")
        .developer_name("Valerii Myronov")
        .version(env!("CARGO_PKG_VERSION"))
        .license_type(gtk::License::MitX11)
        // Setting the comments property would create a Details page and move
        // the website link onto it, off the main page.
        .website(env!("CARGO_PKG_HOMEPAGE"))
        .issue_url(concat!(env!("CARGO_PKG_REPOSITORY"), "/issues"))
        .debug_info_filename("frameguin-debug-info.txt")
        // Placeholder rather than empty: libadwaita hides the Troubleshooting
        // page entirely when debug info is blank, and this fills in later.
        .debug_info("collecting…")
        .build();
    about.set_transient_for(parent);

    let filling = about.clone();
    glib::spawn_future_local(async move {
        let info = debug_info().await;
        filling.set_debug_info(&info);
    });
    about.present();
}

// --- autostart ---

fn autostart_entry_path() -> std::path::PathBuf {
    glib::user_config_dir().join(format!("autostart/{APP_ID}.desktop"))
}

/// Names the binary rather than a path, so the entry survives a move between
/// install prefixes; `TryExec` lets the session skip it once Frameguin is gone,
/// which no uninstaller can do for a file in someone's home directory.
fn autostart_entry() -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Frameguin\n\
         Comment=Framework laptop controls in the tray\n\
         TryExec=frameguin\n\
         Exec=frameguin --gapplication-service\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         NoDisplay=true\n"
    )
}

fn set_autostart(enabled: bool) -> std::io::Result<()> {
    let path = autostart_entry_path();
    if enabled {
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, autostart_entry())
    } else {
        match fs::remove_file(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

// --- UI ---

struct Ui {
    toasts: adw::ToastOverlay,
    limit_combo: adw::ComboRow,
    limit_scale: gtk::Scale,
    /// The slider's row; shown only while the ceiling is Custom.
    limit_custom_row: adw::ActionRow,
    speed_combo: adw::ComboRow,
    speed_scale: gtk::Scale,
    /// The slider's row; shown only while the speed is Custom.
    speed_custom_row: adw::ActionRow,
    /// The battery's design capacity in mAh, None until read. Numerically it
    /// is the 1C current, which is what turns the combo's fractions into the
    /// milliamps the daemon takes.
    design_capacity: Cell<Option<u32>>,
    /// What the daemon said this board supports, so a later reload knows
    /// which values to ask for.
    caps: Cell<Capabilities>,
    /// Set while widgets are being moved to mirror the hardware, so their
    /// change handlers don't echo the reading back as a write.
    syncing: Cell<bool>,
    kbd_scale: gtk::Scale,
    fp_scale: gtk::Scale,
    fp_combo: adw::ComboRow,
    /// The slider's row; shown only while the level is Custom.
    fp_custom_row: adw::ActionRow,
    /// The level names behind the combo's rows, set once capabilities are
    /// known (full set, or high/medium/low on v0-only firmware).
    fp_levels: Cell<&'static [&'static str]>,
    haptic_combo: adw::ComboRow,
    force_combo: adw::ComboRow,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
}

impl Ui {
    fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    /// Moves widgets to match the hardware without their handlers writing the
    /// value straight back. Every setter returns early while this is set.
    fn sync(&self, update: impl FnOnce()) {
        self.syncing.set(true);
        update();
        self.syncing.set(false);
    }

    fn sync_tray_charge_limit(&self, percent: u8) {
        if let Some(handle) = &self.tray {
            tray_set_charge_limit(handle, percent);
        }
    }

    fn sync_tray_fp_level(&self, level: &str) {
        if let Some(handle) = &self.tray {
            tray_set_fp_level(handle, level);
        }
    }

    /// Moves the charge-limit widgets onto a ceiling without writing it back,
    /// the counterpart of [`Ui::show_charge_speed`] — change one and read the
    /// other.
    fn show_charge_limit(&self, percent: u8, custom: Custom) {
        let preset = CHARGE_PRESETS.iter().position(|p| *p == percent);
        let index = custom_or(&self.limit_combo, CHARGE_LIMIT_CUSTOM, preset, custom);
        self.sync(|| {
            self.limit_combo.set_selected(combo_index(index));
            self.limit_custom_row
                .set_visible(index == CHARGE_LIMIT_CUSTOM);
            self.limit_scale.set_value(f64::from(percent));
        });
    }

    /// Moves the charge-speed widgets onto a limit without writing it back.
    /// Shared by the reload and the write, so the combo, the slider and the
    /// slider's visibility can't disagree about which one is in effect.
    fn show_charge_speed(&self, milliamps: u32, custom: Custom) {
        let Some(capacity) = self.design_capacity.get() else {
            self.sync(|| self.speed_combo.set_selected(gtk::INVALID_LIST_POSITION));
            return;
        };
        let preset = charge_speed_position(capacity, milliamps);
        let index = custom_or(&self.speed_combo, CHARGE_SPEED_CUSTOM, preset, custom);
        self.sync(|| {
            self.speed_combo.set_selected(combo_index(index));
            self.speed_custom_row
                .set_visible(index == CHARGE_SPEED_CUSTOM);
            // Full speed is the absence of a limit, not a position on a
            // slider that can only express one.
            if milliamps != NO_CHARGE_CURRENT_LIMIT {
                self.speed_scale.set_value(f64::from(milliamps));
            }
        });
    }

    fn sync_tray_charge_speed(&self, milliamps: u32) {
        if let Some(handle) = &self.tray {
            tray_set_charge_speed(handle, self.design_capacity.get(), milliamps);
        }
    }

    fn fp_combo_index(&self, level: &str) -> u32 {
        self.fp_levels
            .get()
            .iter()
            .position(|l| *l == level)
            .map_or(gtk::INVALID_LIST_POSITION, combo_index)
    }

    fn sync_tray_caps(&self, caps: Capabilities) {
        if let Some(handle) = &self.tray {
            tray_set_caps(handle, caps);
        }
    }
}

// The tray-push protocol, owned in one place: both the window's Ui and the
// tray-only startup path go through these.
fn tray_set_caps(handle: &ksni::blocking::Handle<TrayIcon>, caps: Capabilities) {
    handle.update(move |tray| tray.caps = Some(caps));
}

fn tray_set_charge_limit(handle: &ksni::blocking::Handle<TrayIcon>, percent: u8) {
    handle.update(move |tray| tray.charge_limit = Some(percent));
}

fn tray_set_charge_speed(
    handle: &ksni::blocking::Handle<TrayIcon>,
    design_capacity: Option<u32>,
    milliamps: u32,
) {
    handle.update(move |tray| {
        // A write from a window that hasn't read the battery yet knows the
        // milliamps but not the capacity, and the menu's own copy is the
        // better one — never trade a known capacity for None.
        if design_capacity.is_some() {
            tray.design_capacity = design_capacity;
        }
        tray.charge_current_limit = Some(milliamps);
    });
}

fn tray_set_fp_level(handle: &ksni::blocking::Handle<TrayIcon>, level: &str) {
    let level = level.to_string();
    handle.update(move |tray| tray.fp_level = Some(level));
}

/// The app's one way of attaching to the daemon.
async fn daemon_proxy() -> zbus::Result<FrameguinProxy<'static>> {
    let conn = zbus::Connection::system().await?;
    FrameguinProxy::new(&conn).await
}

/// (Re)arms a debounce slot: cancels any pending source and schedules `action`
/// after `delay`.
fn debounce(
    slot: &Rc<RefCell<Option<glib::SourceId>>>,
    delay: Duration,
    action: impl FnOnce() + 'static,
) {
    if let Some(source) = slot.borrow_mut().take() {
        source.remove();
    }
    let cleared = slot.clone();
    let id = glib::timeout_add_local_once(delay, move || {
        cleared.replace(None);
        action();
    });
    slot.replace(Some(id));
}

// Widgets for absent capabilities stay hidden and insensitive, so their
// handlers can never fire; connecting unconditionally keeps one wiring path.
fn connect_setters(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    connect_charge_setter(ui, proxy);
    connect_charge_speed_setter(ui, proxy);
    connect_kbd_setter(ui, proxy);
    connect_fp_setter(ui, proxy);
    connect_touchpad_setters(ui, proxy);
}

/// Wires a slider whose value reaches the hardware when a drag ends rather
/// than as it moves: these controls show nothing while they change, and every
/// value passed through would be one more authorized EC write. Keyboard and
/// wheel changes raise no release, so they settle on a debounce instead.
///
/// `read` turns the slider's position into the value that gets written, and
/// is also what decides whether a drag moved at all — comparing positions
/// would count a nudge that rounds back to where it started.
fn connect_slider_writes<T: Copy + PartialEq + 'static>(
    ui: &Rc<Ui>,
    scale: &gtk::Scale,
    read: impl Fn(f64) -> T + 'static,
    write: impl Fn(T) + 'static,
) {
    let read = Rc::new(read);
    let write = Rc::new(write);
    let dragging: Rc<Cell<Option<T>>> = Rc::new(Cell::new(None));

    let slot = Rc::new(RefCell::new(None));
    let keys_ui = ui.clone();
    let keys_dragging = dragging.clone();
    let (keys_read, keys_write) = (read.clone(), write.clone());
    scale.connect_value_changed(move |scale| {
        if keys_ui.syncing.get() || keys_dragging.get().is_some() {
            return;
        }
        let value = keys_read(scale.value());
        let write = keys_write.clone();
        debounce(&slot, SETTLE_DEBOUNCE, move || write(value));
    });

    // Raw events, not a gesture: the scale's own drag gesture claims the
    // pointer sequence, which cancels any competing gesture instead of
    // releasing it — so a GestureClick here would see the press and never the
    // release, and the drag would never end.
    let drag = gtk::EventControllerLegacy::new();
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    let drag_scale = scale.clone();
    drag.connect_event(move |_, event| {
        match event.event_type() {
            gtk::gdk::EventType::ButtonPress | gtk::gdk::EventType::TouchBegin => {
                dragging.set(Some(read(drag_scale.value())));
            }
            gtk::gdk::EventType::ButtonRelease
            | gtk::gdk::EventType::TouchEnd
            | gtk::gdk::EventType::TouchCancel => {
                let value = read(drag_scale.value());
                // A press that lands where the handle already sat changes
                // nothing, and writing it would announce a value nobody moved.
                if dragging.replace(None) != Some(value) {
                    write(value);
                }
            }
            _ => {}
        }
        glib::Propagation::Proceed
    });
    scale.add_controller(drag);
}

fn connect_charge_speed_setter(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let speed_ui = ui.clone();
    let speed_proxy = proxy.clone();
    ui.speed_combo.connect_selected_notify(move |row| {
        if speed_ui.syncing.get() {
            return;
        }
        // An unselected row reports INVALID_LIST_POSITION, which is not an
        // index — reading it as one would land on "full speed" and lift a
        // limit nobody asked to lift.
        let Ok(index) = usize::try_from(row.selected()) else {
            return;
        };
        if index > CHARGE_SPEED_CUSTOM {
            return;
        }
        // Choosing Custom writes nothing: the limit in effect is already
        // whatever it is, and the row only reveals the slider that can change
        // it. Unlike the fingerprint's custom level, there is no EC state to
        // enter here — a dialled-in current is just a current.
        if index == CHARGE_SPEED_CUSTOM {
            speed_ui.speed_custom_row.set_visible(true);
            return;
        }
        // The row stays insensitive until the capacity is read, so a preset
        // can't be picked without one; the early return only keeps the
        // conversion total.
        let Some(design_capacity) = speed_ui.design_capacity.get() else {
            return;
        };
        let milliamps = charge_speed_milliamps(design_capacity, index);
        let ui = speed_ui.clone();
        let proxy = speed_proxy.clone();
        glib::spawn_future_local(async move {
            apply_charge_speed(&ui, &proxy, milliamps, Custom::Rederive).await;
        });
    });

    // Slider: a raw current, reachable only while the combo is on Custom.
    let scale_ui = ui.clone();
    let scale_proxy = proxy.clone();
    connect_slider_writes(ui, &ui.speed_scale, scale_milliamps, move |milliamps| {
        let ui = scale_ui.clone();
        let proxy = scale_proxy.clone();
        glib::spawn_future_local(async move {
            apply_charge_speed(&ui, &proxy, milliamps, Custom::Keep).await;
        });
    });

    // The cap itself cannot be read back from the EC, so what the charger is
    // actually doing is the only confirmation the app can offer that it took
    // effect.
    let poll_ui = ui.clone();
    let poll_proxy = proxy.clone();
    poll_while_mapped(&ui.speed_combo, CHARGE_CURRENT_SECONDS, move || {
        let ui = poll_ui.clone();
        let proxy = poll_proxy.clone();
        glib::spawn_future_local(async move {
            let subtitle = match proxy.get_charge_current().await {
                Ok(0) => "Not charging".to_string(),
                Ok(milliamps) => format!("Charging at {}", amps(milliamps)),
                Err(_) => return,
            };
            ui.speed_combo.set_subtitle(&subtitle);
        });
    });
}

fn connect_touchpad_setters(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let haptic_ui = ui.clone();
    let haptic_proxy = proxy.clone();
    ui.haptic_combo.connect_selected_notify(move |row| {
        if haptic_ui.syncing.get() {
            return;
        }
        let percent = HAPTIC_LEVELS[row.selected() as usize];
        let ui = haptic_ui.clone();
        let proxy = haptic_proxy.clone();
        glib::spawn_future_local(async move {
            if let Err(e) = proxy.set_haptic_intensity(percent).await {
                ui.toast(&format!("Setting haptic intensity failed: {e}"));
            }
        });
    });

    let force_ui = ui.clone();
    let force_proxy = proxy.clone();
    ui.force_combo.connect_selected_notify(move |row| {
        if force_ui.syncing.get() {
            return;
        }
        let force = CLICK_FORCES[row.selected() as usize];
        let ui = force_ui.clone();
        let proxy = force_proxy.clone();
        glib::spawn_future_local(async move {
            if let Err(e) = proxy.set_touchpad_click_force(force).await {
                ui.toast(&format!("Setting click force failed: {e}"));
            }
        });
    });
}

/// The one write for the charge limit. The window's row and the tray preset
/// both come here, so neither can drift from the other on what it reports or
/// what it tells the tray.
///
/// [`apply_charge_speed`] is the same shape for the other Battery control.
/// They are deliberately two functions rather than one generic: the values
/// differ (`u8` against `u32`), the speed resolves its presets against the
/// battery's capacity where the ceiling's are constants, and only the speed
/// has a "full speed means no limit" case. A change to one is usually a
/// change to both — read the sibling before editing either.
async fn apply_charge_limit(ui: &Ui, proxy: &FrameguinProxy<'static>, percent: u8, custom: Custom) {
    // Ask before writing, rather than trusting what the app last saw: the
    // EC's battery extender lowers this ceiling on its own, so a remembered
    // value can be wrong, and skipping on a wrong one would swallow the
    // request in silence. The read costs less than the write it saves, and
    // the widgets still move — Custom and the preset that names the same
    // number are different rows.
    if proxy.get_charge_limit().await == Ok(percent) {
        ui.show_charge_limit(percent, custom);
        return;
    }
    match proxy.set_charge_limit(percent).await {
        Ok(()) => {
            ui.toast(&format!("Charge limit set to {percent}%"));
            ui.sync_tray_charge_limit(percent);
            ui.show_charge_limit(percent, custom);
        }
        Err(e) => ui.toast(&format!("Setting charge limit failed: {e}")),
    }
}

/// The one write for the charge speed, in mA or `NO_CHARGE_CURRENT_LIMIT`.
/// Callers resolve a speed to milliamps against the battery capacity they
/// hold — the window's, or the tray's own copy.
async fn apply_charge_speed(
    ui: &Ui,
    proxy: &FrameguinProxy<'static>,
    milliamps: u32,
    custom: Custom,
) {
    // Read before writing, for the reason [`apply_charge_limit`] gives.
    if proxy.get_charge_current_limit().await == Ok(milliamps) {
        ui.show_charge_speed(milliamps, custom);
        return;
    }
    if let Err(e) = proxy.set_charge_current_limit(milliamps).await {
        ui.toast(&format!("Setting charge speed failed: {e}"));
        return;
    }
    if milliamps == NO_CHARGE_CURRENT_LIMIT {
        ui.toast("Charging at full speed");
    } else {
        ui.toast(&format!("Charge speed capped at {}", amps(milliamps)));
    }
    ui.sync_tray_charge_speed(milliamps);
    ui.show_charge_speed(milliamps, custom);
}

/// The one write for a fingerprint preset. "custom" is not one: the EC
/// reports it after a raw percentage write, which goes through
/// [`apply_fp_brightness`] instead.
async fn apply_fp_level(ui: &Ui, proxy: &FrameguinProxy<'static>, level: &str) {
    if let Err(e) = proxy.set_fingerprint_level(level).await {
        ui.toast(&format!("Setting fingerprint level failed: {e}"));
        return;
    }
    ui.sync_tray_fp_level(level);
    ui.sync(|| {
        ui.fp_custom_row.set_visible(false);
        ui.fp_combo.set_selected(ui.fp_combo_index(level));
    });
    // The preset resolves to a percentage only the EC knows.
    if let Ok((percent, _)) = proxy.get_fingerprint_brightness().await {
        ui.sync(|| ui.fp_scale.set_value(f64::from(percent)));
    }
}

/// The one write for a custom fingerprint percentage. Any raw percentage
/// leaves the EC reporting "custom", so this owns that consequence rather
/// than leaving each caller to remember it.
async fn apply_fp_brightness(ui: &Ui, proxy: &FrameguinProxy<'static>, percent: u8) {
    if let Err(e) = proxy.set_fingerprint_brightness(percent).await {
        ui.toast(&format!("Setting fingerprint brightness failed: {e}"));
        return;
    }
    ui.sync_tray_fp_level("custom");
    ui.sync(|| ui.fp_custom_row.set_visible(true));
}

fn connect_fp_setter(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    // Slider: a raw percentage write; only reachable while the level is
    // Custom, so combo and tray already reflect it.
    let fp_slot = Rc::new(RefCell::new(None));
    let fp_ui = ui.clone();
    let fp_proxy = proxy.clone();
    ui.fp_scale.connect_value_changed(move |scale| {
        if fp_ui.syncing.get() {
            return;
        }
        let value = scale_percent(scale.value());
        let ui = fp_ui.clone();
        let proxy = fp_proxy.clone();
        debounce(&fp_slot, SLIDER_DEBOUNCE, move || {
            glib::spawn_future_local(async move {
                apply_fp_brightness(&ui, &proxy, value).await;
            });
        });
    });

    // Combo: presets write the level and re-read so the slider carries the
    // percentage the preset resolved to; Custom reveals the slider and
    // applies its value, making the EC state actually custom.
    let combo_ui = ui.clone();
    let combo_proxy = proxy.clone();
    ui.fp_combo.connect_selected_notify(move |row| {
        if combo_ui.syncing.get() {
            return;
        }
        let level = combo_ui.fp_levels.get()[row.selected() as usize];
        let ui = combo_ui.clone();
        let proxy = combo_proxy.clone();
        glib::spawn_future_local(async move {
            if level == "custom" {
                let percent = scale_percent(ui.fp_scale.value());
                apply_fp_brightness(&ui, &proxy, percent).await;
                return;
            }
            apply_fp_level(&ui, &proxy, level).await;
        });
    });
}

fn connect_charge_setter(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let limit_ui = ui.clone();
    let limit_proxy = proxy.clone();
    ui.limit_combo.connect_selected_notify(move |row| {
        if limit_ui.syncing.get() {
            return;
        }
        let Ok(index) = usize::try_from(row.selected()) else {
            return;
        };
        if index > CHARGE_LIMIT_CUSTOM {
            return;
        }
        // Choosing Custom writes nothing — the ceiling in effect is already
        // whatever it is, and the row only reveals the slider that moves it.
        if index == CHARGE_LIMIT_CUSTOM {
            limit_ui.limit_custom_row.set_visible(true);
            return;
        }
        let percent = CHARGE_PRESETS[index];
        let ui = limit_ui.clone();
        let proxy = limit_proxy.clone();
        glib::spawn_future_local(async move {
            apply_charge_limit(&ui, &proxy, percent, Custom::Rederive).await;
        });
    });

    // Slider: a raw ceiling, reachable only while the combo is on Custom.
    let scale_ui = ui.clone();
    let scale_proxy = proxy.clone();
    connect_slider_writes(ui, &ui.limit_scale, scale_percent, move |percent| {
        let ui = scale_ui.clone();
        let proxy = scale_proxy.clone();
        glib::spawn_future_local(async move {
            apply_charge_limit(&ui, &proxy, percent, Custom::Keep).await;
        });
    });
}

fn connect_kbd_setter(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let kbd_slot = Rc::new(RefCell::new(None));
    let kbd_ui = ui.clone();
    let kbd_proxy = proxy.clone();
    let kbd_write_slot = kbd_slot.clone();
    ui.kbd_scale.connect_value_changed(move |scale| {
        if kbd_ui.syncing.get() {
            return;
        }
        let value = scale_percent(scale.value());
        let ui = kbd_ui.clone();
        let proxy = kbd_proxy.clone();
        debounce(&kbd_write_slot, SLIDER_DEBOUNCE, move || {
            glib::spawn_future_local(async move {
                if let Err(e) = proxy.set_keyboard_backlight(value).await {
                    ui.toast(&format!("Setting backlight failed: {e}"));
                }
            });
        });
    });

    // The EC is a second writer to the backlight (Fn+Space, and on newer
    // boards a firmware auto mode that overrides host writes), so while the
    // slider is on screen it follows the actual value. The tick skips while a
    // write is pending so it can't yank the slider mid-drag.
    let kbd_poll_ui = ui.clone();
    let kbd_poll_proxy = proxy.clone();
    poll_while_mapped(&ui.kbd_scale, KBD_SYNC_SECONDS, move || {
        if kbd_slot.borrow().is_some() {
            return;
        }
        let ui = kbd_poll_ui.clone();
        let proxy = kbd_poll_proxy.clone();
        glib::spawn_future_local(async move {
            if let Ok(percent) = proxy.get_keyboard_backlight().await
                && percent != scale_percent(ui.kbd_scale.value())
            {
                ui.sync(|| ui.kbd_scale.set_value(f64::from(percent)));
            }
        });
    });

}

/// Runs `tick` every `seconds` for as long as `widget` is on screen: a
/// resident app whose window is hidden does no periodic work, and neither
/// does one whose board lacks the control, since an unsupported row is never
/// mapped.
fn poll_while_mapped(widget: &impl IsA<gtk::Widget>, seconds: u32, tick: impl Fn() + 'static) {
    let tick = Rc::new(tick);
    let source: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    let arm: Rc<dyn Fn()> = {
        let source = source.clone();
        Rc::new(move || {
            let tick = tick.clone();
            let id = glib::timeout_add_seconds_local(seconds, move || {
                tick();
                glib::ControlFlow::Continue
            });
            if let Some(old) = source.replace(Some(id)) {
                old.remove();
            }
        })
    };
    let map_arm = arm.clone();
    widget.as_ref().connect_map(move |_| map_arm());
    let unmap_source = source;
    widget.as_ref().connect_unmap(move |_| {
        if let Some(id) = unmap_source.take() {
            id.remove();
        }
    });
    // The window is usually already on screen when setters connect (init is
    // async), so map won't fire for the current visibility.
    if widget.as_ref().is_mapped() {
        arm();
    }
}

#[expect(clippy::too_many_lines, reason = "flat widget construction")]
fn build_window(
    app: &adw::Application,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
) -> (adw::ApplicationWindow, Rc<Ui>) {
    let page = adw::PreferencesPage::new();

    let battery = adw::PreferencesGroup::builder().title("Battery").build();
    let limit_labels = with_custom_row(charge_limit_labels());
    let limit_combo = adw::ComboRow::builder()
        .title("Charge limit")
        .subtitle("Maximum charge percentage")
        .model(&string_list(&limit_labels))
        .sensitive(false)
        .build();
    battery.add(&limit_combo);
    let limit_custom_row = adw::ActionRow::builder().title("Maximum charge").build();
    let limit_adjustment =
        gtk::Adjustment::new(MIN_CHARGE_LIMIT, MIN_CHARGE_LIMIT, 100.0, 5.0, 5.0, 0.0);
    let limit_scale = build_scale(&limit_adjustment, |value| format!("{value:.0}%"));
    limit_custom_row.add_suffix(&limit_scale);
    limit_custom_row.set_visible(false);
    battery.add(&limit_custom_row);
    let speed_combo = adw::ComboRow::builder()
        .title("Charge speed")
        .subtitle("Maximum charging rate")
        .model(&gtk::StringList::new(&CHARGE_SPEED_LABELS))
        .sensitive(false)
        .build();
    battery.add(&speed_combo);
    let speed_custom_row = adw::ActionRow::builder().title("Charge current").build();
    // The upper bound is the battery's 1C current, filled in once it is read;
    // asking for more than the pack requests would be a limit that never
    // binds. Explicit adjustment for the same reason as the backlight's.
    let speed_adjustment = gtk::Adjustment::new(
        MIN_CUSTOM_CHARGE_MA,
        MIN_CUSTOM_CHARGE_MA,
        MIN_CUSTOM_CHARGE_MA,
        100.0,
        100.0,
        0.0,
    );
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "slider is bounded by its adjustment, which holds milliamps"
    )]
    let speed_scale = build_scale(&speed_adjustment, |value| amps(value as u32));
    speed_custom_row.add_suffix(&speed_scale);
    speed_custom_row.set_visible(false);
    battery.add(&speed_custom_row);
    page.add(&battery);

    let keyboard = adw::PreferencesGroup::builder().title("Keyboard").build();
    let kbd_row = adw::ActionRow::builder().title("Backlight").build();
    // Explicit adjustment: with_range would set page_increment to 10x the
    // step, and a mouse wheel click on a GtkRange moves by the page
    // increment — which would jump the slider across its whole range.
    let kbd_adjustment = gtk::Adjustment::new(0.0, 0.0, 100.0, 10.0, 10.0, 0.0);
    let kbd_scale = build_scale(&kbd_adjustment, |value| format!("{value:.0}%"));
    kbd_row.add_suffix(&kbd_scale);
    keyboard.add(&kbd_row);
    page.add(&keyboard);

    let fingerprint = adw::PreferencesGroup::builder().title("Fingerprint").build();
    let fp_combo = adw::ComboRow::builder()
        .title("LED level")
        .model(&gtk::StringList::new(&FP_LEVEL_LABELS))
        .sensitive(false)
        .build();
    fingerprint.add(&fp_combo);
    let fp_row = adw::ActionRow::builder().title("LED brightness").build();
    // The EC accepts 1-100 for the fingerprint LED; 0 is not a valid level.
    let fp_adjustment = gtk::Adjustment::new(1.0, 1.0, 100.0, 10.0, 10.0, 0.0);
    let fp_scale = build_scale(&fp_adjustment, |value| format!("{value:.0}%"));
    fp_row.add_suffix(&fp_scale);
    fp_row.set_visible(false);
    fingerprint.add(&fp_row);
    page.add(&fingerprint);

    let touchpad = adw::PreferencesGroup::builder().title("Touchpad").build();
    let haptic_combo = adw::ComboRow::builder()
        .title("Haptic intensity")
        .subtitle("Strength of the click feedback")
        .model(&gtk::StringList::new(&HAPTIC_LABELS))
        .sensitive(false)
        .build();
    touchpad.add(&haptic_combo);
    let force_combo = adw::ComboRow::builder()
        .title("Click force")
        .subtitle("How hard you press to click")
        .model(&gtk::StringList::new(&CLICK_FORCE_LABELS))
        .sensitive(false)
        .build();
    touchpad.add(&force_combo);
    page.add(&touchpad);

    let application = adw::PreferencesGroup::builder().title("Application").build();
    let autostart_row = adw::SwitchRow::builder()
        .title("Start at login")
        .subtitle("Show only the tray icon until opened")
        .build();
    autostart_row.set_active(autostart_entry_path().exists());
    application.add(&autostart_row);
    page.add(&application);

    // Detected hardware as the header subtitle: one line, no key/value rows.
    let detected = if dmi("sys_vendor") == "Framework" {
        dmi("product_name")
    } else {
        "No Framework hardware detected".to_string()
    };

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Frameguin", &detected)));

    let menu = gio::Menu::new();
    menu.append(Some("_About Frameguin"), Some("app.about"));
    menu.append(Some("_Quit"), Some("app.quit"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Main menu")
        .build();
    header.pack_end(&menu_button);
    view.add_top_bar(&header);
    view.set_content(Some(&page));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&view));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Frameguin")
        .default_width(420)
        // Tall enough for every control group at the default font scale;
        // re-measure when adding a group.
        .default_height(760)
        .content(&toasts)
        .icon_name(APP_ID)
        .build();

    // Hiding instead of closing only makes sense while a tray icon exists to
    // bring the window back.
    window.set_hide_on_close(tray.is_some());

    let ui = Rc::new(Ui {
        toasts,
        limit_combo,
        limit_scale,
        limit_custom_row,
        speed_combo,
        speed_scale,
        speed_custom_row,
        design_capacity: Cell::default(),
        caps: Cell::default(),
        syncing: Cell::new(false),
        kbd_scale,
        fp_scale,
        fp_combo,
        fp_custom_row: fp_row,
        fp_levels: Cell::new(fp_levels_labels(true).0),
        haptic_combo,
        force_combo,
        tray,
    });

    let autostart_ui = ui.clone();
    autostart_row.connect_active_notify(move |row| {
        if let Err(e) = set_autostart(row.is_active()) {
            autostart_ui.toast(&format!("Updating autostart failed: {e}"));
        }
    });

    let groups = CapabilityWidgets {
        battery: battery.clone(),
        limit_combo: ui.limit_combo.clone(),
        speed_combo: ui.speed_combo.clone(),
        keyboard: keyboard.clone(),
        fingerprint: fingerprint.clone(),
        touchpad: touchpad.clone(),
    };
    glib::spawn_future_local(init_from_daemon(ui.clone(), groups, window.clone()));

    (window, ui)
}

/// Everything gated on a capability, so hiding what a board lacks is one call
/// rather than one line per control. A group whose controls are probed
/// separately carries its rows here too, keeping visibility a single question
/// with a single answer.
struct CapabilityWidgets {
    battery: adw::PreferencesGroup,
    limit_combo: adw::ComboRow,
    speed_combo: adw::ComboRow,
    keyboard: adw::PreferencesGroup,
    fingerprint: adw::PreferencesGroup,
    touchpad: adw::PreferencesGroup,
}

impl CapabilityWidgets {
    fn show_supported(&self, caps: Capabilities) {
        self.battery
            .set_visible(caps.charge_limit || caps.charge_current_limit);
        self.limit_combo.set_visible(caps.charge_limit);
        self.speed_combo.set_visible(caps.charge_current_limit);
        self.keyboard.set_visible(caps.keyboard_backlight);
        self.fingerprint.set_visible(caps.fp_brightness);
        self.touchpad.set_visible(caps.haptic_touchpad);
    }
}

/// Asks the daemon what this board supports, hides what it can't do, loads
/// current values, then connects the setters — last, so the programmatic
/// `set_value` calls during init can't echo back into the daemon.
async fn init_from_daemon(ui: Rc<Ui>, groups: CapabilityWidgets, window: adw::ApplicationWindow) {
    let proxy = match daemon_proxy().await {
        Ok(p) => p,
        Err(e) => {
            ui.toast(&format!("Daemon unavailable: {e}"));
            return;
        }
    };

    let names = match proxy.get_capabilities().await {
        Ok(names) => names,
        Err(e) => {
            ui.toast(&format!("Reading capabilities failed: {e}"));
            Vec::new()
        }
    };
    let caps = Capabilities::from_names(&names);
    ui.caps.set(caps);
    groups.show_supported(caps);
    ui.sync_tray_caps(caps);
    // Fixed for a firmware generation, so the combo's rows are chosen once
    // rather than rebuilt by every reload.
    if caps.fp_brightness {
        let (levels, labels) = fp_levels_labels(caps.fp_custom);
        ui.fp_levels.set(levels);
        if !caps.fp_custom {
            ui.fp_combo.set_model(Some(&gtk::StringList::new(labels)));
        }
    }
    load_values(&ui, &proxy).await;
    connect_setters(&ui, &proxy);

    // The hardware moves while the app sits in the tray: the EC's battery
    // extender lowers the charge limit on its own, and framework_tool writes
    // any of these behind the app's back. So the window reloads every time it
    // returns to the screen instead of trusting what it read at startup.
    let map_ui = ui.clone();
    let map_proxy = proxy.clone();
    window.connect_map(move |_| {
        let ui = map_ui.clone();
        let proxy = map_proxy.clone();
        glib::spawn_future_local(async move { load_values(&ui, &proxy).await });
    });
}

/// The Battery group's half of a reload: the ceiling and the speed, each with
/// its combo and slider. Returns what the tray should be told, for the one
/// push [`load_values`] makes at the end.
async fn load_battery_values(
    ui: &Rc<Ui>,
    proxy: &FrameguinProxy<'static>,
) -> (Option<u8>, Option<u32>) {
    let caps = ui.caps.get();
    let mut tray_limit = None;
    let mut tray_speed = None;
    if caps.charge_limit {
        match proxy.get_charge_limit().await {
            Ok(limit) => {
                ui.show_charge_limit(limit, Custom::Rederive);
                ui.sync(|| {
                    ui.limit_combo.set_sensitive(true);
                    ui.limit_scale.set_sensitive(true);
                });
                tray_limit = Some(limit);
            }
            Err(e) => ui.toast(&format!("Reading charge limit failed: {e}")),
        }
    }
    if caps.charge_current_limit {
        // A pack's design capacity can't change under a running app, so it is
        // read once and the labels built from it stay put.
        if ui.design_capacity.get().is_none() {
            match proxy.get_battery_design_capacity().await {
                Ok(capacity) => {
                    ui.design_capacity.set(Some(capacity));
                    let labels = with_custom_row(charge_speed_labels(capacity));
                    // 1C is as fast as the pack ever asks, so a slider beyond
                    // it would only offer limits that never bind. Floored to
                    // the step the value rounds to, so the far end of the
                    // track is a position that sends what it shows rather
                    // than a sliver that rounds back down.
                    let top = (f64::from(capacity) / CUSTOM_CHARGE_STEP_MA).floor()
                        * CUSTOM_CHARGE_STEP_MA;
                    ui.sync(|| {
                        ui.speed_combo.set_model(Some(&string_list(&labels)));
                        ui.speed_scale
                            .adjustment()
                            .set_upper(top.max(MIN_CUSTOM_CHARGE_MA));
                    });
                }
                Err(e) => ui.toast(&format!("Reading battery capacity failed: {e}")),
            }
        }
        match proxy.get_charge_current_limit().await {
            Ok(milliamps) => {
                ui.show_charge_speed(milliamps, Custom::Rederive);
                // Without the battery's capacity the fractions have no
                // milliamps behind them, so the row stays read-only.
                let known = ui.design_capacity.get().is_some();
                ui.sync(|| {
                    ui.speed_combo.set_sensitive(known);
                    ui.speed_scale.set_sensitive(known);
                });
                tray_speed = Some(milliamps);
            }
            Err(e) => ui.toast(&format!("Reading charge speed failed: {e}")),
        }
    }
    (tray_limit, tray_speed)
}

/// Re-reads every supported control and moves the widgets to match, pushing
/// the same values to the tray. Each write goes through `Ui::sync`, so a
/// reload can't echo back as a setter call. The tray's copies are collected
/// and handed over in one go at the end: each push blocks on the tray's
/// thread and rebuilds its whole menu, which would be wasted three times over
/// on a menu nobody has opened.
async fn load_values(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let caps = ui.caps.get();
    let mut tray_level = None;
    let (tray_limit, tray_speed) = load_battery_values(ui, proxy).await;
    if caps.keyboard_backlight {
        match proxy.get_keyboard_backlight().await {
            Ok(percent) => ui.sync(|| {
                ui.kbd_scale.set_value(f64::from(percent));
                ui.kbd_scale.set_sensitive(true);
            }),
            Err(e) => ui.toast(&format!("Reading keyboard backlight failed: {e}")),
        }
    }
    if caps.fp_brightness {
        match proxy.get_fingerprint_brightness().await {
            Ok((percent, level)) => {
                ui.sync(|| {
                    ui.fp_scale.set_value(f64::from(percent));
                    ui.fp_combo.set_selected(ui.fp_combo_index(&level));
                    ui.fp_custom_row.set_visible(level == "custom");
                    ui.fp_scale.set_sensitive(true);
                    ui.fp_combo.set_sensitive(true);
                });
                tray_level = Some(level);
            }
            Err(e) => ui.toast(&format!("Reading fingerprint brightness failed: {e}")),
        }
    }
    if caps.haptic_touchpad {
        match proxy.get_haptic_intensity().await {
            Ok(percent) => {
                let index = HAPTIC_LEVELS.iter().position(|l| *l == percent).unwrap_or(3);
                ui.sync(|| {
                    ui.haptic_combo.set_selected(combo_index(index));
                    ui.haptic_combo.set_sensitive(true);
                });
            }
            Err(e) => ui.toast(&format!("Reading haptic intensity failed: {e}")),
        }
        match proxy.get_touchpad_click_force().await {
            Ok(force) => {
                let index = CLICK_FORCES.iter().position(|f| *f == force).unwrap_or(1);
                ui.sync(|| {
                    ui.force_combo.set_selected(combo_index(index));
                    ui.force_combo.set_sensitive(true);
                });
            }
            Err(e) => ui.toast(&format!("Reading click force failed: {e}")),
        }
    }
    if let Some(handle) = &ui.tray {
        let capacity = ui.design_capacity.get();
        handle.update(move |tray| {
            if let Some(limit) = tray_limit {
                tray.charge_limit = Some(limit);
            }
            if capacity.is_some() {
                tray.design_capacity = capacity;
            }
            if let Some(speed) = tray_speed {
                tray.charge_current_limit = Some(speed);
            }
            if let Some(level) = tray_level {
                tray.fp_level = Some(level);
            }
        });
    }
}

/// Window and tray state shared between activation and tray events. The
/// window is built on first use, so a service-mode start (autostart) costs
/// only the tray icon.
#[derive(Default)]
struct AppState {
    window: RefCell<Option<(adw::ApplicationWindow, Rc<Ui>)>>,
    tray: RefCell<Option<ksni::blocking::Handle<TrayIcon>>>,
}

impl AppState {
    fn window_for(
        &self,
        app: &adw::Application,
    ) -> (adw::ApplicationWindow, Rc<Ui>) {
        let mut slot = self.window.borrow_mut();
        slot.get_or_insert_with(|| build_window(app, self.tray.borrow().clone()))
            .clone()
    }
}

/// Reads the values the tray menu renders from. The tray keeps its own copies
/// because it has to draw with no window open, so they are pulled from the
/// daemon rather than from the window's widgets.
async fn refresh_tray(handle: &ksni::blocking::Handle<TrayIcon>, proxy: &FrameguinProxy<'static>) {
    // One read and one write. Every `update` blocks this thread on the tray's
    // own, and makes it rebuild the entire menu and signal it over D-Bus, so
    // a field-at-a-time refresh would do that four times for one menu.
    let Some((known_caps, known_capacity)) = handle.update(|tray| (tray.caps, tray.design_capacity))
    else {
        return;
    };
    // Capabilities and the battery's capacity are both fixed for the daemon's
    // run, so the menu keeps them and only the values that can change are
    // asked for again.
    let caps = if let Some(caps) = known_caps {
        caps
    } else {
        let Ok(names) = proxy.get_capabilities().await else { return };
        Capabilities::from_names(&names)
    };
    let limit = if caps.charge_limit {
        proxy.get_charge_limit().await.ok()
    } else {
        None
    };
    let mut capacity = known_capacity;
    let mut speed = None;
    if caps.charge_current_limit {
        if capacity.is_none() {
            capacity = proxy.get_battery_design_capacity().await.ok();
        }
        if capacity.is_some() {
            speed = proxy.get_charge_current_limit().await.ok();
        }
    }
    let level = if caps.fp_brightness {
        proxy
            .get_fingerprint_brightness()
            .await
            .ok()
            .map(|(_, level)| level)
    } else {
        None
    };
    handle.update(move |tray| {
        tray.caps = Some(caps);
        if let Some(limit) = limit {
            tray.charge_limit = Some(limit);
        }
        if capacity.is_some() {
            tray.design_capacity = capacity;
        }
        if let Some(speed) = speed {
            tray.charge_current_limit = Some(speed);
        }
        if let Some(level) = level {
            tray.fp_level = Some(level);
        }
    });
}

fn setup_tray(app: &adw::Application, state: Rc<AppState>) {
    use ksni::blocking::TrayMethods;

    let (tx, rx) = async_channel::unbounded();
    let tray = TrayIcon {
        tx,
        charge_limit: None,
        charge_current_limit: None,
        design_capacity: None,
        fp_level: None,
        caps: None,
    };
    let handle = match tray.spawn() {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("tray icon unavailable: {e}");
            return;
        }
    };
    *state.tray.borrow_mut() = Some(handle.clone());

    let hold = app.hold();
    let app = app.clone();
    glib::spawn_future_local(async move {
        let _hold = hold;
        // One connection for the tray's whole life: opening the menu refreshes
        // it, and dialling the bus each time would put a connect and an
        // authentication handshake in front of a menu that has to feel instant.
        let proxy = daemon_proxy().await.ok();
        // Populate the menu right away: in tray-only mode (autostart) nothing
        // else fetches capabilities until the window is first opened, which
        // would leave the menu at Open/Quit.
        if let Some(proxy) = &proxy {
            refresh_tray(&handle, proxy).await;
        }
        while let Ok(event) = rx.recv().await {
            match event {
                TrayEvent::Show => {
                    let window = state.window_for(&app).0;
                    window.unminimize();
                    // A tray event has no activation token, so raising an
                    // already-mapped window is denied by Wayland focus-
                    // stealing prevention. Remapping sidesteps that: a
                    // freshly mapped window is granted focus by the
                    // compositor's new-window policy. Costs the remembered
                    // window position, which Wayland doesn't keep anyway.
                    if window.is_visible() && !window.is_active() {
                        window.set_visible(false);
                    }
                    window.present();
                }
                // Tray presets call the shared write rather than writing by
                // moving the widget: a widget already showing the requested
                // value emits no change, and the click would be swallowed.
                TrayEvent::SetChargeLimit(percent) => {
                    if let Some(proxy) = &proxy {
                        let ui = state.window_for(&app).1;
                        apply_charge_limit(&ui, proxy, percent, Custom::Rederive).await;
                    }
                }
                TrayEvent::Refresh => {
                    if let Some(proxy) = &proxy {
                        refresh_tray(&handle, proxy).await;
                    }
                }
                TrayEvent::SetChargeSpeed(milliamps) => {
                    if let Some(proxy) = &proxy {
                        let ui = state.window_for(&app).1;
                        apply_charge_speed(&ui, proxy, milliamps, Custom::Rederive).await;
                    }
                }
                TrayEvent::SetFingerprintLevel(level) => {
                    if let Some(proxy) = &proxy {
                        apply_fp_level(&state.window_for(&app).1, proxy, level).await;
                    }
                }
                TrayEvent::Quit => app.quit(),
            }
        }
    });
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();

    // The line is parsed, not just read: it must end in the bare version.
    app.add_main_option(
        "version",
        b'V'.into(),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Show the version",
        None,
    );
    // The About window's report, for the no-display and window-won't-open
    // cases that produce bug reports in the first place.
    app.add_main_option(
        "debug-info",
        glib::Char::from(0),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Print a hardware report",
        None,
    );
    app.connect_handle_local_options(|_, options| {
        let report = if options.contains("version") {
            format!("frameguin {}\n", env!("CARGO_PKG_VERSION"))
        } else if options.contains("debug-info") {
            glib::MainContext::default().block_on(debug_info())
        } else {
            return ControlFlow::Continue(());
        };
        print!("{report}");
        ControlFlow::Break(glib::ExitCode::SUCCESS)
    });

    app.add_action_entries([
        gio::ActionEntry::builder("about")
            .activate(|app: &adw::Application, _, _| show_about(app.active_window().as_ref()))
            .build(),
        gio::ActionEntry::builder("quit")
            .activate(|app: &adw::Application, _, _| app.quit())
            .build(),
    ]);

    // Autostart launches with GIO's built-in --gapplication-service, which
    // registers the primary instance without emitting activate — so login
    // brings up only the tray; the first Show or plain launch builds the
    // window.
    let state = Rc::new(AppState::default());
    let startup_state = state.clone();
    app.connect_startup(move |app| setup_tray(app, startup_state.clone()));
    app.connect_activate(move |app| state.window_for(app).0.present());
    app.run()
}
