//! The USB-C ports: one row carrying where the machine is powered from, and
//! a page picking one port among them, with the rest of what the EC's copy
//! of that port's controller's state says and what the controller's own
//! registers add, headed by a switch where the controller can turn the port
//! off.
//!
//! The picked port's groups are laid out as titled rows before any widget is
//! built and redrawn whole only when the titles change, its rows coming and
//! going with what is attached; a reading that keeps them sets the values in
//! place.
//!
//! What each value is *called* is `frameguin_modelview::ports`'s. Where a
//! socket is on the machine is `frameguin_model::port`'s, which answers only
//! for the boards it has been measured on; what that place is called is
//! `frameguin_modelview::port`'s.

mod picker;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use frameguin_contract::{Attached, PortMask, PortReading, PortState, PowerRole};
use frameguin_model::control::ports::{Ports, attached, bus_voltage, powering};
use frameguin_model::port::Placement;
use frameguin_model::reading::{Reading, Request};
use frameguin_modelview::port;
use frameguin_modelview::ports::{
    DISPLAY_PORT, bus_track, cable_label, cable_type_label, carried, contract_label,
    data_role_label, display_port_label, measured_label, mismatch_label, out_of_range_label,
    peak_label, power_limited_label, power_role_label, powered_from, refusal_label,
    supply_kind_label, vconn_label,
};
use frameguin_modelview::track::Track;
use frameguin_modelview::usb::{capacity_label, device_name, network_label, speed_label};
use frameguin_wire::Bus;
use gtk4 as gtk;
use gtk4::glib;

use self::picker::Picker;
use super::track::{TrackRow, TrackView};
use super::{Answer, Sidebar, Target, Writes};
use crate::reading::{Feed, show_while_mapped};
use crate::report::value_row;
use crate::synced::{Guard, connect_switch, show_switch};

const PORTS: &str = "Ports";

const STAYS_OFF: &str = "Stays off across restarts until turned back on";

/// The narrowest the page keeps [`STAYS_OFF`] on one line.
pub(super) const CONTENT_WIDTH: i32 = 420;

const PARTNER_DETECTED: Duration = Duration::from_millis(500);

struct Section {
    heading: super::Section,
    /// None until a reading carries a port.
    shown: RefCell<Option<Shown>>,
    control: Rc<Ports<Bus>>,
    writes: Rc<Writes>,
}

struct Shown {
    row: adw::ActionRow,
    page: Rc<PortsPage>,
    /// In the order `Placement::order` puts them.
    indices: Vec<u8>,
    powering: Cell<Option<u8>>,
}

/// Fed only while it is on screen.
struct PortsPage {
    /// Weak: the page's own subscription holds this.
    page: glib::WeakRef<adw::PreferencesPage>,
    placement: Placement,
    picker: Picker,
    picked: Cell<u8>,
    /// Beside the first group's title while its port is picked, moved onto
    /// the new one on a redraw.
    switches: Vec<(u8, gtk::Switch)>,
    guard: Guard,
    drawn: RefCell<Option<Drawn>>,
    /// Weak: the feed holds the page's subscription, which holds this.
    feed: Weak<Feed>,
    request: Request,
}

struct Drawn {
    groups: Vec<adw::PreferencesGroup>,
    /// In the order `layout` lists them.
    rows: Vec<DrawnRow>,
    layout: Vec<Group>,
}

struct DrawnRow {
    line: Line,
    value: gtk::Label,
    /// Hidden while the row's value carries no warning.
    warning: gtk::Image,
}

/// The widget that shows a row's [`Beneath`].
enum Line {
    Plain(adw::ActionRow),
    Tracked(TrackView),
}

