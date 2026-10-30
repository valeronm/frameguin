//! The Readings window: what the hardware is doing now, as sections of rows in
//! a sidebar and the selected row's page beside them.
//!
//! A rarely needed switch sits on the page of the part it acts on, under
//! [`crate::synced`]'s guard; nothing here debounces or pushes to the tray.

mod battery;
mod battery_extender;
mod chassis;
mod layers;
mod ports;
mod privacy_switches;
mod thermal;
mod track;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use frameguin_contract::{BatteryFeature, DeviceError};
use frameguin_model::reading::{Reading, Request};
use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;

use self::ports::CONTENT_WIDTH;
use super::{Shell, collapse_when_narrow, headed};
use crate::daemon::Daemon;
use crate::failure;
use crate::reading::{Feed, show_while_mapped};
use crate::synced::Guard;

/// The only way the window is opened.
pub(crate) const ACTION: &str = "status";

const TITLE: &str = "Readings";

const SIDEBAR_WIDTH: f64 = 260.0;

/// Only the window holds the rows a target is settled against, so the
/// application action forwards its target to this action on the window.
const SELECT: &str = "select";

/// A page the window can be opened on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Battery,
    /// The port powering the machine, or the first port where none is —
    /// settled against the reading the window holds when it is opened.
    Charger,
}

impl Target {
    /// A target missing here is read back as no target at all.
    const ALL: [Self; 2] = [Self::Battery, Self::Charger];

    fn name(self) -> &'static str {
        match self {
            Self::Battery => "battery",
            Self::Charger => "charger",
        }
    }
}

/// The action's parameter. No target opens the window on whatever it already
/// shows, and a fresh window on its first row.
///
/// A string, empty for no target, rather than a maybe: the application's
/// actions are exported over D-Bus, which has no maybe type.
pub(crate) fn target(target: Option<Target>) -> glib::Variant {
    target.map_or("", Target::name).to_variant()
}

fn targeted(parameter: Option<&glib::Variant>) -> Option<Target> {
    let name = parameter?.str()?;
    Target::ALL.into_iter().find(|target| target.name() == name)
}

pub(super) fn action(daemon: Rc<Daemon>, feed: Rc<Feed>) -> gio::ActionEntry<adw::Application> {
    let fill = move |shell: Shell, window: &adw::Window| build(shell, window, &daemon, &feed);
    gio::ActionEntry::builder(ACTION)
        .parameter_type(Some(glib::VariantTy::STRING))
        .activate(move |app: &adw::Application, _, parameter| {
            let window = super::open(app, ACTION, TITLE, (760, 700), &fill);
            let _ = window.activate_action(&format!("{ACTION}.{SELECT}"), parameter);
        })
        .build()
}

fn build(
    shell: Shell,
    window: &adw::Window,
    daemon: &Rc<Daemon>,
    feed: &Rc<Feed>,
) -> adw::NavigationSplitView {
    let (sidebar, split) = Sidebar::new();
    collapse_when_narrow(window, &split, SIDEBAR_WIDTH + f64::from(CONTENT_WIDTH));
    window.set_width_request(CONTENT_WIDTH);

    let select = gio::SimpleAction::new(SELECT, Some(glib::VariantTy::STRING));
    let selecting = sidebar.clone();
    select.connect_activate(move |_, parameter| selecting.select(targeted(parameter)));
    let actions = gio::SimpleActionGroup::new();
    actions.add_action(&select);
    window.insert_action_group(ACTION, Some(&actions));

    let daemon = daemon.clone();
    let feed = feed.clone();
    glib::spawn_future_local(async move {
        let writes = Rc::new(Writes {
            guard: Guard::default(),
            toasts: shell.toasts.downgrade(),
        });
        fill(&sidebar, &daemon, &feed, &shell, &writes).await;
        sidebar.pages.set_opacity(1.0);
    });
    split
}

async fn fill(
    sidebar: &Rc<Sidebar>,
    daemon: &Daemon,
    feed: &Rc<Feed>,
    shell: &Shell,
    writes: &Rc<Writes>,
) {
    let controls = match daemon.controls().await {
        Ok(controls) => controls,
        Err(e) => {
            shell.toast_error("Reading the hardware", e);
            return;
        }
    };
    // A window closed while the daemon was being dialled has nothing left
    // to draw into.
    let Some(split) = sidebar
        .split
        .upgrade()
        .filter(|split| split.root().is_some())
    else {
        return;
    };
    if let Some(control) = &controls.thermal {
        let section = sidebar.section("Thermals");
        thermal::add(sidebar, section, feed, control, controls.board.platform());
    }
    if let Some(control) = &controls.battery {
        let section = sidebar.section("Battery");
        battery::add(sidebar, section, feed, control);
        if control.has(BatteryFeature::Extender) {
            battery_extender::add(sidebar, section, feed);
        }
    }
    if let Some(control) = &controls.ports {
        ports::add(sidebar, feed, controls.usb.is_some(), control, writes);
    }
    if controls.chassis.is_some() || controls.privacy_switches.is_some() {
        let section = sidebar.section("Switches");
        if let Some(control) = &controls.chassis {
            chassis::add(sidebar, section, feed, control);
        }
        if controls.privacy_switches.is_some() {
            privacy_switches::add(sidebar, section, feed);
        }
    }
    if sidebar.sections.borrow().is_empty() {
        sidebar.pages.add_child(
            &adw::StatusPage::builder()
                .icon_name("dialog-information-symbolic")
                .title("No readings to show")
                .description("The daemon found nothing on this machine whose state it reads.")
                .build(),
        );
    }
    let failure = match feed.fill(&split).await {
        Ok((_, failure)) => failure.map(|failure| failure.error),
        Err(e) => Some(e),
    };
    sidebar.filled.set(true);
    sidebar.settle();
    if let Some(e) = failure {
        shell.toast_error("Reading the hardware", e);
    }
}

