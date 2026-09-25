//! The USB-C ports: a row per port carrying what is plugged into it,
//! and a page per port with the rest of what the EC's copy of its
//! controller's state says and what the controller's own registers add.
//!
//! A page is laid out as titled rows before any widget is built and redrawn
//! whole only when the titles change, its rows coming and going with what is
//! attached; a reading that keeps them sets the values in place. It keeps
//! the last registers it read while the same kind of partner stays attached,
//! for a reading that did not ask for them or failed to read them.
//!
//! What each value is *called* is `frameguin_model::control::ports`'s, and
//! where a socket is on the machine is `frameguin_model::port`'s — which
//! answers for the boards it has been measured on and no others, asking for
//! nothing on one nobody measured.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use frameguin_contract::{Attached, PortPartner, PortRegisters, PortSet, PortState, PowerRole};
use frameguin_model::control::ports::{
    NOTHING_ATTACHED, POWERING_THE_MACHINE, cable_length_label, cable_rating_label,
    cable_speed_label, cable_type_label, carried, contract_label, data_role_label,
    display_port_label, epr_label, measured_label, mismatch_label, partner_label, peak_label,
    port_summary, power_limited_label, power_role_label, powering_label, supply_kind_label,
    vconn_label,
};
use frameguin_model::control::usb::{capacity_label, device_name, network_label, speed_label};
use frameguin_model::port::Placement;
use frameguin_model::reading::Request;
use gtk4 as gtk;
use gtk4::glib;

use super::{Sidebar, Target};
use crate::reading::{Feed, show_while_mapped};
use crate::report::{described_value, value};

/// The section's rows, one per port the last reading carried.
struct Section {
    list: gtk::ListBox,
    ports: RefCell<Vec<Port>>,
    placement: Placement,
}

struct Port {
    index: u8,
    row: adw::ActionRow,
    charging: Cell<bool>,
}

/// A port's page, fed only while it is on screen.
struct PortPage {
    index: u8,
    /// Weak: the page's own subscription holds this.
    page: glib::WeakRef<adw::PreferencesPage>,
    placement: Placement,
    /// The last registers read while the same kind of partner stayed
    /// attached, standing in for a reading that did not ask for them or
    /// failed to read them.
    registers: Cell<Option<(PortPartner, PortRegisters)>>,
    drawn: RefCell<Option<Drawn>>,
}

struct Drawn {
    groups: Vec<adw::PreferencesGroup>,
    /// One per row, in the order `layout` lists them.
    values: Vec<gtk::Label>,
    layout: Vec<Group>,
}

/// A group as the page lays it out, before any widget is built: a reading
/// that keeps every title moves only the values, which are set in place.
struct Group {
    title: String,
    rows: Vec<Row>,
    /// One per row.
    values: Vec<String>,
}

#[derive(PartialEq)]
struct Row {
    title: &'static str,
    subtitle: Option<&'static str>,
}

impl Group {
    fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            rows: Vec::new(),
            values: Vec::new(),
        }
    }

    fn row(&mut self, title: &'static str, value: impl Into<String>) {
        self.push(title, None, value.into());
    }

    fn described(&mut self, title: &'static str, subtitle: &'static str, value: impl Into<String>) {
        self.push(title, Some(subtitle), value.into());
    }

    fn push(&mut self, title: &'static str, subtitle: Option<&'static str>, value: String) {
        self.rows.push(Row { title, subtitle });
        self.values.push(value);
    }

    fn shape(&self) -> (&str, &[Row]) {
        (&self.title, &self.rows)
    }
}

/// The rows arrive with the first reading, which is what says how many ports
/// there are.
pub(super) fn add(sidebar: &Rc<Sidebar>, feed: &Rc<Feed>, usb: bool, placement: Placement) {
    let request = Request {
        ports: true,
        usb: usb && placement.wired(),
        ..Request::default()
    };
    let section = Rc::new(Section {
        list: sidebar.section("USB-C Ports"),
        ports: RefCell::default(),
        placement,
    });

    let answering = Rc::downgrade(&section);
    sidebar.answer(move |target| {
        (target == Target::Charger)
            .then(|| answering.upgrade())
            .flatten()
            .and_then(|section| section.charger())
    });

    let showing = sidebar.clone();
    // Weak: this closure is the feed's own subscription.
    let asking = Rc::downgrade(feed);
    sidebar.follow(feed, request, move |reading| {
        if let Some(ports) = &reading.ports {
            let devices = reading.usb.as_deref().unwrap_or_default();
            section.show(&showing, &asking, request, ports, devices);
        }
    });
}