/// A group as the page lays it out, before any widget is built: a reading
/// that keeps every title moves only the values, which are set in place.
struct Group {
    title: String,
    rows: Vec<(&'static str, Value)>,
}

#[derive(Default)]
struct Value {
    text: String,
    /// What the warning icon says, and None where it is hidden.
    warning: Option<&'static str>,
    beneath: Beneath,
}

/// What sits under a row's title and value. A row with a track is a widget of
/// another kind, holding the track whether or not this reading draws one.
#[derive(PartialEq)]
enum Beneath {
    /// Hidden where empty.
    Subtitle(String),
    Track(Option<Track>),
}

impl Default for Beneath {
    fn default() -> Self {
        Self::Subtitle(String::new())
    }
}

impl Value {
    fn tracked(&self) -> bool {
        matches!(self.beneath, Beneath::Track(_))
    }
}

impl Group {
    fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            rows: Vec::new(),
        }
    }

    fn row(&mut self, title: &'static str, text: impl Into<String>) {
        self.described(title, None, text);
    }

    fn described(
        &mut self,
        title: &'static str,
        subtitle: impl Into<Option<&'static str>>,
        text: impl Into<String>,
    ) {
        self.push(
            title,
            Value {
                text: text.into(),
                beneath: Beneath::Subtitle(subtitle.into().unwrap_or_default().to_owned()),
                ..Value::default()
            },
        );
    }

    fn push(&mut self, title: &'static str, value: Value) {
        self.rows.push((title, value));
    }

    fn values(&self) -> impl Iterator<Item = &Value> {
        self.rows.iter().map(|(_, value)| value)
    }

    fn same_shape(&self, other: &Self) -> bool {
        let shape = |(title, value): &(&'static str, Value)| (*title, value.tracked());
        self.title == other.title && self.rows.iter().map(shape).eq(other.rows.iter().map(shape))
    }
}

/// Whether `fresh` keeps every group and row title of `drawn`, and which
/// rows have a track, its values aside.
fn same_layout(drawn: &[Group], fresh: &[Group]) -> bool {
    drawn.len() == fresh.len()
        && drawn
            .iter()
            .zip(fresh)
            .all(|(drawn, fresh)| drawn.same_shape(fresh))
}

/// The row arrives with the first reading that carries a port.
pub(super) fn add(
    sidebar: &Rc<Sidebar>,
    feed: &Rc<Feed>,
    usb: bool,
    control: &Rc<Ports<Bus>>,
    writes: &Rc<Writes>,
) {
    let placement = control.placement();
    let summary = Request {
        ports: true,
        ..Request::default()
    };
    let page = Request {
        usb: usb && placement.wired(),
        ..summary
    };
    let section = Rc::new(Section {
        heading: sidebar.section("USB-C Ports"),
        shown: RefCell::default(),
        control: control.clone(),
        writes: writes.clone(),
    });

    let answering = Rc::downgrade(&section);
    sidebar.answer(move |target| {
        if target != Target::Charger {
            return None;
        }
        answering.upgrade()?.charger()
    });

    let showing = sidebar.clone();
    // Weak: this closure is the feed's own subscription.
    let asking = Rc::downgrade(feed);
    sidebar.follow(feed, summary, move |reading| {
        if let Some(ports) = &reading.ports {
            section.show(&showing, &asking, page, ports);
        }
    });
}

impl Section {
    /// The page is rebuilt only where the set of ports changed, which on a
    /// board's fixed ports is the first reading alone.
    fn show(&self, sidebar: &Sidebar, feed: &Weak<Feed>, request: Request, ports: &[PortState]) {
        let placement = self.control.placement();
        let powering = powering(ports).map(|(index, _)| index);
        let mut indices: Vec<u8> = ports.iter().map(|state| state.index).collect();
        indices.sort_by_key(|&index| placement.order(index));
        let same = self
            .shown
            .borrow()
            .as_ref()
            .map_or(indices.is_empty(), |shown| shown.indices == indices);
        if !same {
            let kept = self.shown.take().map(|shown| {
                sidebar.remove(&shown.row);
                shown.page.picked.get()
            });
            let picked = kept
                .filter(|index| indices.contains(index))
                .or_else(|| preferred(powering, &indices));
            *self.shown.borrow_mut() =
                picked.map(|picked| self.build(sidebar, feed, request, indices, picked));
        }
        if let Some(shown) = &*self.shown.borrow()
            && shown.powering.replace(powering) != powering
        {
            let subtitle = powering.map(|index| powered_from(placement, index));
            shown.row.set_subtitle(&subtitle.unwrap_or_default());
        }
        if !same {
            sidebar.settle();
        }
    }

