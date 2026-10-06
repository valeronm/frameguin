//! The EC's temperature sensors and fans: a row whose summary is the hottest
//! sensor, and a page listing every sensor, then the fans.
//!
//! What each value is *called* is `frameguin_modelview::thermal`'s.

mod track;

use std::rc::Rc;

use adw::prelude::*;
use frameguin_contract::{Platform, Sensor, ThermalState, Thresholds};
use frameguin_model::control::thermal::Thermal;
use frameguin_model::reading::Request;
use frameguin_modelview::thermal::{
    NOT_PRESENT, Scale, fan_label, fan_name, sensor_name, temperature_label, thermal_summary,
};
use frameguin_wire::Bus;
use gtk4 as gtk;

use self::track::{TrackView, legend};
use super::Sidebar;
use crate::reading::{Feed, show_while_mapped};
use crate::report::{value_label, value_row};

/// A `PreferencesRow` gives a child of its own none of an action row's
/// padding or height.
const ROW_PADDING: i32 = 12;
const ROW_MARGIN: i32 = 10;
const ROW_HEADER_HEIGHT: i32 = 30;

/// A sensor's name and temperature over its track, which an action row has
/// no place for.
struct SensorRow {
    sensor: Sensor,
    value: gtk::Label,
    track: TrackView,
}

impl SensorRow {
    fn new(group: &adw::PreferencesGroup, sensor: &Sensor) -> Self {
        let title = sensor_name(sensor);
        let name = gtk::Label::builder()
            .label(&title)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let value = value_label();
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(ROW_PADDING)
            .height_request(ROW_HEADER_HEIGHT)
            .build();
        header.append(&name);
        header.append(&value);
        let track = TrackView::new();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_top(ROW_MARGIN)
            .margin_bottom(ROW_MARGIN)
            .margin_start(ROW_PADDING)
            .margin_end(ROW_PADDING)
            .build();
        content.append(&header);
        content.append(track.widget());
        let row = adw::PreferencesRow::builder()
            .title(&title)
            .activatable(false)
            .child(&content)
            .build();
        group.add(&row);
        Self {
            sensor: sensor.clone(),
            value,
            track,
        }
    }

    /// The track keeps what it shows where `thresholds` is None, as a reading
    /// carries none where nothing had asked for them and where the ask
    /// failed.
    fn show(&self, state: &ThermalState, thresholds: Option<(&[Thresholds], Scale)>) {
        let reading = state.sensors.iter().find(|s| s.index == self.sensor.index);
        let label = reading.map_or(NOT_PRESENT.to_owned(), |s| temperature_label(s.temperature));
        self.value.set_label(&label);
        if let Some((all, scale)) = thresholds {
            let track = reading.and_then(|s| scale.track(&self.sensor, all, s.temperature));
            self.track.show(track);
        }
    }
}

pub(super) fn add(
    sidebar: &Rc<Sidebar>,
    list: &gtk::ListBox,
    feed: &Rc<Feed>,
    thermal: &Thermal<Bus>,
    platform: Platform,
) {
    let layout = thermal.layout();
    let page = adw::PreferencesPage::new();
    let sensor_group = adw::PreferencesGroup::builder()
        .title("Sensors")
        .header_suffix(&legend())
        .build();
    let sensors: Vec<SensorRow> = layout
        .sensors
        .iter()
        .map(|sensor| SensorRow::new(&sensor_group, sensor))
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

    let listed = layout.sensors.clone();
    let request = Request {
        thermal: true,
        thresholds: true,
        fan_duty: true,
        ..Request::default()
    };
    show_while_mapped(feed, &page, request, move |reading| {
        if let Some(state) = &reading.thermal {
            let thresholds = reading
                .thresholds
                .as_deref()
                .map(|all| (all, Scale::of(platform, &listed, all)));
            for row in &sensors {
                row.show(state, thresholds);
            }
            let duties = reading.fan_duties.as_deref().unwrap_or_default();
            for (index, value) in &fans {
                let label = state
                    .fans
                    .iter()
                    .find(|f| f.index == *index)
                    .map_or(NOT_PRESENT.to_owned(), |f| fan_label(f, duties));
                value.set_label(&label);
            }
        }
    });
}
