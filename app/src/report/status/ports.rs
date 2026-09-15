//! The USB-C ports section: a row per port carrying what is plugged into it,
//! and a page per port with the rest of what the EC's copy of its
//! controller's state says.
//!
//! What each value is *called* is `frameguin_model::control::ports`'s, and
//! where a socket is on the machine is `frameguin_model::port`'s — which
//! answers for the boards it has been measured on and no others.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::ports::{
    NOTHING_ATTACHED, POWERING_THE_MACHINE, cc_label, contract_label, data_role_label, epr_label,
    negotiated, partner_label, port_summary, power_role_label, vconn_label,
};
use frameguin_model::port;
use frameguin_wire::PortState;
use gtk4 as gtk;

use super::{Sidebar, Target};
use crate::board;
use crate::reading::{Feed, Wants};
use crate::report::{described_value, value};

/// The section's rows, one per port the last reading carried.
struct Section {
    list: gtk::ListBox,
    ports: RefCell<Vec<Port>>,
}

struct Port {
    index: u8,
    row: adw::ActionRow,
    page: adw::PreferencesPage,
    drawn: RefCell<Option<(adw::PreferencesGroup, PortState)>>,
}

/// The rows arrive with the first reading, which is what says how many ports
/// there are.
pub(super) fn add(sidebar: &Rc<Sidebar>, feed: &Rc<Feed>) {
    let section = Rc::new(Section {
        list: sidebar.section(Some("USB-C Ports")),
        ports: RefCell::default(),
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
        ..Wants::default()
    };
    let showing = sidebar.clone();
    sidebar.follow(feed, wants, move |reading| {
        if let Some(ports) = &reading.ports {
            section.show(&showing, ports);
        }
    });
}

impl Section {
    /// The rows are rebuilt only where the set of ports changed, which on a
    /// board's fixed ports is the first reading alone.
    fn show(&self, sidebar: &Sidebar, ports: &[PortState]) {
        let product = board::product();
        let mut ports: Vec<&PortState> = ports.iter().collect();
        ports.sort_by_key(|state| port::order(product, state.index));
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
                    let row = sidebar.add(&self.list, &port::label(product, state.index), &page);
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
            port.draw(product, state);
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
                    .is_some_and(|(_, state)| state.charging)
            })
            .or_else(|| ports.first())
            .map(|port| port.row.clone().upcast())
    }
}

impl Port {
    /// Redrawn whole: which rows a port has depends on what is plugged into
    /// it. Skipped where the state is unchanged, a contract being negotiated
    /// rather than measured and consecutive readings usually identical.
    fn draw(&self, product: &str, state: &PortState) {
        let mut drawn = self.drawn.borrow_mut();
        if drawn.as_ref().is_some_and(|(_, shown)| shown == state) {
            return;
        }
        if let Some((group, _)) = drawn.take() {
            self.page.remove(&group);
        }
        self.row.set_subtitle(&port_summary(state));
        let group = group(product, state);
        self.page.add(&group);
        *drawn = Some((group, state.clone()));
    }
}

/// Untitled, the page's own title already naming where the port is.
fn group(product: &str, state: &PortState) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    if let Some(number) = port::secondary(product, state.index) {
        value(&group, "Port").set_label(&number);
    }
    let Some(partner) = partner_label(state.partner) else {
        group.set_description(Some(NOTHING_ATTACHED));
        return group;
    };
    value(&group, "Attached").set_label(partner);
    if let Some(contract) = negotiated(state) {
        value(&group, "Negotiated").set_label(&contract);
    }
    if state.charging {
        value(&group, POWERING_THE_MACHINE).set_label("Yes");
    }
    if state.video {
        value(&group, "DisplayPort").set_label("Connected");
    }
    value(&group, "Power role").set_label(power_role_label(state.power_role));
    value(&group, "Data role").set_label(data_role_label(state.data_role));
    value(&group, "Contract").set_label(contract_label(state.contract));
    value(&group, "Orientation").set_label(cc_label(state.cc));
    described_value(
        &group,
        "VCONN",
        "Whether this port powers the chips inside the cable or accessory",
    )
    .set_label(vconn_label(state.vconn));
    if let Some(epr) = epr_label(state.epr) {
        value(&group, "Extended power range").set_label(epr);
    }
    group
}