    fn build(
        &self,
        sidebar: &Sidebar,
        feed: &Weak<Feed>,
        request: Request,
        indices: Vec<u8>,
        initial: u8,
    ) -> Shown {
        let placement = self.control.placement();
        let widget = adw::PreferencesPage::new();
        let picker = Picker::new(placement, &indices);
        widget.add(picker.group());
        let switchable = self.control.switchable();
        let switches = indices
            .iter()
            .filter(|&&index| switchable.contains(index))
            .map(|&index| {
                let switch = gtk::Switch::builder()
                    .valign(gtk::Align::Center)
                    .sensitive(false)
                    .build();
                switch.update_property(&[gtk::accessible::Property::Label("Enabled")]);
                self.connect(index, &switch, feed);
                (index, switch)
            })
            .collect();
        let page = Rc::new(PortsPage {
            page: widget.downgrade(),
            placement,
            picker,
            picked: Cell::new(initial),
            switches,
            guard: self.writes.guard.clone(),
            drawn: RefCell::default(),
            feed: feed.clone(),
            request,
        });
        // Selected before the handler is connected, so nothing is drawn
        // until the page is mapped.
        page.picker.select(initial);
        let picking = Rc::downgrade(&page);
        page.picker.connect_picked(move |index| {
            if let Some(page) = picking.upgrade() {
                page.pick(index);
            }
        });
        let row = sidebar.add(self.heading, PORTS, &widget);
        if let Some(feed) = &feed.upgrade() {
            let showing = page.clone();
            show_while_mapped(feed, &widget, request, move |reading| {
                if let Some((ports, devices)) = read(reading) {
                    showing.show(ports, devices);
                }
            });
        }
        Shown {
            row,
            page,
            indices,
            powering: Cell::default(),
        }
    }

    /// A refused write puts the switch back: a port shown in a state it is
    /// not in is one someone plugs into or leaves alone on its word. Every
    /// write is followed by a reading, since the lock moves with the port
    /// switched.
    fn connect(&self, index: u8, switch: &gtk::Switch, feed: &Weak<Feed>) {
        let (switched, feed) = (switch.downgrade(), feed.clone());
        connect_switch(
            &self.writes.guard,
            &self.writes,
            &self.control,
            switch,
            move |writes, control, enabled| {
                let (switched, feed) = (switched.clone(), feed.clone());
                async move {
                    let Some(feed) = feed.upgrade() else {
                        return;
                    };
                    let written = {
                        let _writing = feed.writing();
                        control.set_enabled(index, enabled).await
                    };
                    let turned_on = enabled && written.is_ok();
                    if let Err(e) = written {
                        writes.toast_error("Switching a USB-C port", e);
                        if let Some(switch) = switched.upgrade() {
                            show_switch(&writes.guard, &switch, !enabled);
                        }
                    }
                    // Its own failure is the next tick's to report.
                    let refill = async || {
                        if let Some(switch) = switched.upgrade() {
                            let _ = feed.fill(&switch).await;
                        }
                    };
                    refill().await;
                    if turned_on {
                        // A port turned on reads nothing attached until its
                        // controller has detected the partner.
                        glib::timeout_future(PARTNER_DETECTED).await;
                        refill().await;
                    }
                }
            },
        );
    }

    /// The row, with the port the machine is powered through picked on its
    /// page.
    fn charger(&self) -> Option<Answer> {
        let shown = self.shown.borrow();
        let shown = shown.as_ref()?;
        let index = preferred(shown.powering.get(), &shown.indices)?;
        let page = shown.page.clone();
        Some(Answer {
            row: shown.row.clone().upcast(),
            then: Some(Box::new(move || page.picker.select(index))),
        })
    }
}

