//! The Power button LED group: a combo over the levels the board has, and the
//! slider its Custom row reveals.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_contract::PowerLedLevel;
use frameguin_model::control::Custom;
use frameguin_model::control::power_led::{self, MIN_POWER_LED_BRIGHTNESS, Snapshot, labels};
use frameguin_wire::Bus;
use gtk4 as gtk;

use crate::window::Ui;
use crate::window::widgets::{
    SliderWrites, build_scale, connect_combo, connect_slider_writes, reveal_under, scale_percent,
    select_row, string_list,
};

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
        let floor = f64::from(MIN_POWER_LED_BRIGHTNESS);
        let adjustment = gtk::Adjustment::new(floor, floor, 100.0, 10.0, 10.0, 0.0);
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
            if let Some(index) = control.custom_row() {
                reveal_under(&self.combo, &self.custom_row, index);
            }
        }
    }

    pub(crate) async fn load(&self, ui: &Ui, control: &PowerLed) {
        match control.read().await {
            Ok(snapshot) => self.show(ui, control, snapshot, Custom::Rederive),
            Err(e) => ui.toast_error("Reading the power button LED", e),
        }
    }

    /// Moves the widgets onto the snapshot without their handlers writing it
    /// back, and makes them usable — a row is only ever filled from a read
    /// that succeeded.
    fn show(&self, ui: &Ui, control: &PowerLed, snapshot: Snapshot, custom: Custom) {
        ui.sync(|| {
            self.scale.set_value(f64::from(snapshot.percent));
            self.scale.set_sensitive(true);
            self.combo.set_sensitive(true);
            select_row(&self.combo, |selected| {
                control.row_for(snapshot.level, selected, custom)
            });
        });
    }

    /// Shown from a read after every write, taken or refused: a preset
    /// resolves to a percentage only the EC knows, and a refused write can
    /// leave a level the window never picked — Off, where the kernel would
    /// not hand the LED back — which the combo would otherwise go on
    /// asserting as lit. Silent on a read that fails, the next reload asking
    /// again.
    async fn reload(&self, ui: &Ui, control: &PowerLed, custom: Custom) {
        if let Ok(snapshot) = control.read().await {
            self.show(ui, control, snapshot, custom);
        }
    }

    pub(crate) fn connect(&self, ui: &Rc<Ui>, control: &Rc<PowerLed>) {
        // Only reachable while the combo sits on Custom.
        connect_slider_writes(
            ui,
            control,
            &self.scale,
            scale_percent,
            |ui, control, percent| async move { write_brightness(&ui, &control, percent).await },
            SliderWrites::Live,
        );

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
                        write_brightness(&ui, &control, percent).await;
                        return;
                    }
                    if let Err(e) = control.set_level(level).await {
                        ui.toast_error("Setting the power button LED level", e);
                    }
                    ui.power_led.reload(&ui, &control, Custom::Rederive).await;
                }
            },
        );
    }
}

async fn write_brightness(ui: &Ui, control: &PowerLed, percent: u8) {
    let custom = match control.set_brightness(percent).await {
        Ok(()) => Custom::Keep,
        Err(e) => {
            ui.toast_error("Setting the power button LED brightness", e);
            Custom::Rederive
        }
    };
    ui.power_led.reload(ui, control, custom).await;
}
