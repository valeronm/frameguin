//! The USB-C ports section: a row per port carrying what is plugged into it,
//! and a page per port with the rest of what the EC's copy of its
//! controller's state says.
//!
//! What each value is *called* is `frameguin_model::control::ports`'s, and
//! where a socket is on the machine is `frameguin_model::port`'s — which
//! answers for the boards it has been measured on and no others, asking for
//! nothing on one nobody measured.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::ports::{
    NO_E_MARKER, NOTHING_ATTACHED, POWERING_THE_MACHINE, cable_length_label, cable_rating_label,
    cable_speed_label, carried, contract_label, data_role_label, display_port_label, epr_label,
    partner_label, port_summary, power_role_label, powering_label,
};
use frameguin_model::control::usb::{capacity_label, device_name, network_label, speed_label};
use frameguin_model::port::Placement;
use frameguin_wire::{Attached, CableMarking, PortPartner, PortState};
use gtk4 as gtk;

use super::{Sidebar, Target};
use crate::reading::{Feed, Wants};
use crate::report::value;

/// The section's rows, one per port the last reading carried.
struct Section {
    list: gtk::ListBox,
    ports: RefCell<Vec<Port>>,
    placement: Placement,
}

struct Port {
    index: u8,
    row: adw::ActionRow,
    page: adw::PreferencesPage,
    drawn: RefCell<Option<Drawn>>,
}

struct Drawn {
    groups: Vec<adw::PreferencesGroup>,
    state: PortState,
    devices: Vec<Attached>,
}

/// The rows arrive with the first reading, which is what says how many ports
/// there are.
pub(super) fn add(sidebar: &Rc<Sidebar>, feed: &Rc<Feed>, usb: bool, placement: Placement) {
    let section = Rc::new(Section {
        list: sidebar.section(Some("USB-C Ports")),
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

    let wants = Wants {
        ports: true,
        cables: true,
        usb: usb && placement.wired(),
        ..Wants::default()
    };
    let showing = sidebar.clone();
    sidebar.follow(feed, wants, move |reading| {
        if let Some(ports) = &reading.ports {
            section.show(&showing, ports, reading.usb.as_deref().unwrap_or_default());
        }
    });
}

impl Section {
    /// The rows are rebuilt only where the set of ports changed, which on a
    /// board's fixed ports is the first reading alone.
    fn show(&self, sidebar: &Sidebar, ports: &[PortState], devices: &[Attached]) {
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
            let built = ports
                .iter()
                .map(|state| {
                    let page = adw::PreferencesPage::new();
                    let row = sidebar.add(&self.list, &placement.label(state.index), &page);
                    row.set_use_markup(false);
                    Port {
                        index: state.index,
                        row,
                        page,
                        drawn: RefCell::default(),
                    }
                })
                .collect();
            *self.ports.borrow_mut() = built;
        }
        for (port, state) in self.ports.borrow().iter().zip(&ports) {
            let here: Vec<Attached> = placement
                .attached(state.index, devices)
                .into_iter()
                .cloned()
                .collect();
            port.draw(placement, state, here);
        }
        if !same {
            sidebar.settle();
        }
    }

    fn charger(&self) -> Option<gtk::ListBoxRow> {
        let ports = self.ports.borrow();
        ports
            .iter()
            .find(|port| {
                port.drawn
                    .borrow()
                    .as_ref()
                    .is_some_and(|drawn| drawn.state.charging)
            })
            .or_else(|| ports.first())
            .map(|port| port.row.clone().upcast())
    }
}

impl Port {
    /// Redrawn whole: which rows a port has depends on what is plugged into
    /// it. Skipped where neither the state nor the devices moved.
    fn draw(&self, placement: Placement, state: &PortState, devices: Vec<Attached>) {
        let mut drawn = self.drawn.borrow_mut();
        if drawn
            .as_ref()
            .is_some_and(|shown| shown.state == *state && shown.devices == devices)
        {
            return;
        }
        if let Some(shown) = drawn.take() {
            for group in shown.groups {
                self.page.remove(&group);
            }
        }
        let name = devices.first().map(device_name);
        self.row.set_subtitle(&port_summary(state, name.as_deref()));
        let mut groups = vec![connection_group(placement, state)];
        groups.extend(cable_group(state));
        groups.extend(contract_group(state));
        groups.extend(devices.iter().map(device_group));
        for group in &groups {
            self.page.add(group);
        }
        *drawn = Some(Drawn {
            groups,
            state: state.clone(),
            devices,
        });
    }
}

fn connection_group(placement: Placement, state: &PortState) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title(partner_label(state.partner).unwrap_or(NOTHING_ATTACHED));
    if let Some(number) = placement.secondary(state.index) {
        value(&group, "Port").set_label(&number);
    }
    if state.partner == PortPartner::Nothing {
        return group;
    }
    if let Some(powering) = powering_label(state) {
        value(&group, POWERING_THE_MACHINE).set_label(powering);
    }
    if let Some(video) = display_port_label(state) {
        value(&group, "DisplayPort").set_label(video);
    }
    if let Some(power) = power_role_label(state.partner, state.power_role) {
        value(&group, "Power role").set_label(power);
    }
    value(&group, "Data role").set_label(data_role_label(state.data_role));
    group
}

/// None where nothing is attached or the cable could not be read, a guess
/// being worse than no group.
fn cable_group(state: &PortState) -> Option<adw::PreferencesGroup> {
    let cable = &state.cable;
    if state.partner == PortPartner::Nothing || cable.marking == CableMarking::Unknown {
        return None;
    }
    let group = adw::PreferencesGroup::new();
    group.set_title("Cable");
    if cable.marking == CableMarking::Unmarked {
        value(&group, "E-marker").set_label(NO_E_MARKER);
        return Some(group);
    }
    if let Some(speed) = cable_speed_label(cable.speed) {
        value(&group, "Speed").set_label(speed);
    }
    if let Some(rating) = cable_rating_label(cable) {
        value(&group, "Rating").set_label(&rating);
    }
    if let Some(length) = cable_length_label(cable.latency) {
        value(&group, "Length").set_label(length);
    }
    Some(group)
}

fn contract_group(state: &PortState) -> Option<adw::PreferencesGroup> {
    if state.partner == PortPartner::Nothing {
        return None;
    }
    let supply = carried(state);
    let epr = epr_label(state.epr);
    if supply.is_none() && epr.is_none() {
        return None;
    }
    let group = adw::PreferencesGroup::new();
    group.set_title(contract_label(state.contract));
    if let Some(supply) = supply {
        value(&group, "Supply").set_label(&supply);
    }
    if let Some(epr) = epr {
        value(&group, "Extended power range").set_label(epr);
    }
    Some(group)
}

fn device_group(device: &Attached) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(device_name(device))
        .build();
    if !device.manufacturer.is_empty() {
        value(&group, "Manufacturer").set_label(&device.manufacturer);
    }
    value(&group, "Link speed").set_label(speed_label(device.speed));
    for link in &device.network {
        value(&group, "Network").set_label(&network_label(link));
        value(&group, "Interface").set_label(&link.interface);
        if !link.mac.is_empty() {
            value(&group, "MAC address").set_label(&link.mac);
        }
    }
    for &bytes in &device.storage {
        value(&group, "Capacity").set_label(&capacity_label(bytes));
    }
    if !device.firmware.is_empty() {
        value(&group, "Firmware").set_label(&device.firmware);
    }
    group
}