/// The ports a reading carries and the devices on them, and None where it
/// carries no ports.
fn read(reading: &Reading) -> Option<(&[PortState], &[Attached])> {
    Some((
        reading.ports.as_deref()?,
        reading.usb.as_deref().unwrap_or_default(),
    ))
}

fn preferred(powering: Option<u8>, indices: &[u8]) -> Option<u8> {
    powering.or_else(|| indices.first().copied())
}

impl PortsPage {
    fn show(&self, ports: &[PortState], devices: &[Attached]) {
        self.picker.show(ports);
        self.draw(ports, devices);
    }

    /// A pick between a write and the reading after it draws nothing until
    /// that reading.
    fn pick(&self, index: u8) {
        self.picked.set(index);
        self.clear();
        let held = self.feed.upgrade().and_then(|feed| feed.held(self.request));
        if let Some((ports, devices)) = held.as_deref().and_then(read) {
            self.draw(ports, devices);
        }
    }

    fn switch(&self) -> Option<&gtk::Switch> {
        let picked = self.picked.get();
        self.switches
            .iter()
            .find(|(index, _)| *index == picked)
            .map(|(_, switch)| switch)
    }

    fn clear(&self) {
        let Some(drawn) = self.drawn.take() else {
            return;
        };
        if let Some(first) = drawn.groups.first() {
            first.set_header_suffix(None::<&gtk::Widget>);
        }
        if let Some(page) = self.page.upgrade() {
            for group in drawn.groups {
                page.remove(&group);
            }
        }
    }

    fn draw(&self, ports: &[PortState], devices: &[Attached]) {
        let Some(page) = self.page.upgrade() else {
            return;
        };
        let index = self.picked.get();
        let Some(state) = ports.iter().find(|state| state.index == index) else {
            return;
        };
        let layout = match &state.mask {
            PortMask::Off => vec![off_group(self.placement, index)],
            PortMask::On(reading, _) | PortMask::Unchecked(reading, _) => {
                let devices = self.placement.attached(index, devices);
                let mut layout = vec![connection_group(self.placement, index, reading)];
                layout.extend(contract_group(reading));
                layout.extend(devices.into_iter().map(device_group));
                layout
            }
        };
        if let Some(drawn) = self.drawn.borrow_mut().as_mut()
            && same_layout(&drawn.layout, &layout)
        {
            let old = drawn.layout.iter().flat_map(Group::values);
            let fresh = layout.iter().flat_map(Group::values);
            for ((row, old), new) in drawn.rows.iter().zip(old).zip(fresh) {
                row.show(old, new);
            }
            drawn.layout = layout;
            self.show_mask(state, &drawn.groups);
            return;
        }
        self.clear();
        let mut groups = Vec::new();
        let mut rows = Vec::new();
        for laid in &layout {
            let group = adw::PreferencesGroup::builder().title(&laid.title).build();
            for (title, value) in &laid.rows {
                let row = DrawnRow::new(&group, title, value.tracked());
                row.show(&Value::default(), value);
                rows.push(row);
            }
            page.add(&group);
            groups.push(group);
        }
        if let (Some(switch), Some(first)) = (self.switch(), groups.first()) {
            first.set_header_suffix(Some(switch));
        }
        self.show_mask(state, &groups);
        *self.drawn.borrow_mut() = Some(Drawn {
            groups,
            rows,
            layout,
        });
    }

    /// The first group's description is the switch's, since the switch has
    /// no row of its own to carry one.
    fn show_mask(&self, state: &PortState, groups: &[adw::PreferencesGroup]) {
        let (Some(switch), Some(first)) = (self.switch(), groups.first()) else {
            return;
        };
        let refusal = state.mask.refusal();
        first.set_description(Some(refusal.map_or(STAYS_OFF, refusal_label)));
        self.guard.sync(|| {
            switch.set_active(!matches!(state.mask, PortMask::Off));
            switch.set_sensitive(refusal.is_none());
        });
    }
}