impl Section {
    /// The rows are rebuilt only where the set of ports changed, which on a
    /// board's fixed ports is the first reading alone.
    fn show(
        &self,
        sidebar: &Sidebar,
        feed: &Weak<Feed>,
        request: Request,
        ports: &[PortState],
        devices: &[Attached],
    ) {
        let placement = self.placement;
        let mut ports: Vec<&PortState> = ports.iter().collect();
        ports.sort_by_key(|state| placement.order(state.index));
        let same = self
            .ports
            .borrow()
            .iter()
            .map(|port| port.index)
            .eq(ports.iter().map(|state| state.index));
        if !same {
            let gone: Vec<Port> = self.ports.borrow_mut().drain(..).collect();
            for port in gone {
                sidebar.remove(&port.row);
            }
            let feed = feed.upgrade();
            let built = ports
                .iter()
                .map(|state| {
                    let widget = adw::PreferencesPage::new();
                    let page = PortPage {
                        index: state.index,
                        page: widget.downgrade(),
                        placement,
                        registers: Cell::default(),
                        drawn: RefCell::default(),
                    };
                    let row = sidebar.add(&self.list, &placement.label(state.index), &widget);
                    row.set_use_markup(false);
                    if let Some(feed) = &feed {
                        let request = Request {
                            controller_ports: PortSet::of(state.index),
                            ..request
                        };
                        show_while_mapped(feed, &widget, request, move |reading| {
                            if let Some(ports) = &reading.ports {
                                let devices = reading.usb.as_deref().unwrap_or_default();
                                page.show(ports, devices);
                            }
                        });
                    }
                    Port {
                        index: state.index,
                        row,
                        charging: Cell::default(),
                    }
                })
                .collect();
            *self.ports.borrow_mut() = built;
        }
        for (port, state) in self.ports.borrow().iter().zip(&ports) {
            let name = placement
                .attached(state.index, devices)
                .first()
                .map(|device| device_name(device));
            port.row.set_subtitle(&port_summary(state, name.as_deref()));
            port.charging.set(state.charging);
        }
        if !same {
            sidebar.settle();
        }
    }

    fn charger(&self) -> Option<gtk::ListBoxRow> {
        let ports = self.ports.borrow();
        ports
            .iter()
            .find(|port| port.charging.get())
            .or_else(|| ports.first())
            .map(|port| port.row.clone().upcast())
    }
}

impl PortPage {
    fn show(&self, ports: &[PortState], devices: &[Attached]) {
        let Some(page) = self.page.upgrade() else {
            return;
        };
        let Some(state) = ports.iter().find(|state| state.index == self.index) else {
            return;
        };
        let mut state = state.clone();
        match state.registers {
            Some(registers) => self.registers.set(Some((state.partner, registers))),
            None if state.partner == PortPartner::Nothing => self.registers.set(None),
            None => {
                state.registers = self
                    .registers
                    .get()
                    .filter(|&(partner, _)| partner == state.partner)
                    .map(|(_, registers)| registers);
            }
        }
        let devices = self.placement.attached(self.index, devices);
        let mut layout = vec![connection_group(self.placement, &state)];
        layout.extend(cable_group(&state));
        layout.extend(contract_group(&state));
        layout.extend(devices.into_iter().map(device_group));
        let mut drawn = self.drawn.borrow_mut();
        if let Some(shown) = drawn.as_mut()
            && shown
                .layout
                .iter()
                .map(Group::shape)
                .eq(layout.iter().map(Group::shape))
        {
            let old = shown.layout.iter().flat_map(|group| &group.values);
            let fresh = layout.iter().flat_map(|group| &group.values);
            for ((label, old), new) in shown.values.iter().zip(old).zip(fresh) {
                if old != new {
                    label.set_label(new);
                }
            }
            shown.layout = layout;
            return;
        }
        if let Some(shown) = drawn.take() {
            for group in shown.groups {
                page.remove(&group);
            }
        }
        let mut groups = Vec::new();
        let mut values = Vec::new();
        for laid in &layout {
            let group = adw::PreferencesGroup::builder().title(&laid.title).build();
            for (row, text) in laid.rows.iter().zip(&laid.values) {
                let label = match row.subtitle {
                    Some(subtitle) => described_value(&group, row.title, subtitle),
                    None => value(&group, row.title),
                };
                label.set_label(text);
                values.push(label);
            }
            page.add(&group);
            groups.push(group);
        }
        *drawn = Some(Drawn {
            groups,
            values,
            layout,
        });
    }
}

fn connection_group(placement: Placement, state: &PortState) -> Group {
    let mut group = Group::new(partner_label(state.partner).unwrap_or(NOTHING_ATTACHED));
    if let Some(number) = placement.secondary(state.index) {
        group.row("Port", number);
    }
    if state.partner == PortPartner::Nothing {
        return group;
    }
    if let Some(powering) = powering_label(state) {
        group.row(POWERING_THE_MACHINE, powering);
    }
    if let Some(video) = display_port_label(state) {
        group.row("DisplayPort", video);
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
    group
}

fn cable_group(state: &PortState) -> Option<Group> {
    let cable = state.registers?.cable?;
    let mut group = Group::new("Cable");
    if let Some(kind) = cable_type_label(cable.active) {
        group.row("Type", kind);
    }
    if let Some(speed) = cable.speed {
        group.row("Speed", cable_speed_label(speed));
    }
    if let Some(rating) = cable_rating_label(&cable) {
        group.row("Rating", rating);
    }
    if let Some(latency) = cable.latency {
        group.row("Length", cable_length_label(latency));
    }
    Some(group)
}

fn contract_group(state: &PortState) -> Option<Group> {
    if state.partner == PortPartner::Nothing {
        return None;
    }
    let mut group = Group::new(contract_label(state.contract));
    if let Some(supply) = carried(state) {
        group.described("Contract", contract_ceiling(state.power_role), supply);
    }
    if let Some(registers) = state.registers {
        group.row("Measured", measured_label(registers.measured_millivolts));
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
    if let Some(epr) = epr_label(state.epr) {
        group.row("Extended power range", epr);
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
