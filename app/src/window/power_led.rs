//! The Power button LED group: a combo over the levels the board has, the
//! slider its Custom row reveals, and the one write both front-ends make.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::power_led::{self, Snapshot, labels};
use frameguin_wire::PowerLedLevel;
use gtk4 as gtk;

use crate::bus::Bus;
use crate::tray::TrayValues;
use crate::window::widgets::{
    SliderWrites, build_scale, combo_selection, connect_combo, connect_slider_writes, reveal_under,
    scale_percent, string_list,
};
use crate::window::{Sink, Ui};

pub(crate) type PowerLed = power_led::PowerLed<Bus>;

pub(crate) struct Group {
    pub(crate) widget: adw::PreferencesGroup,
    combo: adw::ComboRow,
    scale: gtk::Scale,
    custom_row: adw::ActionRow,
}

impl Group {
    pub(crate) fn build() -> Self {
        // "Power button LED" is the name Framework's own firmware setup uses.
        let widget = adw::PreferencesGroup::builder()
            .title("Power button LED")
            .build();
        // No model: which levels a board has is the device's answer, and the
        // row it would show meanwhile is one the board may not have.
        let combo = adw::ComboRow::builder()
            .title("Level")
            .sensitive(false)
            .build();
        widget.add(&combo);
        let custom_row = adw::ActionRow::builder().title("Brightness").build();
        // The EC accepts 1-100 for this LED; 0 is not a valid level.
        let adjustment = gtk::Adjustment::new(1.0, 1.0, 100.0, 10.0, 10.0, 0.0);
        let scale = build_scale(&adjustment, |value| format!("{value:.0}%"));
        custom_row.add_suffix(&scale);
        custom_row.set_visible(false);
        widget.add(&custom_row);
        Self {
            widget,
            combo,
            scale,
            custom_row,
        }
    }

    /// Shows the group where the EC answers for the LED, and hides it
    /// otherwise. The combo's rows are the levels this board has, fixed for
    /// the daemon's run, so they are set here rather than by every reload —
    /// and before any handler is connected, so nothing echoes the model.
    pub(crate) fn gate(&self, control: Option<&Rc<PowerLed>>) {
        self.widget.set_visible(control.is_some());
        if let Some(control) = control {
            self.combo
                .set_model(Some(&string_list(&labels(control.rows()))));
            if let Some(index) = control.row(PowerLedLevel::Custom) {
                reveal_under(&self.combo, &self.custom_row, index);
            }
        }
    }

    pub(crate) async fn load(&self, ui: &Ui, control: &PowerLed, values: &mut TrayValues) {
        match control.read().await {
            Ok(snapshot) => {
                self.show(ui, control, snapshot);
                values.power_led_level = Some(snapshot.level);
            }
            Err(e) => ui.toast_error("Reading the power button LED", e),
        }
    }

    /// Moves the widgets onto the snapshot without their handlers writing it
    /// back, and makes them usable — a row is only ever filled from a read
    /// that succeeded.
    fn show(&self, ui: &Ui, control: &PowerLed, snapshot: Snapshot) {
        ui.sync(|| {
            self.scale.set_value(f64::from(snapshot.percent));
            self.scale.set_sensitive(true);
            self.combo.set_sensitive(true);
            self.show_level(control, snapshot.level);
        });
    }

    /// Call under [`Ui::sync`].
    fn show_level(&self, control: &PowerLed, level: PowerLedLevel) {
        self.combo.set_selected(combo_selection(control.row(level)));
    }

    pub(crate) fn connect(&self, ui: &Rc<Ui>, control: &Rc<PowerLed>) {
        // Slider: a raw percentage write; only reachable while the level is
        // Custom, so combo and tray already reflect it.
        connect_slider_writes(
            ui,
            control,
            &self.scale,
            scale_percent,
            |ui, control, percent| async move { apply_brightness(&ui, &control, percent).await },
            SliderWrites::Live,
        );

        // Combo: presets write the level and re-read so the slider carries
        // the percentage the preset resolved to; Custom reveals the slider
        // and applies its value, making the EC state actually custom.
        let at_control = control.clone();
        connect_combo(
            ui,
            control,
            &self.combo,
            move |index| at_control.at(index),
            |ui, control, level| {
                let percent = scale_percent(ui.power_led.scale.value());
                async move {
                    if level == PowerLedLevel::Custom {
                        apply_brightness(&ui, &control, percent).await;
                        return;
                    }
                    apply(Sink::Window(&ui), &control, level).await;
                }
            },
        );
    }
}

/// The one write for a preset. The window's combo and the tray's row both
/// come here, so neither can drift from the other on what it reports or what
/// it tells the tray. Custom is not a preset: the EC reports it after a raw
/// percentage write, which goes through [`apply_brightness`] instead.
pub(crate) async fn apply(sink: Sink<'_>, control: &PowerLed, level: PowerLedLevel) {
    if let Err(e) = control.set_level(level).await {
        sink.toast_error("Setting the power button LED level", e);
        return;
    }
    sink.push_tray(TrayValues::power_led_level(level));
    let Sink::Window(ui) = sink else {
        return;
    };
    ui.sync(|| ui.power_led.show_level(control, level));
    // The preset resolves to a percentage only the EC knows, and only the
    // window has anywhere to put it.
    if let Ok(snapshot) = control.read().await {
        ui.sync(|| ui.power_led.scale.set_value(f64::from(snapshot.percent)));
    }
}

/// The one write for a custom percentage. Any raw percentage leaves the EC
/// reporting "custom", so this owns that consequence rather than leaving
/// each caller to remember it.
async fn apply_brightness(ui: &Ui, control: &PowerLed, percent: u8) {
    if let Err(e) = control.set_brightness(percent).await {
        ui.toast_error("Setting the power button LED brightness", e);
        return;
    }
    ui.sync_tray(TrayValues::power_led_level(PowerLedLevel::Custom));
}