impl DrawnRow {
    /// Its widgets show the default [`Value`]: empty, the icon and the track
    /// hidden.
    fn new(group: &adw::PreferencesGroup, title: &str, tracked: bool) -> Self {
        let warning = gtk::Image::from_icon_name("dialog-warning-symbolic");
        warning.add_css_class("warning");
        warning.set_visible(false);
        let (line, value) = if tracked {
            let row = TrackRow::new(group, title);
            row.add_suffix(&warning);
            (Line::Tracked(row.track), row.value)
        } else {
            let (row, value) = value_row(group, title);
            row.add_suffix(&warning);
            (Line::Plain(row), value)
        };
        Self {
            line,
            value,
            warning,
        }
    }

    fn show(&self, old: &Value, new: &Value) {
        if old.text != new.text {
            self.value.set_label(&new.text);
        }
        if old.warning != new.warning {
            self.show_warning(new.warning);
        }
        // Spares the clone of a track that `TrackView::show` would only
        // compare and drop.
        if old.beneath == new.beneath {
            return;
        }
        match (&self.line, &new.beneath) {
            (Line::Plain(row), Beneath::Subtitle(subtitle)) => row.set_subtitle(subtitle),
            (Line::Tracked(view), Beneath::Track(track)) => view.show(track.clone()),
            (Line::Plain(_), Beneath::Track(_)) | (Line::Tracked(_), Beneath::Subtitle(_)) => {
                unreachable!("a row is built for its layout's kind, which same_layout keeps")
            }
        }
    }

    /// The tooltip is out of reach of touch and keyboard.
    fn show_warning(&self, warning: Option<&str>) {
        self.warning.set_visible(warning.is_some());
        self.warning.set_tooltip_text(warning);
        self.warning
            .update_property(&[gtk::accessible::Property::Label(
                warning.unwrap_or_default(),
            )]);
    }
}

fn off_group(placement: Placement, index: u8) -> Group {
    Group::new(port::heading(placement, index))
}

fn connection_group(placement: Placement, index: u8, state: &PortReading) -> Group {
    let mut group = Group::new(port::heading(placement, index));
    if attached(state) {
        if let Some(video) = display_port_label(state) {
            group.row(DISPLAY_PORT, video);
        }
        if let Some(power) = power_role_label(state.partner, state.power_role) {
            group.row("Power role", power);
        }
        if let Some(vconn) = vconn_label(state.vconn) {
            group.described(
                "VCONN",
                "Power for the chips inside the cable or accessory",
                vconn,
            );
        }
        if let Some(data) = data_role_label(state.data_role) {
            group.row("Data role", data);
        }
    }
    if let Some(cable) = state.registers.and_then(|registers| registers.cable)
        && let Some(label) = cable_label(&cable)
    {
        group.described("Cable", cable_type_label(cable.active), label);
    }
    group
}

fn contract_group(state: &PortReading) -> Option<Group> {
    if !attached(state) {
        return None;
    }
    let mut group = Group::new(contract_label(state.contract));
    if let Some(supply) = carried(state) {
        group.described("Contract", contract_ceiling(state.power_role), supply);
    }
    if let Some(voltage) = bus_voltage(state) {
        group.push(
            "Measured",
            Value {
                text: measured_label(voltage),
                warning: out_of_range_label(voltage),
                beneath: Beneath::Track(bus_track(voltage)),
            },
        );
    }
    if let Some(pd) = state.registers.and_then(|registers| registers.pd) {
        if let Some(kind) = supply_kind_label(pd.kind) {
            group.row("Supply type", kind);
        }
        if let Some(mismatch) = mismatch_label(&pd) {
            group.described(
                "Capability mismatch",
                "The end drawing power wanted more than any offer",
                mismatch,
            );
        }
        if let Some(peak) = peak_label(&pd, state.partner) {
            group.row("Peak current", peak);
        }
        if let Some(limited) = power_limited_label(&pd, state.partner) {
            group.row("Power limited", limited);
        }
    }
    (!group.rows.is_empty()).then_some(group)
}

