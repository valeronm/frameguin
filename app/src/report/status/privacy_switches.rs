//! The privacy switches section: one row naming where both switches sit, and
//! a page with a row for each.
//!
//! What each value is *called* is `frameguin_model::control::privacy_switches`'s.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::privacy_switches::{switch_label, switches_summary};

use super::Sidebar;
use crate::reading::{Feed, Wants};
use crate::report::value;

pub(super) fn add(sidebar: &Rc<Sidebar>, feed: &Rc<Feed>) {
    let list = sidebar.section(None);
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    let camera = value(&group, "Camera");
    let microphone = value(&group, "Microphone");
    page.add(&group);
    let row = sidebar.add(&list, "Privacy switches", &page);

    let wants = Wants {
        privacy_switches: true,
        ..Wants::default()
    };
    sidebar.follow(feed, wants, move |reading| {
        if let Some(switches) = reading.privacy_switches {
            row.set_subtitle(&switches_summary(switches));
            camera.set_label(switch_label(switches.camera));
            microphone.set_label(switch_label(switches.microphone));
        }
    });
}
