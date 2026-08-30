//! How the window fills itself, and the page it shows while it cannot.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk4 as gtk;
use gtk4::gdk;
use gtk4::glib;

use super::{NO_HARDWARE, Ui};
use crate::mapped::{Once, while_mapped};
use crate::{about, board};

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
pub(super) struct EmptyPage {
    stack: gtk::Stack,
    status: adw::StatusPage,
    report: gtk::Button,
    detail: gtk::Label,
    progress: gtk::Label,
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

/// Builds the page and the stack that can put it in front of the controls,
/// which arrive already built, and makes the stack `view`'s content: the
/// two faces are made together because the stack is what makes either of
/// them reachable.
pub(super) fn build_empty_page(
    view: &adw::ToolbarView,
    controls: &adw::PreferencesPage,
) -> EmptyPage {
    let stack = gtk::Stack::new();
    view.set_content(Some(&stack));
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
    /// The page rather than a toast: a toast is gone in seconds and leaves a
    /// window whose emptiness has no explanation on it.
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
    /// Set while a fill is in flight, so two starting together cannot both
    /// be in it: two runs finishing would connect every setter twice, which
    /// is two writes and two prompts per change.
    filling: Cell<bool>,
    /// How long until the next unprompted attempt, doubling per failure.
    next_attempt: Cell<u32>,
    /// The pending tick, held so that arming replaces a countdown rather than
    /// racing it.
    retry: Cell<Option<Once>>,
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
        self.ui.load_values(&controls).await;
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
        self.retry
            .set(Some(Once::after(Duration::from_secs(1), move || {
                if remaining > 1 {
                    init.count_down(remaining - 1);
                    return;
                }
                init.empty.show_progress("Trying again…");
                glib::spawn_future_local(async move { init.fill().await });
            })));
    }

    /// Drops the pending tick, if there is one. Called where the wait has
    /// stopped mattering: the window leaving the screen, and the daemon
    /// answering.
    fn stop_retrying(&self) {
        self.retry.set(None);
    }

    /// Gates each group on its control, loads the values, then connects the
    /// setters — last, so a value loaded cannot echo back as a write. Returns
    /// the state the window is left in, or None where it was filled.
    async fn attempt(&self) -> Option<Empty> {
        let ui = &self.ui;
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
        ui.gate(&controls);
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
        ui.load_values(&controls).await;
        ui.connect_handlers(&controls);
        None
    }
}

/// Starts the window filling itself: on every map, and on the countdown
/// after a daemon that did not answer. Both starters share one `filling`
/// guard, and the window leaving the screen stops the countdown.
pub(super) fn attach(window: &adw::ApplicationWindow, ui: &Rc<Ui>, empty: EmptyPage) {
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
    // Nothing is lost by the countdown stopping with the window off screen:
    // the map asks again the moment anyone looks, which is also when an
    // answer could change what is on screen.
    while_mapped(window, move || {
        let refreshing = init.clone();
        glib::spawn_future_local(async move { refreshing.refresh().await });
        Retrying(init.clone())
    });
}

/// Stops the countdown when it falls.
struct Retrying(Rc<Init>);

impl Drop for Retrying {
    fn drop(&mut self) {
        self.0.stop_retrying();
    }
}
