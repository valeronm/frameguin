//! The preferences window: its widgets, the reads that fill them, and the
//! writes they send.
//!
//! [`Sink`] lives here rather than beside the tray because the window is the
//! end with somewhere to report; a group's `apply` takes one, so a tray
//! preset borrows the window when one has been built and answers for itself
//! when one has not.
//!
//! A control with a module of its own in `frameguin_model` has a group
//! module of its own here — its widgets, how a snapshot moves them, and what
//! its handlers dispatch — and the rest of this file is the window itself.

pub(crate) mod battery;
pub(crate) mod power_led;
mod touchpad;
pub(crate) mod touchscreen;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use frameguin_model::control::Controls;
use frameguin_wire::DeviceError;
use gtk4 as gtk;
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;

use crate::bus::Bus;
use crate::daemon::Daemon;
use crate::reading::Feed;
use crate::report::parts;
use crate::tray::{TrayIcon, TrayValues, tray_push};
use crate::{APP_ID, about, autostart, board};

/// How long the window waits before asking an unreachable daemon again, and
/// the ceiling the wait doubles up to. Bounded rather than endless-fast: the
/// service is bus-activated, so every attempt is a start attempt.
const FIRST_RETRY_SECONDS: u32 = 2;
const MAX_RETRY_SECONDS: u32 = 30;
/// The stack's two faces. Named rather than spelled at each call because
/// `set_visible_child_name` answers a name it doesn't know with nothing at
/// all — a typo would be a window that silently stops switching.
const CONTROLS_PAGE: &str = "controls";
const EMPTY_PAGE: &str = "empty";
/// The empty page's icon, sized here rather than left to the theme. One
/// number, so trying another size is one edit.
const EMPTY_ICON_CLASS: &str = "frameguin-empty-icon";
const EMPTY_ICON_PIXELS: u8 = 32;
/// The one sentence the header and the empty page both say, so a reword
/// cannot leave the window carrying two versions of it.
const NO_HARDWARE: &str = "No Framework hardware detected";

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

/// The chrome every slider in this window shares, so a change to how one
/// reads doesn't have to be made once per slider. `format` renders the value in
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

/// The empty page's secondary text: the error and what the app is doing
/// about it, which sit adjacent and have to read as one voice. Hidden to
/// start, since each is shown only in the states that have something to put
/// in it.
fn caption_label() -> gtk::Label {
    let label = gtk::Label::builder().visible(false).build();
    label.add_css_class("dim-label");
    label.add_css_class("caption");
    label
}

/// A `StringList` from owned labels, which every combo here builds from a
/// `Vec<String>` that GTK will only take as borrowed strs.
fn string_list(labels: &[String]) -> gtk::StringList {
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    gtk::StringList::new(&labels)
}

/// Names a combo row for GTK, which addresses rows by u32. Positions come
/// from fixed arrays of a handful of entries, so the fallback is unreachable
/// — it is what keeps the conversion total without a cast.
fn combo_index(position: usize) -> u32 {
    u32::try_from(position).unwrap_or(gtk::INVALID_LIST_POSITION)
}

/// The row to select, where the hardware may be sitting on none of them —
/// GTK spells "nothing selected" as a sentinel position rather than as an
/// absent one.
fn combo_selection(position: Option<usize>) -> u32 {
    position.map_or(gtk::INVALID_LIST_POSITION, combo_index)
}

/// The inverse of [`combo_selection`]: the row a combo reports, and None for
/// the sentinel GTK uses where it sits on none.
fn combo_position(selected: u32) -> Option<usize> {
    (selected != gtk::INVALID_LIST_POSITION).then_some(selected as usize)
}

pub(crate) struct Ui {
    toasts: adw::ToastOverlay,
    /// Set while widgets are being moved to mirror the hardware, so their
    /// change handlers don't echo the reading back as a write.
    syncing: Cell<bool>,
    battery: battery::Group,
    power_led: power_led::Group,
    touchpad: touchpad::Group,
    touchscreen: touchscreen::Group,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
    daemon: Rc<Daemon>,
    /// Where the status row's reading comes from, shared with the battery
    /// report so the two windows cost the EC one walk between them rather than
    /// one apiece.
    feed: Rc<Feed>,
}