/// The power role is the machine's.
fn contract_ceiling(role: PowerRole) -> &'static str {
    match role {
        PowerRole::Sink => "The most the device can supply",
        PowerRole::Source => "The most the device can draw",
        PowerRole::Unknown => "The most the port can carry",
    }
}

fn device_group(device: &Attached) -> Group {
    let mut group = Group::new(device_name(device));
    if !device.manufacturer.is_empty() {
        group.row("Manufacturer", device.manufacturer.as_str());
    }
    group.row("Link speed", speed_label(device.speed));
    for link in &device.network {
        group.row("Network", network_label(link));
        group.row("Interface", link.interface.as_str());
        if !link.mac.is_empty() {
            group.row("MAC address", link.mac.as_str());
        }
    }
    for &bytes in &device.storage {
        group.row("Capacity", capacity_label(bytes));
    }
    if !device.firmware.is_empty() {
        group.row("Firmware", device.firmware.as_str());
    }
    group
}

#[cfg(test)]
mod tests {
    use frameguin_modelview::track::{Mark, Track};

    use super::{Beneath, Group, Value, same_layout};

    fn group(title: &str, rows: &[(&'static str, &str)]) -> Group {
        let mut group = Group::new(title);
        for (row, text) in rows {
            group.row(row, *text);
        }
        group
    }

    #[test]
    fn a_layout_keeping_every_title_is_the_same_whatever_its_values() {
        let drawn = [group("Port 1", &[("Contract", "100 W · 20 V")])];
        let fresh = [group("Port 1", &[("Contract", "60 W · 20 V")])];
        assert!(same_layout(&drawn, &fresh));
    }

    #[test]
    fn a_row_title_that_changes_makes_a_new_layout() {
        let drawn = [group("Port 1", &[("Contract", "100 W · 20 V")])];
        let fresh = [group("Port 1", &[("Measured", "20.1 V")])];
        assert!(!same_layout(&drawn, &fresh));
    }

    #[test]
    fn a_group_title_that_changes_makes_a_new_layout() {
        let drawn = [group("Power delivery", &[("Contract", "100 W · 20 V")])];
        let fresh = [group("Type-C only", &[("Contract", "100 W · 20 V")])];
        assert!(!same_layout(&drawn, &fresh));
    }

    #[test]
    fn rows_split_differently_between_groups_make_a_new_layout() {
        let drawn = [group("A", &[("One", "1"), ("Two", "2")]), group("B", &[])];
        let fresh = [group("A", &[("One", "1")]), group("B", &[("Two", "2")])];
        assert!(!same_layout(&drawn, &fresh));
    }

    #[test]
    fn a_row_gaining_a_track_makes_a_new_layout() {
        let drawn = [group("Port 1", &[("Measured", "19.9 V")])];
        let mut fresh = Group::new("Port 1");
        fresh.push(
            "Measured",
            Value {
                text: "19.9 V".to_owned(),
                beneath: Beneath::Track(None),
                ..Value::default()
            },
        );
        assert!(!same_layout(&drawn, &[fresh]));
    }

    #[test]
    fn a_track_row_keeps_its_layout_whether_or_not_it_draws_a_track() {
        let tracked = |track| {
            let mut group = Group::new("Port 1");
            group.push(
                "Measured",
                Value {
                    text: "19.9 V".to_owned(),
                    beneath: Beneath::Track(track),
                    ..Value::default()
                },
            );
            group
        };
        let track = Track {
            band: Some((250, 750)),
            marks: vec![(568, Mark::Target)],
            dot: Some(550),
            ticks: Vec::new(),
            description: String::new(),
        };
        assert!(same_layout(&[tracked(None)], &[tracked(Some(track))]));
    }

    #[test]
    fn a_group_added_makes_a_new_layout() {
        let drawn = [group("A", &[("One", "1")])];
        let fresh = [group("A", &[("One", "1")]), group("B", &[("Two", "2")])];
        assert!(!same_layout(&drawn, &fresh));
    }
}
