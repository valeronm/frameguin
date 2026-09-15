//! The status window: what the hardware is doing now, as sections of rows in
//! a sidebar and the selected row's page beside them.
//!
//! Nothing here writes, so none of [`crate::window`]'s machinery applies — no
//! sync guard, no debounce, no tray push.

mod battery;
mod battery_extender;
mod chassis;
mod ports;
mod privacy_switches;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use frameguin_wire::BatteryFeature;
use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;

use super::{Shell, collapse_when_narrow, headed};
use crate::daemon::Daemon;
use crate::reading::{Feed, Reading, Wants, show_while_mapped};

/// The only way the window is opened.
pub(crate) const ACTION: &str = "status";

const TITLE: &str = "Status";

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
            let window = super::open(app, ACTION, TITLE, (760, 680), &fill);
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
    collapse_when_narrow(window, &split);

    let select = gio::SimpleAction::new(SELECT, Some(glib::VariantTy::STRING));
    let selecting = sidebar.clone();
    select.connect_activate(move |_, parameter| selecting.select(targeted(parameter)));
    let actions = gio::SimpleActionGroup::new();
    actions.add_action(&select);
    window.insert_action_group(ACTION, Some(&actions));

    let daemon = daemon.clone();
    let feed = feed.clone();
    glib::spawn_future_local(async move {
        let controls = match daemon.controls().await {
            Ok(controls) => controls,
            Err(e) => {
                shell.toast_error("Reading the status", e);
                return;
            }
        };
        // A window closed while the daemon was being dialled has nothing left
        // to draw into.
        if sidebar
            .split
            .upgrade()
            .is_none_or(|split| split.root().is_none())
        {
            return;
        }
        if let Some(control) = &controls.battery {
            battery::add(&sidebar, &feed, control);
            if control.has(BatteryFeature::Extender) {
                battery_extender::add(&sidebar, &feed);
            }
        }
        if let Some(control) = &controls.chassis {
            chassis::add(&sidebar, &feed, control);
        }
        if controls.privacy_switches.is_some() {
            privacy_switches::add(&sidebar, &feed);
        }
        if controls.ports.is_some() {
            ports::add(&sidebar, &feed);
        }
        if sidebar.lists.borrow().is_empty() {
            sidebar.pages.add_child(
                &adw::StatusPage::builder()
                    .icon_name("dialog-information-symbolic")
                    .title("No status to show")
                    .description("The daemon found nothing on this machine whose state it reads.")
                    .build(),
            );
        }
        if let Err(e) = feed.read().await {
            shell.toast_error("Reading the status", e);
        }
        sidebar.filled.set(true);
        sidebar.settle();
    });
    split
}

fn pick(row: &gtk::ListBoxRow) {
    if let Some(list) = row.parent().and_downcast::<gtk::ListBox>() {
        list.select_row(Some(row));
    }
}

struct SidebarRow {
    row: gtk::ListBoxRow,
    page: gtk::Widget,
    title: String,
}

type Find = dyn Fn(Target) -> Option<gtk::ListBoxRow>;

