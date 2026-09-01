//! The ports report: what is plugged into each USB-C port, as the EC's copy
//! of its controllers' state has it.
//!
//! Nothing here writes — what a port does is settled between its controller
//! and whatever is attached — so none of [`crate::window`]'s machinery
//! applies, as in [`super::battery`].
//!
//! Fed rather than polled, for the reason [`crate::reading`] exists: the
//! Battery group's charger row shows the same read, and a window polling for
//! itself would have the two asking the daemon separately for one answer and
//! sitting a tick apart on it.
//!
//! What each value is *called* is `frameguin_model::control::ports`'s, and
//! where a socket is on the machine is `frameguin_model::port`'s — which
//! answers for the boards it has been measured on and no others.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::ports::{
    cc_label, contract_label, data_role_label, epr_label, negotiated, partner_label,
    power_role_label, vconn_label,
};
use frameguin_model::port;
use frameguin_wire::PortState;
use gtk4::gio;
use gtk4::glib;

use super::{Shell, value};
use crate::board;
use crate::reading::{Feed, Wants, show_while_mapped};

/// The application action that opens the report, and the only way in.
pub(crate) const ACTION: &str = "ports";

pub(super) fn action(feed: Rc<Feed>) -> gio::ActionEntry<adw::Application> {
    super::action(ACTION, "USB-C Ports", 640, move |shell| {
        build(shell, &feed);
    })
}

/// Fills the window, then follows the feed while it is mapped. The fill is
/// the read placed to say the daemon could not be reached; the feed's ticks
/// are silent, the rule every poll in this app follows.
fn build(shell: Shell, feed: &Rc<Feed>) {
    // The page is held weakly and nothing else here outlives it. The
    // subscription hangs on the page's own map signal, so a strong reference
    // to it from inside the shown closure would be the page keeping itself
    // alive — and this window is destroyed on close, which is the moment that
    // would silently stop happening.
    let page = shell.page.downgrade();
    let drawn = RefCell::new(Drawn::default());
    // Subscribed before the window is filled, as the battery report is and
    // for the same reason: filling it is then the feed's own read, which
    // every other view sees at the same instant — the main window's charger
    // row included, so opening this page cannot leave it a tick behind.
    let wants = Wants {
        ports: true,
        ..Wants::default()
    };
    show_while_mapped(feed, &shell.page, wants, move |reading| {
        if let (Some(page), Some(ports)) = (page.upgrade(), &reading.ports) {
            draw(&page, &drawn, ports);
        }
    });
    let feed = feed.clone();
    glib::spawn_future_local(async move {
        // Asks for nothing of its own: the subscription above is registered
        // by the time this runs, so the fold already carries what this window
        // wants.
        if let Err(e) = feed.read(Wants::default()).await {
            shell.toast_error("Reading the ports", e);
        }
    });
}

/// Redrawn whole rather than moved row by row: a port's rows come and go with
/// what is plugged into it — an empty one has no contract to show — so the set
/// of rows is itself part of the reading.
///
/// Only where the reading changed, though. A contract is a negotiated value
/// rather than a measurement, so consecutive readings are usually identical,
/// and rebuilding every row twice a second to paint the same thing costs a
/// full relayout for nothing.
fn draw(page: &adw::PreferencesPage, drawn: &RefCell<Drawn>, ports: &[PortState]) {
    let mut drawn = drawn.borrow_mut();
    if drawn.shown == ports {
        return;
    }
    let product = board::product();
    for group in drawn.groups.drain(..) {
        page.remove(&group);
    }
    for state in ports {
        let group = group(product, state);
        page.add(&group);
        drawn.groups.push(group);
    }
    drawn.shown = ports.to_vec();
}

/// What is on the page, and the reading it was drawn from. The page holds no
/// list of its own to walk, so a redraw takes back exactly what it added.
#[derive(Default)]
struct Drawn {
    groups: Vec<adw::PreferencesGroup>,
    shown: Vec<PortState>,
}

/// The heading is where the port is; its number goes in a row under that,
/// being what a reader needs only to match this against a tool that counts
/// ports rather than placing them. On a board with no measured position the
/// number is the heading and the row would say it twice, so there is none.
fn group(product: &str, state: &PortState) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(port::label(product, state.index))
        .build();
    if let Some(number) = port::secondary(product, state.index) {
        value(&group, "Port").set_label(&number);
    }
    let Some(partner) = partner_label(state.partner) else {
        group.set_description(Some("Nothing attached"));
        return group;
    };
    value(&group, "Attached").set_label(partner);
    if let Some(contract) = negotiated(state) {
        value(&group, "Negotiated").set_label(&contract);
    }
    if state.charging {
        value(&group, "Powering the machine").set_label("Yes");
    }
    if state.video {
        value(&group, "DisplayPort").set_label("Connected");
    }
    value(&group, "Power role").set_label(power_role_label(state.power_role));
    value(&group, "Data role").set_label(data_role_label(state.data_role));
    value(&group, "Contract").set_label(contract_label(state.contract));
    value(&group, "Orientation").set_label(cc_label(state.cc));
    value(&group, "VCONN").set_label(vconn_label(state.vconn));
    if let Some(epr) = epr_label(state.epr) {
        value(&group, "Extended power range").set_label(epr);
    }
    group
}