impl Ui {
    fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    /// A call that failed, named by what was being attempted — "Setting the
    /// charge limit", "Reading the battery". The sentence is built here rather
    /// than at each site so that every failure loses the D-Bus error name and
    /// none has to remember to; what each site still spells is its own half,
    /// which is the half no other site could supply. Takes a bus error or a
    /// device's `DeviceError` alike, the conversion being what drops the name.
    fn toast_error(&self, attempt: &str, error: impl Into<DeviceError>) {
        self.toast(&format!("{attempt} failed: {}", error.into()));
    }

    /// Moves widgets to match the hardware without their handlers writing the
    /// value straight back. Every setter returns early while this is set.
    fn sync(&self, update: impl FnOnce()) {
        self.syncing.set(true);
        update();
        self.syncing.set(false);
    }

    fn sync_tray(&self, values: TrayValues) {
        if let Some(handle) = &self.tray {
            tray_push(handle, values);
        }
    }
}

/// A row a sync moved the combo to must not be written back, and a combo
/// wired here cannot forget that.
fn connect_combo<T>(
    ui: &Rc<Ui>,
    combo: &adw::ComboRow,
    at: impl Fn(usize) -> Option<T> + 'static,
    write: impl Fn(&Rc<Ui>, T) + 'static,
) {
    let ui = ui.clone();
    combo.connect_selected_notify(move |row| {
        if ui.syncing.get() {
            return;
        }
        if let Some(value) = combo_position(row.selected()).and_then(&at) {
            write(&ui, value);
        }
    });
}

/// Shows `row` exactly while `combo` sits at `index`, so a slider a combo
/// row reveals is never left showing under another row, whoever moved the
/// combo.
fn reveal_under(combo: &adw::ComboRow, row: &impl IsA<gtk::Widget>, index: usize) {
    combo
        .bind_property("selected", row, "visible")
        .transform_to(move |_, selected: u32| Some(combo_position(selected) == Some(index)))
        .sync_create()
        .build();
}

/// When a slider's value reaches the hardware.
#[derive(Clone, Copy)]
enum SliderWrites {
    /// When a drag ends, keys and the wheel settling on a longer debounce:
    /// the control shows nothing while it changes, and every value passed
    /// through would be one more authorized EC write.
    OnRelease,
    /// As it moves, on a short debounce: the control shows every value, so
    /// the passes are the point.
    Live,
}

impl SliderWrites {
    fn delay(self) -> Duration {
        match self {
            Self::OnRelease => Duration::from_millis(700),
            Self::Live => Duration::from_millis(200),
        }
    }
}

