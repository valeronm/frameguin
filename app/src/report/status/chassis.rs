//! The chassis section: one row saying whether the machine is open, and a
//! page adding how often the EC has counted it opened.
//!
//! What each value is *called* is `frameguin_model::control::chassis`'s.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::chassis::{open_now_label, state_label, times_label};

use super::Sidebar;
use crate::reading::{Feed, Wants};
use crate::report::value;

pub(super) fn add(sidebar: &Rc<Sidebar>, feed: &Rc<Feed>) {
    let list = sidebar.section(None);
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    let open = value(&group, "Open now");
    let opened = value(&group, "Times opened");
    let found_open = value(&group, "Times found open at power-up");
    page.add(&group);
    let row = sidebar.add(&list, "Chassis", &page);

    let wants = Wants {
        chassis: true,
        ..Wants::default()
    };
    sidebar.follow(feed, wants, move |reading| {
        if let Some(state) = reading.chassis {
            row.set_subtitle(state_label(state.open));
            open.set_label(open_now_label(state.open));
            opened.set_label(&times_label(state.opened));
            found_open.set_label(&times_label(state.found_open));
        }
    });
}