/// What a control on a page writes through, one for the window.
struct Writes {
    guard: Guard,
    /// Weak: an ancestor of every control whose handler holds this.
    toasts: glib::WeakRef<adw::ToastOverlay>,
}

impl Writes {
    fn toast_error(&self, attempt: &str, error: impl Into<DeviceError>) {
        if let Some(toasts) = self.toasts.upgrade() {
            toasts.add_toast(failure::toast(attempt, error));
        }
    }
}

struct SidebarRow {
    row: gtk::ListBoxRow,
    page: gtk::Widget,
    title: String,
    section: Section,
}

/// A heading in the sidebar, which rows are added under.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Section(usize);

/// The row a target opens, and what its page does once that row is selected.
struct Answer {
    row: gtk::ListBoxRow,
    then: Option<Box<dyn FnOnce()>>,
}

impl Answer {
    fn row(row: &impl IsA<gtk::ListBoxRow>) -> Self {
        Self {
            row: row.clone().upcast(),
            then: None,
        }
    }
}

type Find = dyn Fn(Target) -> Option<Answer>;

/// Holds the split view weakly, the rows' subscriptions hanging on it and
/// holding this. Everything else here is a descendant of it.
struct Sidebar {
    split: glib::WeakRef<adw::NavigationSplitView>,
    /// A list with no selected row hands its first row the selection along
    /// with keyboard focus entering it.
    list: gtk::ListBox,
    /// Indexed by [`Section`], in the order the rows are listed.
    sections: RefCell<Vec<String>>,
    pages: gtk::Stack,
    titled: adw::NavigationPage,
    heading: adw::WindowTitle,
    rows: RefCell<Vec<SidebarRow>>,
    finds: RefCell<Vec<Box<Find>>>,
    /// A target whose row has not arrived, or that arrived before the fill.
    pending: Cell<Option<Target>>,
    /// Whether the fill has read, before which nothing is selected: a target's
    /// row can depend on that reading.
    filled: Cell<bool>,
}

