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
const CHARGE_DEBOUNCE: Duration = Duration::from_millis(700);
const SLIDER_DEBOUNCE: Duration = Duration::from_millis(200);
const KBD_SYNC_SECONDS: u32 = 2;

#[zbus::proxy(
    interface = "io.github.valeronm.Frameguin1",
    default_service = "io.github.valeronm.Frameguin",
    default_path = "/io/github/valeronm/Frameguin"
)]
trait Frameguin {
    async fn get_charge_limit(&self) -> zbus::Result<i32>;
    async fn set_charge_limit(&self, percent: i32) -> zbus::Result<()>;
    async fn get_keyboard_backlight(&self) -> zbus::Result<i32>;
    async fn set_keyboard_backlight(&self, percent: i32) -> zbus::Result<()>;
    async fn get_capabilities(&self) -> zbus::Result<Vec<String>>;
    async fn get_ec_version(&self) -> zbus::Result<String>;
    async fn get_build(&self) -> zbus::Result<(String, String)>;
    async fn get_fingerprint_brightness(&self) -> zbus::Result<(i32, String)>;
    async fn set_fingerprint_brightness(&self, percent: i32) -> zbus::Result<()>;
    async fn set_fingerprint_level(&self, level: &str) -> zbus::Result<()>;
    async fn get_haptic_intensity(&self) -> zbus::Result<i32>;
    async fn set_haptic_intensity(&self, percent: i32) -> zbus::Result<()>;
    async fn get_touchpad_click_force(&self) -> zbus::Result<String>;
    async fn set_touchpad_click_force(&self, force: &str) -> zbus::Result<()>;
}

/// The steps the Boreas haptic firmware implements, and the click-force
/// names the daemon accepts.
const HAPTIC_LEVELS: [i32; 5] = [0, 25, 50, 75, 100];
const HAPTIC_LABELS: [&str; 5] = ["Off", "25%", "50%", "75%", "100%"];
const CLICK_FORCES: [&str; 3] = ["low", "medium", "high"];
const CLICK_FORCE_LABELS: [&str; 3] = ["Low", "Medium", "High"];