/// Wires a slider to `write`, under the `writes` policy. `read` turns the
/// slider's position into the value that gets written, and is also what
/// decides whether a drag moved at all — comparing positions would count a
/// nudge that rounds back to where it started.
fn connect_slider_writes<T: Copy + PartialEq + 'static>(
    ui: &Rc<Ui>,
    scale: &gtk::Scale,
    read: impl Fn(f64) -> T + 'static,
    write: impl Fn(T) + 'static,
    writes: SliderWrites,
) {
    let read = Rc::new(read);
    let write = Rc::new(write);
    let dragging: Rc<Cell<Option<T>>> = Rc::default();

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
        debounce(&slot, writes.delay(), move || write(value));
    });
    if matches!(writes, SliderWrites::Live) {
        return;
    }

    // Raw events, not a gesture: the scale's own drag gesture claims the
    // pointer sequence, which cancels any competing gesture instead of
    // releasing it — so a GestureClick here would see the press and never the
    // release, and the drag would never end.
    let drag = gtk::EventControllerLegacy::new();
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    let drag_scale = scale.clone();
    drag.connect_event(move |_, event| {
        match event.event_type() {
            gdk::EventType::ButtonPress | gdk::EventType::TouchBegin => {
                dragging.set(Some(read(drag_scale.value())));
            }
            gdk::EventType::ButtonRelease
            | gdk::EventType::TouchEnd
            | gdk::EventType::TouchCancel => {
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

// A control's group connects only where the control is, its handlers needing
// one to dispatch to.
fn connect_handlers(ui: &Rc<Ui>, controls: &Controls<Bus>) {
    if let Some(battery) = &controls.battery {
        ui.battery.connect(ui, battery);
    }
    if let Some(power_led) = &controls.power_led {
        ui.power_led.connect(ui, power_led);
    }
    if let Some(touchpad) = &controls.touchpad {
        ui.touchpad.connect(ui, touchpad);
    }
    if let Some(touchscreen) = &controls.touchscreen {
        ui.touchscreen.connect(ui, touchscreen);
    }
}

/// Where a write reports back to. A tray preset can arrive in a session whose
/// window has never been built, and building a widget tree to hold a toast
/// nobody will see is not worth it — so the tray answers for itself, and only
/// the window carries the parts a window has.
#[derive(Clone, Copy)]
pub(crate) enum Sink<'a> {
    Window(&'a Ui),
    Tray(&'a ksni::blocking::Handle<TrayIcon>),
}

impl Sink<'_> {
    fn toast(&self, message: &str) {
        if let Sink::Window(ui) = self {
            ui.toast(message);
        }
    }

    fn toast_error(&self, attempt: &str, error: impl Into<DeviceError>) {
        if let Sink::Window(ui) = self {
            ui.toast_error(attempt, error);
        }
    }

    /// Sends what this sink can vouch for to the tray, wherever it lives.
    fn push_tray(&self, values: TrayValues) {
        match self {
            Sink::Window(ui) => ui.sync_tray(values),
            Sink::Tray(handle) => tray_push(handle, values),
        }
    }
}

pub(crate) fn build_window(
    app: &adw::Application,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
    daemon: Rc<Daemon>,
    feed: Rc<Feed>,
) -> (adw::ApplicationWindow, Rc<Ui>) {
    let page = adw::PreferencesPage::new();

    let battery = battery::Group::build();
    page.add(&battery.widget);

    let power_led = power_led::Group::build();
    page.add(&power_led.widget);

    let touchpad = touchpad::Group::build();
    page.add(&touchpad.widget);

    let touchscreen = touchscreen::Group::build();
    page.add(&touchscreen.widget);

    let application = adw::PreferencesGroup::builder()
        .title("Application")
        .build();
    let autostart_row = adw::SwitchRow::builder()
        .title("Start at login")
        .subtitle("Show only the tray icon until opened")
        .build();
    autostart_row.set_active(autostart::entry_path().exists());
    application.add(&autostart_row);
    page.add(&application);

    // Detected hardware as the header subtitle: one line, no key/value rows.
    let detected = board::detected().unwrap_or_else(|| NO_HARDWARE.to_string());

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Frameguin", &detected)));

    let menu = gio::Menu::new();
    menu.append(Some("_Parts"), Some(&format!("app.{}", parts::ACTION)));
    menu.append(Some("_About Frameguin"), Some("app.about"));
    menu.append(Some("_Quit"), Some("app.quit"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Main menu")
        .build();
    header.pack_end(&menu_button);
    view.add_top_bar(&header);
    let empty = build_empty_page(&page);
    view.set_content(Some(&empty.stack));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&view));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Frameguin")
        .default_width(420)
        // Tall enough for every control group at the default font scale;
        // re-measure when the rows change.
        .default_height(710)
        .content(&toasts)
        .icon_name(APP_ID)
        .build();

    // Hiding instead of closing only makes sense while a tray icon exists to
    // bring the window back.
    window.set_hide_on_close(tray.is_some());

    let ui = Rc::new(Ui {
        toasts,
        syncing: Cell::new(false),
        battery,
        power_led,
        touchpad,
        touchscreen,
        tray,
        daemon,
        feed,
    });

    let autostart_ui = ui.clone();
    autostart_row.connect_active_notify(move |row| {
        if let Err(e) = autostart::set(row.is_active()) {
            // Quoted off the row rather than spelled again: a message naming
            // a row by a title the row no longer has is worse than a vaguer
            // one.
            autostart_ui.toast(&format!("Setting “{}” failed: {e}", row.title()));
        }
    });

    // The report goes into the issue body rather than onto the clipboard with
    // instructions to paste it somewhere.
    let report_ui = ui.clone();
    empty.report.connect_clicked(move |button| {
        // Insensitive for the length of the gather: this is offered from the
        // state where nothing answers, so a second click would open a second
        // bus connection and a second browser tab.
        button.set_sensitive(false);
        let ui = report_ui.clone();
        let button = button.clone();
        glib::spawn_future_local(async move {
            if let Err(e) = about::report_issue().await {
                ui.toast(&format!("Opening the browser failed: {e}"));
            }
            button.set_sensitive(true);
        });
    });
    let init = Rc::new(Init {
        ui: ui.clone(),
        empty,
        answered: Cell::default(),
        filling: Cell::default(),
        next_attempt: Cell::new(FIRST_RETRY_SECONDS),
        retry: Cell::default(),
    });
    let map_init = init.clone();
    window.connect_map(move |_| {
        let init = map_init.clone();
        glib::spawn_future_local(async move { init.refresh().await });
    });
    // A window hidden to the tray does no periodic work, the rule every other
    // timer here follows. Nothing is lost by stopping: the map above asks
    // again the moment anyone looks, which is also when an answer could
    // change what is on screen.
    window.connect_unmap(move |_| init.stop_retrying());

    (window, ui)
}

/// Why the window has no controls. Told apart because what the reader should
/// do differs: a machine that is not a Framework never will have any, an
/// unreachable service is a thing to retry, and a Framework board answering
/// with none is worth reporting. An empty window that does not say which
/// leaves all three looking like the app failing to start.
enum Empty {
    /// Carries the vendor the machine names itself with, so every variant
    /// arrives self-describing and the page's text costs no sysfs read of
    /// its own.
    NoHardware(String),
    DaemonUnavailable(String),
    NoControls,
}

/// The window's other face: a page saying why there is nothing to show, and
/// the stack that puts it in front of the controls. One type because filling
/// the page and switching to it are halves of a single act — a status page
/// nothing switches to is invisible, and a switch to an unfilled one is a
/// blank window.
struct EmptyPage {
    stack: gtk::Stack,
    status: adw::StatusPage,
    report: gtk::Button,
    detail: gtk::Label,
    progress: gtk::Label,
}

/// Builds the page and the stack that can put it in front of the controls,
/// which arrive already built: the two faces are made together because the
/// stack is what makes either of them reachable.
fn build_empty_page(controls: &adw::PreferencesPage) -> EmptyPage {
    let stack = gtk::Stack::new();
    // Homogeneous by default, which measures every page on every layout pass
    // and sizes the window to the larger of the two. Nothing here animates
    // between them, so the page that isn't showing should cost nothing.
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    // The controls first, so a window opened before detection answers shows
    // the page it will keep in the ordinary case rather than flashing an
    // explanation of an emptiness that is about to be filled.
    stack.add_named(controls, Some(CONTROLS_PAGE));

    let status = adw::StatusPage::new();
    // Compact, which is libadwaita's own answer to the page-high icon: this
    // window is 420px wide and the icon at full size fills it, which reads as
    // a catastrophe whatever the icon happens to be.
    status.add_css_class("compact");
    // Compact is libadwaita's smallest, and still larger than this page wants:
    // the icon is a label on a sentence, not the subject of the screen. The
    // rule is scoped to a class of this app's own so it cannot reach another
    // status page, and it names the image node because the size is the icon
    // theme's to choose otherwise.
    let icon_css = gtk::CssProvider::new();
    icon_css.load_from_data(&format!(
        ".{EMPTY_ICON_CLASS} image {{ -gtk-icon-size: {EMPTY_ICON_PIXELS}px; }}"
    ));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &icon_css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    status.add_css_class(EMPTY_ICON_CLASS);
    stack.add_named(&status, Some(EMPTY_PAGE));

    let detail = caption_label();
    // Selectable because the first thing asked of anyone reporting this is
    // what it said, and a screenshot of an error is worse than its text.
    detail.set_wrap(true);
    detail.set_justify(gtk::Justification::Center);
    detail.set_selectable(true);
    // Neither action asks the reader to do the app's work: it retries on its
    // own, and the report carries what it knows without anyone running a
    // command for it.

    // Title case against the app's sentence case: libadwaita spells this
    // action "Report an Issue" in the About window, one menu away.
    let report = gtk::Button::builder().label("Report an Issue").build();
    report.add_css_class("pill");
    report.add_css_class("suggested-action");
    // Quit beside it, on every one of these states: an app whose whole
    // purpose is controlling hardware has nothing to offer a machine with
    // none, so quitting is the conclusion rather than a consolation — there
    // is nothing to leave running for. The header menu carries Quit as well;
    // what the page adds is offering it where the reason for it is. Both
    // point at the same action, so there is nothing here to keep in step.
    let quit = gtk::Button::builder()
        .label("Quit")
        .action_name("app.quit")
        .build();
    quit.add_css_class("pill");
    let buttons = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .spacing(12)
        .build();
    buttons.append(&report);
    buttons.append(&quit);
    // What the app is doing about it, between the error and the buttons: a
    // page that says nothing while it waits looks like one that has given up.
    let progress = caption_label();

    // The error first: it is what the page is really saying, and the buttons
    // are what to do about it.
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .build();
    actions.append(&detail);
    actions.append(&progress);
    actions.append(&buttons);
    status.set_child(Some(&actions));

    EmptyPage {
        stack,
        status,
        report,
        detail,
        progress,
    }
}

/// One empty page's contents. A struct rather than the tuple this grew out
/// of, so a state cannot silently take another's icon or leave a button on.
struct EmptyText {
    icon: &'static str,
    title: &'static str,
    /// What is known about the state, in the app's own words. None where the
    /// error below says it better.
    description: Option<String>,
    /// The machine's own words, verbatim and selectable.
    detail: Option<String>,
    report: bool,
}

impl EmptyPage {
    fn show(&self, reason: Empty) {
        // Only the unreachable service warns. It is the one state where the
        // app cannot do its job and the reader may be able to fix it; the
        // other two are reports about the machine, which an alert icon would
        // overstate.
        let text = match reason {
            Empty::NoHardware(vendor) => EmptyText {
                icon: "computer-symbolic",
                title: NO_HARDWARE,
                description: Some(format!(
                    "Frameguin controls the hardware of Framework laptops. \
                     This machine reports itself as “{vendor}”."
                )),
                detail: None,
                // Working as designed on someone else's laptop, so there is
                // nothing here anyone wants filed.
                report: false,
            },
            // No sentence of our own: what went wrong is the bus's to say,
            // and a paraphrase would add length without adding knowledge.
            // No instructions either — the app cannot tell a masked unit from
            // a crashed one, so anything it told the reader to run would be a
            // guess dressed as advice.
            Empty::DaemonUnavailable(e) => EmptyText {
                icon: "dialog-warning-symbolic",
                title: "frameguin-daemon isn’t answering",
                description: None,
                detail: Some(e),
                report: true,
            },
            Empty::NoControls => EmptyText {
                icon: "dialog-information-symbolic",
                title: "No controls for this board",
                description: Some(
                    "The daemon reached the hardware and found none of the operations \
                     this app offers."
                        .to_string(),
                ),
                detail: None,
                // A Framework board detection finds nothing on is the report
                // this project asks for by name, and the report already
                // carries the parts it found.
                report: true,
            },
        };
        self.status.set_icon_name(Some(text.icon));
        self.status.set_title(text.title);
        self.status.set_description(text.description.as_deref());
        self.detail
            .set_text(text.detail.as_deref().unwrap_or_default());
        self.detail.set_visible(text.detail.is_some());
        self.report.set_visible(text.report);
        // Cleared here rather than by whoever set it: every path that changes
        // which state is shown comes through this, and a line about an
        // attempt that belongs to the state before it would be a lie the
        // reader has no way to spot.
        self.progress.set_visible(false);
        self.stack.set_visible_child_name(EMPTY_PAGE);
    }

    fn show_controls(&self) {
        self.stack.set_visible_child_name(CONTROLS_PAGE);
    }

    /// What the app is doing while the reader waits.
    fn show_progress(&self, line: &str) {
        self.progress.set_text(line);
        self.progress.set_visible(true);
    }
}

/// What filling the window takes: where an answer goes, and where a failure
/// says so. One type behind one `Rc` because both callers — the window
/// arriving on screen and the countdown that keeps asking — need all of it
/// and differ only in what made them run.
struct Init {
    ui: Rc<Ui>,
    empty: EmptyPage,
    /// Whether an ask has reached the daemon, which is the answer to whether
    /// asking again could tell us anything new: one that answered will answer
    /// the same way for as long as it runs. A flag rather than the connection
    /// it used to be kept as — `Daemon` holds the connection, and a handle
    /// stored to be tested for presence says the wrong thing about why it is
    /// there.
    answered: Cell<bool>,
    /// Set while a fill is in flight, so the two things that start one — the
    /// window mapping and the countdown reaching zero — cannot both be in it:
    /// two runs finishing would connect every setter twice, which is two
    /// writes and two prompts per change.
    filling: Cell<bool>,
    /// How long until the next unprompted attempt, doubling per failure.
    next_attempt: Cell<u32>,
    /// The pending tick, held so that arming replaces a countdown rather than
    /// racing it, and so the window going off screen can stop it.
    retry: Cell<Option<glib::SourceId>>,
}

impl Init {
    /// What the window does every time it comes to the screen. The hardware
    /// moves while the app sits in the tray — the EC's battery extender
    /// lowers the charge limit on its own, and `framework_tool` writes any of
    /// these behind the app's back — so a mapped window reloads rather than
    /// trusting what it read at startup. A window still holding no
    /// controls has nothing to reload and asks the daemon again instead,
    /// which is how a service that started late is picked up without the
    /// reader finding the button.
    async fn refresh(self: &Rc<Self>) {
        if !self.answered.get() {
            self.fill().await;
            return;
        }
        // Cached since detection, so this cannot realistically fail; nothing
        // to say if it does, the next map asking again.
        let Ok(controls) = self.ui.daemon.controls().await else {
            return;
        };
        // Answered, with nothing to reload: a board that supports none of
        // these controls and a machine that is not a Framework are what they
        // are, and asking again would spawn the root daemon once per window
        // opened to be told so a second time.
        if controls.is_empty() {
            return;
        }
        load_values(&self.ui, &controls).await;
    }

    /// One attempt at filling the window, and what to do with how it went:
    /// only an unreachable service is worth another, and the waiting between
    /// them is this function's to arrange.
    async fn fill(self: &Rc<Self>) {
        if self.filling.replace(true) {
            return;
        }
        let reason = self.attempt().await;
        self.filling.set(false);
        let Some(reason) = reason else {
            self.stop_retrying();
            self.next_attempt.set(FIRST_RETRY_SECONDS);
            return;
        };
        // Only an unreachable service is worth asking about again, and the
        // waiting is the app's to do: a service that starts on demand can be
        // slow, restarting, or not installed yet, and none of those are work
        // to hand to whoever opened the window. The other two states are what
        // the machine is.
        let again = matches!(reason, Empty::DaemonUnavailable(_));
        self.empty.show(reason);
        if again {
            let delay = self.next_attempt.get();
            self.next_attempt.set((delay * 2).min(MAX_RETRY_SECONDS));
            self.count_down(delay);
        }
    }

    /// Ticks the wait out a second at a time so the page can name what it is
    /// waiting for. A silent page and a page that has given up look identical
    /// — and with the delay doubling to half a minute, silence is most of
    /// what a reader would see.
    ///
    /// Arming replaces whatever was pending, so the window being opened
    /// mid-wait moves the countdown rather than starting a second one beside
    /// it, doubling the backoff twice as fast and writing two numbers into
    /// one label.
    fn count_down(self: &Rc<Self>, remaining: u32) {
        self.empty.show_progress(&format!(
            "Trying again in {remaining} {}",
            if remaining == 1 { "second" } else { "seconds" }
        ));
        let init = self.clone();
        let tick = glib::timeout_add_seconds_local_once(1, move || {
            // Finished the moment it fires: forget it before anything here
            // arms its replacement, so nothing removes a dead source.
            init.retry.set(None);
            if remaining > 1 {
                init.count_down(remaining - 1);
                return;
            }
            init.empty.show_progress("Trying again…");
            glib::spawn_future_local(async move { init.fill().await });
        });
        self.stop_retrying();
        self.retry.set(Some(tick));
    }

    /// Drops the pending tick, if there is one. Called where the wait has
    /// stopped mattering: the window leaving the screen, and the daemon
    /// answering.
    fn stop_retrying(&self) {
        if let Some(tick) = self.retry.take() {
            tick.remove();
        }
    }

    /// Takes the detected controls from the daemon, gates each group on its
    /// control, loads current values, then connects the setters — last, so
    /// the programmatic `set_value` calls during init can't echo back into
    /// the daemon. Returns the state the window is left in, or None where it
    /// was filled.
    async fn attempt(&self) -> Option<Empty> {
        let ui = &self.ui;
        // The page rather than a toast: a toast is gone in seconds and leaves
        // a window whose emptiness has no explanation on it.
        // Through `Daemon` rather than dialled and asked here: a fresh
        // handshake per window and a second cold detection per session are
        // what a window answering for itself costs, and that handle outlives
        // any of them. The answer reaching it here is also what spares the
        // report a detection of its own.
        let controls = match ui.daemon.controls().await {
            Ok(controls) => controls,
            Err(e) => {
                // The page is replaced by the retry that succeeds, so the
                // failure is also written where it stays.
                eprintln!("frameguin-daemon unreachable: {e}");
                return Some(Empty::DaemonUnavailable(e.to_string()));
            }
        };
        ui.battery.gate(controls.battery.as_ref());
        ui.power_led.gate(controls.power_led.as_ref());
        ui.touchpad.gate(controls.touchpad.as_ref());
        ui.touchscreen.gate(controls.touchscreen.as_ref());
        // Set whatever the answer was: what a later refresh needs to know is
        // that this daemon has said its piece, not what it said.
        self.answered.set(true);
        if controls.is_empty() {
            // The daemon gates its EC on the same vendor string the app
            // reads, so an empty answer is expected on anything else and says
            // nothing about the board; only a Framework answering with none
            // is a finding.
            return Some(match board::detected() {
                Some(_) => Empty::NoControls,
                None => Empty::NoHardware(board::dmi("sys_vendor")),
            });
        }
        // Back to the controls, for a run that got here after an earlier one
        // put the empty page up.
        self.empty.show_controls();
        load_values(ui, &controls).await;
        connect_handlers(ui, &controls);
        None
    }
}

/// Re-reads every detected control and moves the widgets to match, pushing
/// the same values to the tray along with what each control offers. Each
/// write goes through `Ui::sync`, so a reload can't echo back as a setter
/// call. The tray's copies are collected and handed over in one go at the
/// end: each push blocks on the tray's thread and rebuilds its whole menu,
/// which would be wasted once per control on a menu nobody has opened.
async fn load_values(ui: &Rc<Ui>, controls: &Controls<Bus>) {
    let mut values = TrayValues::offered(controls);
    if let Some(battery) = &controls.battery {
        ui.battery.load(ui, battery, &mut values).await;
    }
    if let Some(power_led) = &controls.power_led {
        ui.power_led.load(ui, power_led, &mut values).await;
    }
    if let Some(touchpad) = &controls.touchpad {
        ui.touchpad.load(ui, touchpad).await;
    }
    if let Some(touchscreen) = &controls.touchscreen {
        ui.touchscreen.load(ui, touchscreen, &mut values).await;
    }
    ui.sync_tray(values);
}
