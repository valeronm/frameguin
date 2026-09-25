//! The chassis: one row saying whether the machine is open and naming
//! the input deck's state where it is not on, and a page adding how often the
//! EC has counted it opened and the input deck's power state.
//!
//! What each value is *called* is `frameguin_modelview::chassis`'s.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_contract::ChassisFeature;
use frameguin_model::control::chassis::Chassis;
use frameguin_model::reading::Request;
use frameguin_modelview::chassis::{chassis_summary, deck_label, open_now_label, times_label};
use frameguin_wire::Bus;
use gtk4 as gtk;

use super::Sidebar;
use crate::reading::Feed;
use crate::report::{value, value_row};

pub(super) fn add(
    sidebar: &Rc<Sidebar>,
    list: &gtk::ListBox,
    feed: &Rc<Feed>,
    chassis: &Chassis<Bus>,
) {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    let open = value(&group, "Open now");
    let opened = value(&group, "Times opened");
    let found_open = value(&group, "Times found open at power-up");
    let (deck_row, deck) = value_row(&group, "Input deck");
    let has_deck = chassis.has(ChassisFeature::Deck);
    deck_row.set_visible(has_deck);
    page.add(&group);
    let row = sidebar.add(list, "Chassis", &page);

    let request = Request {
        chassis: true,
        deck: has_deck,
        ..Request::default()
    };
    sidebar.follow(feed, request, move |reading| {
        if let Some(state) = reading.chassis {
            row.set_subtitle(&chassis_summary(state.open, reading.deck));
            open.set_label(open_now_label(state.open));
            opened.set_label(&times_label(state.opened));
            found_open.set_label(&times_label(state.found_open));
        }
        if let Some(state) = reading.deck {
            deck.set_label(deck_label(state));
        }
    });
}