/// What the connected board supports, per the daemon's probe. The default
/// (all false) doubles as "not yet known".
#[derive(Clone, Copy, Default)]
struct Capabilities {
    charge_limit: bool,
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
    SetChargeLimit(i32),
    SetFingerprintLevel(&'static str),
    Quit,
}

const CHARGE_PRESETS: [i32; 3] = [60, 80, 100];

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
    charge_limit: Option<i32>,
    /// Current fingerprint LED level name, pushed in from the app; "custom"
    /// marks no radio option.
    fp_level: Option<String>,
    /// Pushed in once the app reads the daemon's probe; the all-false
    /// default means the menu offers no controls, only Open/Quit.
    caps: Capabilities,
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

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{RadioGroup, RadioItem, StandardItem, SubMenu};
        let mut items: Vec<ksni::MenuItem<Self>> = vec![
            StandardItem {
                label: "Open".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::Show)),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
        ];
        let caps = self.caps;
        if caps.charge_limit {
            items.push(SubMenu {
                label: match self.charge_limit {
                    // A 100% ceiling is no limit at all — say so.
                    Some(100) => "Charge limit (off)".into(),
                    Some(limit) => format!("Charge limit ({limit}%)"),
                    None => "Charge limit".into(),
                },
                submenu: vec![RadioGroup {
                    selected: CHARGE_PRESETS
                        .iter()
                        .position(|p| Some(*p) == self.charge_limit)
                        .unwrap_or(usize::MAX),
                    select: Box::new(|tray: &mut Self, index| {
                        tray.send(TrayEvent::SetChargeLimit(CHARGE_PRESETS[index]))
                    }),
                    options: CHARGE_PRESETS
                        .iter()
                        .map(|percent| RadioItem {
                            label: if *percent == 100 {
                                "No limit".into()
                            } else {
                                format!("{percent}%")
                            },
                            ..Default::default()
                        })
                        .collect(),
                }
                .into()],
                ..Default::default()
            }
            .into());
        }
        if caps.fp_brightness {
            // Presets only — "custom" is a state the EC reports, not an
            // action, so the tray offers just the settable levels of this
            // firmware generation.
            let (mut levels, mut labels) = fp_levels_labels(caps.fp_custom);
            if let Some(stripped) = levels.strip_suffix(&["custom"]) {
                levels = stripped;
                labels = &labels[..levels.len()];
            }
            let selected = self
                .fp_level
                .as_deref()
                .and_then(|level| levels.iter().position(|l| *l == level));
            items.push(SubMenu {
                label: match selected {
                    Some(index) => {
                        format!("Fingerprint LED ({})", labels[index])
                    }
                    None => "Fingerprint LED".into(),
                },
                submenu: vec![RadioGroup {
                    selected: selected.unwrap_or(usize::MAX),
                    select: Box::new(move |tray: &mut Self, index| {
                        tray.send(TrayEvent::SetFingerprintLevel(levels[index]))
                    }),
                    options: labels
                        .iter()
                        .map(|label| RadioItem {
                            label: (*label).into(),
                            ..Default::default()
                        })
                        .collect(),
                }
                .into()],
                ..Default::default()
            }
            .into());
        }
        if caps.charge_limit || caps.fp_brightness {
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

// --- about ---

fn dmi(file: &str) -> String {
    fs::read_to_string(format!("/sys/class/dmi/id/{file}"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

/// What a hardware report needs, behind the About window's copy button, so
/// filing one does not require busctl. Both binaries report where they ran
/// from: a mixed install has the app under one prefix and the daemon under
/// another, and no version comparison would show it when the two trees hold
/// the same release.
async fn debug_info() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let mut out = format!("Frameguin {} ({exe})\n", env!("CARGO_PKG_VERSION"));

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
/// install prefixes; TryExec lets the session skip it once Frameguin is gone,
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
    charge_row: adw::SpinRow,
    kbd_scale: gtk::Scale,
    /// Set while the slider is being moved to mirror the EC value, so the
    /// change handler doesn't echo it back as a write.
    kbd_syncing: Cell<bool>,
    fp_scale: gtk::Scale,
    fp_combo: adw::ComboRow,
    /// The slider's row; shown only while the level is Custom.
    fp_custom_row: adw::ActionRow,
    /// Same role as kbd_syncing, for programmatic fingerprint widget updates.
    fp_syncing: Cell<bool>,
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

    fn sync_tray_charge_limit(&self, percent: i32) {
        if let Some(handle) = &self.tray {
            tray_set_charge_limit(handle, percent);
        }
    }

    fn sync_tray_fp_level(&self, level: &str) {
        if let Some(handle) = &self.tray {
            tray_set_fp_level(handle, level);
        }
    }

    fn fp_combo_index(&self, level: &str) -> u32 {
        self.fp_levels
            .get()
            .iter()
            .position(|l| *l == level)
            .map(|i| i as u32)
            .unwrap_or(gtk::INVALID_LIST_POSITION)
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
    handle.update(move |tray| tray.caps = caps);
}

fn tray_set_charge_limit(handle: &ksni::blocking::Handle<TrayIcon>, percent: i32) {
    handle.update(move |tray| tray.charge_limit = Some(percent));
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
    connect_kbd_setter(ui, proxy);
    connect_fp_setter(ui, proxy);
    connect_touchpad_setters(ui, proxy);
}

fn connect_touchpad_setters(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let haptic_ui = ui.clone();
    let haptic_proxy = proxy.clone();
    ui.haptic_combo.connect_selected_notify(move |row| {
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

/// The one write path for a custom fingerprint percentage; returns whether
/// the write landed.
async fn apply_fp_brightness(ui: &Ui, proxy: &FrameguinProxy<'static>, percent: i32) -> bool {
    match proxy.set_fingerprint_brightness(percent).await {
        Ok(()) => true,
        Err(e) => {
            ui.toast(&format!("Setting fingerprint brightness failed: {e}"));
            false
        }
    }
}

fn connect_fp_setter(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    // Slider: a raw percentage write; only reachable while the level is
    // Custom, so combo and tray already reflect it.
    let fp_slot = Rc::new(RefCell::new(None));
    let fp_ui = ui.clone();
    let fp_proxy = proxy.clone();
    ui.fp_scale.connect_value_changed(move |scale| {
        if fp_ui.fp_syncing.get() {
            return;
        }
        let value = scale.value() as i32;
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
        if combo_ui.fp_syncing.get() {
            return;
        }
        let level = combo_ui.fp_levels.get()[row.selected() as usize];
        let ui = combo_ui.clone();
        let proxy = combo_proxy.clone();
        glib::spawn_future_local(async move {
            if level == "custom" {
                let percent = ui.fp_scale.value() as i32;
                if apply_fp_brightness(&ui, &proxy, percent).await {
                    ui.fp_custom_row.set_visible(true);
                    ui.sync_tray_fp_level("custom");
                }
                return;
            }
            if let Err(e) = proxy.set_fingerprint_level(level).await {
                ui.toast(&format!("Setting fingerprint level failed: {e}"));
                return;
            }
            ui.fp_custom_row.set_visible(false);
            ui.sync_tray_fp_level(level);
            if let Ok((percent, _)) = proxy.get_fingerprint_brightness().await {
                ui.fp_syncing.set(true);
                ui.fp_scale.set_value(percent as f64);
                ui.fp_syncing.set(false);
            }
        });
    });
}

fn connect_charge_setter(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let charge_slot = Rc::new(RefCell::new(None));
    let charge_ui = ui.clone();
    let charge_proxy = proxy.clone();
    ui.charge_row.connect_value_notify(move |row| {
        let value = row.value() as i32;
        let ui = charge_ui.clone();
        let proxy = charge_proxy.clone();
        debounce(&charge_slot, CHARGE_DEBOUNCE, move || {
            glib::spawn_future_local(async move {
                match proxy.set_charge_limit(value).await {
                    Ok(()) => {
                        ui.toast(&format!("Charge limit set to {value}%"));
                        ui.sync_tray_charge_limit(value);
                    }
                    Err(e) => ui.toast(&format!("Setting charge limit failed: {e}")),
                }
            });
        });
    });
}

fn connect_kbd_setter(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let kbd_slot = Rc::new(RefCell::new(None));
    let kbd_ui = ui.clone();
    let kbd_proxy = proxy.clone();
    let kbd_write_slot = kbd_slot.clone();
    ui.kbd_scale.connect_value_changed(move |scale| {
        if kbd_ui.kbd_syncing.get() {
            return;
        }
        let value = scale.value() as i32;
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
    // slider is on screen it follows the actual value. The timer exists only
    // between map and unmap — a hidden resident app runs no periodic work —
    // and skips while a write is pending so it can't yank the slider
    // mid-drag.
    let poll_source: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    let arm: Rc<dyn Fn()> = {
        let ui = ui.clone();
        let proxy = proxy.clone();
        let source = poll_source.clone();
        Rc::new(move || {
            let poll_ui = ui.clone();
            let poll_proxy = proxy.clone();
            let kbd_slot = kbd_slot.clone();
            let id = glib::timeout_add_seconds_local(KBD_SYNC_SECONDS, move || {
                if kbd_slot.borrow().is_some() {
                    return glib::ControlFlow::Continue;
                }
                let ui = poll_ui.clone();
                let proxy = poll_proxy.clone();
                glib::spawn_future_local(async move {
                    if let Ok(percent) = proxy.get_keyboard_backlight().await
                        && percent != ui.kbd_scale.value() as i32
                    {
                        ui.kbd_syncing.set(true);
                        ui.kbd_scale.set_value(percent as f64);
                        ui.kbd_syncing.set(false);
                    }
                });
                glib::ControlFlow::Continue
            });
            if let Some(old) = source.replace(Some(id)) {
                old.remove();
            }
        })
    };
    let map_arm = arm.clone();
    ui.kbd_scale.connect_map(move |_| map_arm());
    let unmap_source = poll_source;
    ui.kbd_scale.connect_unmap(move |_| {
        if let Some(id) = unmap_source.take() {
            id.remove();
        }
    });
    // The window is usually already on screen when setters connect (init is
    // async), so map won't fire for the current visibility.
    if ui.kbd_scale.is_mapped() {
        arm();
    }
}

fn build_window(
    app: &adw::Application,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
) -> (adw::ApplicationWindow, Rc<Ui>) {
    let page = adw::PreferencesPage::new();

    let battery = adw::PreferencesGroup::builder().title("Battery").build();
    let charge_row = adw::SpinRow::with_range(20.0, 100.0, 5.0);
    charge_row.set_title("Charge limit");
    charge_row.set_subtitle("Maximum charge percentage");
    charge_row.set_sensitive(false);
    battery.add(&charge_row);
    page.add(&battery);

    let keyboard = adw::PreferencesGroup::builder().title("Keyboard").build();
    let kbd_row = adw::ActionRow::builder().title("Backlight").build();
    // Explicit adjustment: with_range would set page_increment to 10x the
    // step, and a mouse wheel click on a GtkRange moves by the page
    // increment — which would jump the slider across its whole range.
    let kbd_adjustment = gtk::Adjustment::new(0.0, 0.0, 100.0, 10.0, 10.0, 0.0);
    let kbd_scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&kbd_adjustment));
    kbd_scale.set_size_request(180, -1);
    kbd_scale.set_valign(gtk::Align::Center);
    kbd_scale.set_draw_value(true);
    kbd_scale.set_value_pos(gtk::PositionType::Left);
    kbd_scale.set_format_value_func(|_, value| format!("{value:.0}%"));
    kbd_scale.set_sensitive(false);
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
    let fp_scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&fp_adjustment));
    fp_scale.set_size_request(180, -1);
    fp_scale.set_valign(gtk::Align::Center);
    fp_scale.set_draw_value(true);
    fp_scale.set_value_pos(gtk::PositionType::Left);
    fp_scale.set_format_value_func(|_, value| format!("{value:.0}%"));
    fp_scale.set_sensitive(false);
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
        charge_row,
        kbd_scale,
        kbd_syncing: Cell::new(false),
        fp_scale,
        fp_combo,
        fp_custom_row: fp_row,
        fp_syncing: Cell::new(false),
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

    // Ask the daemon what this board supports, hide what it can't do, load
    // current values, then connect the setters — so programmatic set_value
    // during init can't echo back into the daemon.
    let init_ui = ui.clone();
    let battery_group = battery.clone();
    let keyboard_group = keyboard.clone();
    let fingerprint_group = fingerprint.clone();
    let touchpad_group = touchpad.clone();
    glib::spawn_future_local(async move {
        let ui = init_ui;
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
        battery_group.set_visible(caps.charge_limit);
        keyboard_group.set_visible(caps.keyboard_backlight);
        fingerprint_group.set_visible(caps.fp_brightness);
        touchpad_group.set_visible(caps.haptic_touchpad);
        ui.sync_tray_caps(caps);

        if caps.charge_limit {
            match proxy.get_charge_limit().await {
                Ok(limit) => {
                    ui.charge_row.set_value(limit as f64);
                    ui.charge_row.set_sensitive(true);
                    ui.sync_tray_charge_limit(limit);
                }
                Err(e) => ui.toast(&format!("Reading charge limit failed: {e}")),
            }
        }
        if caps.keyboard_backlight {
            match proxy.get_keyboard_backlight().await {
                Ok(percent) => {
                    ui.kbd_scale.set_value(percent as f64);
                    ui.kbd_scale.set_sensitive(true);
                }
                Err(e) => ui.toast(&format!("Reading keyboard backlight failed: {e}")),
            }
        }
        if caps.fp_brightness {
            let (levels, labels) = fp_levels_labels(caps.fp_custom);
            ui.fp_levels.set(levels);
            if !caps.fp_custom {
                ui.fp_combo.set_model(Some(&gtk::StringList::new(labels)));
            }
            match proxy.get_fingerprint_brightness().await {
                Ok((percent, level)) => {
                    ui.fp_scale.set_value(percent as f64);
                    ui.fp_combo.set_selected(ui.fp_combo_index(&level));
                    ui.fp_custom_row.set_visible(level == "custom");
                    ui.fp_scale.set_sensitive(true);
                    ui.fp_combo.set_sensitive(true);
                    ui.sync_tray_fp_level(&level);
                }
                Err(e) => ui.toast(&format!("Reading fingerprint brightness failed: {e}")),
            }
        }
        if caps.haptic_touchpad {
            match proxy.get_haptic_intensity().await {
                Ok(percent) => {
                    let index = HAPTIC_LEVELS.iter().position(|l| *l == percent).unwrap_or(3);
                    ui.haptic_combo.set_selected(index as u32);
                    ui.haptic_combo.set_sensitive(true);
                }
                Err(e) => ui.toast(&format!("Reading haptic intensity failed: {e}")),
            }
            match proxy.get_touchpad_click_force().await {
                Ok(force) => {
                    let index = CLICK_FORCES.iter().position(|f| *f == force).unwrap_or(1);
                    ui.force_combo.set_selected(index as u32);
                    ui.force_combo.set_sensitive(true);
                }
                Err(e) => ui.toast(&format!("Reading click force failed: {e}")),
            }
        }
        connect_setters(&ui, &proxy);
    });

    (window, ui)
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

fn setup_tray(app: &adw::Application, state: Rc<AppState>) {
    use ksni::blocking::TrayMethods;

    let (tx, rx) = async_channel::unbounded();
    let tray = TrayIcon {
        tx,
        charge_limit: None,
        fp_level: None,
        caps: Capabilities::default(),
    };
    let handle = match tray.spawn() {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("tray icon unavailable: {e}");
            return;
        }
    };
    *state.tray.borrow_mut() = Some(handle.clone());

    // Populate the tray menu right away: in tray-only mode (autostart)
    // nothing else fetches capabilities until the window is first opened,
    // which would leave the menu at Open/Quit. Window init later repeats
    // these syncs idempotently.
    glib::spawn_future_local(async move {
        let Ok(proxy) = daemon_proxy().await else { return };
        let Ok(names) = proxy.get_capabilities().await else { return };
        let caps = Capabilities::from_names(&names);
        tray_set_caps(&handle, caps);
        if caps.charge_limit
            && let Ok(limit) = proxy.get_charge_limit().await
        {
            tray_set_charge_limit(&handle, limit);
        }
        if caps.fp_brightness
            && let Ok((_, level)) = proxy.get_fingerprint_brightness().await
        {
            tray_set_fp_level(&handle, &level);
        }
    });

    let hold = app.hold();
    let app = app.clone();
    glib::spawn_future_local(async move {
        let _hold = hold;
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
                TrayEvent::SetChargeLimit(percent) => {
                    state.window_for(&app).1.charge_row.set_value(percent as f64)
                }
                TrayEvent::SetFingerprintLevel(level) => {
                    let ui = state.window_for(&app).1;
                    let index = ui.fp_combo_index(level);
                    ui.fp_combo.set_selected(index);
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