impl Sidebar {
    fn new() -> (Rc<Self>, adw::NavigationSplitView) {
        let list = gtk::ListBox::builder()
            .css_classes(["navigation-sidebar"])
            .build();
        let listed = headed(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&list)
                .build(),
        );
        let pages = gtk::Stack::new();
        // A homogeneous stack measures every page on every layout pass, where
        // only the page on screen has anything to say about the pane's size.
        pages.set_hhomogeneous(false);
        pages.set_vhomogeneous(false);
        // Transparent rather than hidden until the first fill has picked the
        // page to open on: a hidden page is unmapped, and an unmapped page
        // subscribes to nothing the fill would read.
        pages.set_opacity(0.0);
        let heading = adw::WindowTitle::new(TITLE, "");
        let content = adw::ToolbarView::builder()
            .width_request(CONTENT_WIDTH)
            .build();
        content.add_top_bar(&adw::HeaderBar::builder().title_widget(&heading).build());
        content.set_content(Some(&pages));
        let titled = adw::NavigationPage::builder()
            .title(TITLE)
            .child(&content)
            .build();
        let sidebar = adw::NavigationPage::builder()
            .title(TITLE)
            .child(&listed)
            .build();
        let split = adw::NavigationSplitView::builder()
            .sidebar(&sidebar)
            .content(&titled)
            .min_sidebar_width(SIDEBAR_WIDTH)
            .build();
        let this = Rc::new(Self {
            split: split.downgrade(),
            list,
            sections: RefCell::default(),
            pages,
            titled,
            heading,
            rows: RefCell::default(),
            finds: RefCell::default(),
            pending: Cell::default(),
            filled: Cell::default(),
        });
        // Weak: the list is a descendant of what this holds.
        let heading = Rc::downgrade(&this);
        this.list.set_header_func(move |row, before| {
            let Some(sidebar) = heading.upgrade() else {
                return;
            };
            let opens = sidebar.section_of(row).filter(|&section| {
                before.and_then(|before| sidebar.section_of(before)) != Some(section)
            });
            let Some(section) = opens else {
                row.set_header(None::<&gtk::Widget>);
                return;
            };
            let kept = row
                .header()
                .and_downcast::<gtk::Label>()
                .is_some_and(|label| label.label() == sidebar.sections.borrow()[section.0]);
            if !kept {
                row.set_header(Some(&sidebar.header(section)));
            }
        });
        let showing = Rc::downgrade(&this);
        this.list.connect_row_selected(move |_, row| {
            if let (Some(sidebar), Some(row)) = (showing.upgrade(), row) {
                sidebar.show(row);
            }
        });
        let revealing = Rc::downgrade(&this);
        this.list.connect_row_activated(move |_, _| {
            if let Some(sidebar) = revealing.upgrade() {
                // A row the reader picked outranks a target still waiting on
                // its reading.
                sidebar.pending.set(None);
                sidebar.reveal();
            }
        });
        (this, split)
    }

    /// A heading listed after every one before it, shown once a row is added
    /// under it.
    fn section(&self, heading: &str) -> Section {
        let mut sections = self.sections.borrow_mut();
        sections.push(heading.to_owned());
        Section(sections.len() - 1)
    }

    fn position(&self, row: &gtk::ListBoxRow) -> Option<usize> {
        self.rows
            .borrow()
            .iter()
            .position(|listed| listed.row == *row)
    }

    fn section_of(&self, row: &gtk::ListBoxRow) -> Option<Section> {
        self.position(row)
            .map(|index| self.rows.borrow()[index].section)
    }

    fn header(&self, section: Section) -> gtk::Label {
        gtk::Label::builder()
            .label(&self.sections.borrow()[section.0])
            .xalign(0.0)
            .margin_start(12)
            .margin_top(12)
            .margin_bottom(6)
            .css_classes(["heading"])
            .build()
    }

    fn add(&self, section: Section, title: &str, page: &impl IsA<gtk::Widget>) -> adw::ActionRow {
        let row = adw::ActionRow::builder().title(title).build();
        self.pages.add_child(page);
        let position = {
            let mut rows = self.rows.borrow_mut();
            let position = rows
                .iter()
                .filter(|listed| listed.section <= section)
                .count();
            rows.push(SidebarRow {
                row: row.clone().upcast(),
                page: page.clone().upcast(),
                title: title.to_owned(),
                section,
            });
            position
        };
        self.list
            .insert(&row, i32::try_from(position).unwrap_or(-1));
        row
    }

    fn remove(&self, row: &impl IsA<gtk::ListBoxRow>) {
        let row = row.as_ref();
        let Some(index) = self.position(row) else {
            return;
        };
        let removed = self.rows.borrow_mut().remove(index);
        self.list.remove(row);
        self.pages.remove(&removed.page);
    }

    fn answer(&self, find: impl Fn(Target) -> Option<Answer> + 'static) {
        self.finds.borrow_mut().push(Box::new(find));
    }

    /// Feeds sidebar rows for as long as the window is on screen,
    /// whichever page is selected.
    fn follow(&self, feed: &Rc<Feed>, request: Request, show: impl Fn(&Reading) + 'static) {
        if let Some(split) = self.split.upgrade() {
            show_while_mapped(feed, &split, request, show);
        }
    }

    /// No target leaves the selection standing, and replaces any target still
    /// pending: the latest open is the one the reader meant.
    fn select(&self, target: Option<Target>) {
        self.pending.set(target);
        self.settle();
    }

    /// A target no row answers for yet stays pending, and the first row stands
    /// in for it until a module adds the one it names.
    fn settle(&self) {
        if !self.filled.get() {
            return;
        }
        if let Some(answer) = self.pending.get().and_then(|target| self.find(target)) {
            self.pending.set(None);
            self.list.select_row(Some(&answer.row));
            if let Some(then) = answer.then {
                then();
            }
            self.reveal();
            return;
        }
        if self.list.selected_row().is_none() {
            self.list.select_row(self.list.row_at_index(0).as_ref());
        }
    }

    fn find(&self, target: Target) -> Option<Answer> {
        self.finds.borrow().iter().find_map(|find| find(target))
    }

    /// Showing the content is for a row the reader chose or a target named
    /// one, not for the first row a fill falls back on.
    fn reveal(&self) {
        if let Some(split) = self.split.upgrade() {
            split.set_show_content(true);
        }
    }

    fn show(&self, row: &gtk::ListBoxRow) {
        let Some(index) = self.position(row) else {
            return;
        };
        let (page, title, section) = {
            let rows = self.rows.borrow();
            let listed = &rows[index];
            (listed.page.clone(), listed.title.clone(), listed.section)
        };
        self.pages.set_visible_child(&page);
        self.titled.set_title(&title);
        self.heading.set_title(&self.sections.borrow()[section.0]);
        self.heading.set_subtitle(&title);
    }
}
