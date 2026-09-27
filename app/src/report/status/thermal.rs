//! The EC's temperature sensors and fans: a row whose summary is the hottest
//! sensor, and a page listing every sensor, then the fans.
//!
//! What each value is *called* is `frameguin_modelview::thermal`'s.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_contract::Platform;
use frameguin_model::control::thermal::Thermal;
use frameguin_model::reading::Request;
use frameguin_modelview::thermal::{
    NOT_PRESENT, fan_name, rpm_label, sensor_name, temperature_label, thermal_summary,
};
use frameguin_wire::Bus;
use gtk4 as gtk;

use super::Sidebar;
use crate::reading::{Feed, show_while_mapped};
use crate::report::value_row;

pub(super) fn add(
    sidebar: &Rc<Sidebar>,
    list: &gtk::ListBox,
    feed: &Rc<Feed>,
    thermal: &Thermal<Bus>,
    platform: Platform,
) {
    let layout = thermal.layout();
    let page = adw::PreferencesPage::new();
    let sensor_group = adw::PreferencesGroup::builder().title("Sensors").build();
    let sensors: Vec<(u8, gtk::Label)> = layout
        .sensors
        .iter()
        .map(|sensor| {
            (
                sensor.index,
                value_row(&sensor_group, &sensor_name(sensor)).1,
            )
        })
        .collect();
    if !sensors.is_empty() {
        page.add(&sensor_group);
    }
    let fan_group = adw::PreferencesGroup::builder().title("Cooling").build();
    let fans: Vec<(u8, gtk::Label)> = layout
        .fans
        .iter()
        .map(|index| {
            let name = fan_name(platform, *index, layout.fans.len());
            (*index, value_row(&fan_group, &name).1)
        })
        .collect();
    if !fans.is_empty() {
        page.add(&fan_group);
    }
    let row = sidebar.add(list, "Temperatures", &page);

    let summary = Request {
        thermal: true,
        ..Request::default()
    };
    let summarized = layout.clone();
    sidebar.follow(feed, summary, move |reading| {
        if let Some(state) = &reading.thermal {
            row.set_subtitle(&thermal_summary(&summarized, state));
        }
    });

    let request = Request {
        thermal: true,
        ..Request::default()
    };
    show_while_mapped(feed, &page, request, move |reading| {
        if let Some(state) = &reading.thermal {
            for (index, value) in &sensors {
                let label = state
                    .sensors
                    .iter()
                    .find(|s| s.index == *index)
                    .map_or(NOT_PRESENT.to_owned(), |s| temperature_label(s.temperature));
                value.set_label(&label);
            }
            for (index, value) in &fans {
                let label = state
                    .fans
                    .iter()
                    .find(|f| f.index == *index)
                    .map_or(NOT_PRESENT.to_owned(), |f| rpm_label(f.rpm));
                value.set_label(&label);
            }
        }
    });
}