/// Holds the split view weakly, the sections' subscriptions hanging on it and
/// holding this. Everything else here is a descendant of it.
struct Sidebar {
    split: glib::WeakRef<adw::NavigationSplitView>,
    sections: gtk::Box,
    lists: RefCell<Vec<gtk::ListBox>>,
    pages: gtk::Stack,
    titled: adw::NavigationPage,
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
        let sections = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let listed = headed(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&sections)
                .build(),
        );
        let pages = gtk::Stack::new();
        // A homogeneous stack measures every page on every layout pass, where
        // only the page on screen has anything to say about the pane's size.
        pages.set_hhomogeneous(false);
        pages.set_vhomogeneous(false);
        let titled = adw::NavigationPage::builder()
            .title(TITLE)
            .child(&headed(&pages))
            .build();
        let sidebar = adw::NavigationPage::builder()
            .title(TITLE)
            .child(&listed)
            .build();
        // Wide enough for a port's summary on one line, which at the default
        // quarter of the window wraps its wattage.
        let split = adw::NavigationSplitView::builder()
            .sidebar(&sidebar)
            .content(&titled)
            .min_sidebar_width(260.0)
            .build();
        let this = Rc::new(Self {
            split: split.downgrade(),
            sections,
            lists: RefCell::default(),
            pages,
            titled,
            rows: RefCell::default(),
            finds: RefCell::default(),
            pending: Cell::default(),
            filled: Cell::default(),
        });
        (this, split)
    }

    /// A section's list, under `heading` where the section has more than one
    /// row to gather. Both stay hidden until the section adds a row.
    fn section(self: &Rc<Self>, heading: Option<&str>) -> gtk::ListBox {
        let list = gtk::ListBox::new();
        list.add_css_class("navigation-sidebar");
        list.set_visible(false);
        if let Some(heading) = heading {
            let label = gtk::Label::builder()
                .label(heading)
                .xalign(0.0)
                .margin_start(12)
                .margin_top(12)
                .margin_bottom(6)
                .css_classes(["heading"])
                .build();
            list.bind_property("visible", &label, "visible")
                .sync_create()
                .build();
            self.sections.append(&label);
        }
        self.sections.append(&list);
        // Weak: the list is a descendant of what this holds.
        let showing = Rc::downgrade(self);
        list.connect_row_selected(move |list, row| {
            if let (Some(sidebar), Some(row)) = (showing.upgrade(), row) {
                sidebar.show(list, row);
            }
        });
        let revealing = Rc::downgrade(self);
        list.connect_row_activated(move |_, _| {
            if let Some(sidebar) = revealing.upgrade() {
                // A row the reader picked outranks a target still waiting on
                // its reading.
                sidebar.pending.set(None);
                sidebar.reveal();
            }
        });
        self.lists.borrow_mut().push(list.clone());
        list
    }

    fn add(
        &self,
        list: &gtk::ListBox,
        title: &str,
        page: &impl IsA<gtk::Widget>,
    ) -> adw::ActionRow {
        let row = adw::ActionRow::builder().title(title).build();
        list.append(&row);
        list.set_visible(true);
        self.pages.add_child(page);
        self.rows.borrow_mut().push(SidebarRow {
            row: row.clone().upcast(),
            page: page.clone().upcast(),
            title: title.to_owned(),
        });
        row
    }

    fn remove(&self, row: &impl IsA<gtk::ListBoxRow>) {
        let row = row.as_ref();
        let Some(index) = self
            .rows
            .borrow()
            .iter()
            .position(|listed| listed.row == *row)
        else {
            return;
        };
        let removed = self.rows.borrow_mut().remove(index);
        if let Some(list) = row.parent().and_downcast::<gtk::ListBox>() {
            list.remove(row);
            list.set_visible(list.row_at_index(0).is_some());
        }
        self.pages.remove(&removed.page);
    }

    fn answer(&self, find: impl Fn(Target) -> Option<gtk::ListBoxRow> + 'static) {
        self.finds.borrow_mut().push(Box::new(find));
    }

    /// Feeds a section's rows for as long as the window is on screen,
    /// whichever page is selected.
    fn follow(&self, feed: &Rc<Feed>, wants: Wants, show: impl Fn(&Reading) + 'static) {
        if let Some(split) = self.split.upgrade() {
            show_while_mapped(feed, &split, wants, show);
        }
    }

    /// No target leaves the selection standing, and replaces any target still
    /// pending: the latest open is the one the reader meant.
    fn select(&self, target: Option<Target>) {
        self.pending.set(target);
        self.settle();
    }

    /// A target no row answers for yet stays pending, and the first row stands
    /// in for it until a section adds the one it names.
    fn settle(&self) {
        if !self.filled.get() {
            return;
        }
        if let Some(row) = self.pending.get().and_then(|target| self.find(target)) {
            self.pending.set(None);
            pick(&row);
            self.reveal();
            return;
        }
        let selected = self
            .rows
            .borrow()
            .iter()
            .any(|listed| listed.row.is_selected());
        if !selected {
            let first = self
                .lists
                .borrow()
                .iter()
                .find_map(|list| list.row_at_index(0));
            if let Some(first) = first {
                pick(&first);
            }
        }
    }

    fn find(&self, target: Target) -> Option<gtk::ListBoxRow> {
        self.finds.borrow().iter().find_map(|find| find(target))
    }

    /// Showing the content is for a row the reader chose or a target named
    /// one, not for the first row a fill falls back on.
    fn reveal(&self) {
        if let Some(split) = self.split.upgrade() {
            split.set_show_content(true);
        }
    }

    fn show(&self, list: &gtk::ListBox, row: &gtk::ListBoxRow) {
        let others: Vec<_> = self
            .lists
            .borrow()
            .iter()
            .filter(|other| *other != list)
            .cloned()
            .collect();
        for other in others {
            other.unselect_all();
        }
        let shown = self
            .rows
            .borrow()
            .iter()
            .find(|listed| listed.row == *row)
            .map(|listed| (listed.page.clone(), listed.title.clone()));
        if let Some((page, title)) = shown {
            self.pages.set_visible_child(&page);
            self.titled.set_title(&title);
        }
    }
}
