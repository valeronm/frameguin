//! The Charging LED group: the switch, and which side of the chassis is lit.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_contract::ChargingLedFeature;
use frameguin_model::control::charging_led;
use frameguin_model::reading::Request;
use frameguin_modelview::charging_led::side_label;
use frameguin_wire::Bus;
use gtk4 as gtk;

use crate::reading::show_while_mapped;
use crate::window::Ui;
use crate::window::widgets::{connect_switch, show_switch};

pub(crate) type ChargingLed = charging_led::ChargingLed<Bus>;

pub(crate) struct Group {
    pub(crate) widget: adw::PreferencesGroup,
    switch: adw::SwitchRow,
    side_row: adw::ActionRow,
    side: gtk::Label,
}

impl Group {
    pub(crate) fn build() -> Self {
        let widget = adw::PreferencesGroup::builder()
            .title("Charging LED")
            .build();
        let switch = adw::SwitchRow::builder()
            .title("Indicator")
            .subtitle("Off also hides the battery and fault warnings it blinks")
            .sensitive(false)
            .build();
        let side_row = adw::ActionRow::builder().title("Lit side").build();
        let side = gtk::Label::new(None);
        side_row.add_suffix(&side);
        widget.add(&side_row);
        widget.add(&switch);
        Self {
            widget,
            switch,
            side_row,
            side,
        }
    }

    /// Shows the group where the kernel can hold the LED, and the side row
    /// where the EC names the sides.
    pub(crate) fn gate(&self, control: Option<&Rc<ChargingLed>>) {
        self.widget.set_visible(control.is_some());
        self.side_row
            .set_visible(control.is_some_and(|control| control.has(ChargingLedFeature::Side)));
    }

    /// Fed rather than read on load alone: the side moves with the charger,
    /// whether or not anyone touches the app.
    pub(crate) fn watch(&self, ui: &Rc<Ui>) {
        let label = self.side.clone();
        let request = Request {
            charging_led_side: true,
            ..Request::default()
        };
        show_while_mapped(&ui.feed, &self.side_row, request, move |reading| {
            if let Some(side) = reading.charging_led_side {
                label.set_label(side_label(side));
            }
        });
    }

    pub(crate) fn has_fed_rows(&self) -> bool {
        self.widget.is_visible() && self.side_row.is_visible()
    }

    pub(crate) async fn load(&self, ui: &Ui, control: &ChargingLed) {
        match control.read().await {
            Ok(enabled) => show_switch(ui, &self.switch, enabled),
            Err(e) => ui.toast_error("Reading the charging LED", e),
        }
    }

    /// Re-read after every write, taken or refused: a switch left on Off by
    /// a refused write claims warnings are hidden that the LED still shows.
    pub(crate) fn connect(&self, ui: &Rc<Ui>, control: &Rc<ChargingLed>) {
        connect_switch(
            ui,
            control,
            &self.switch,
            |ui, control, enabled| async move {
                if let Err(e) = control.set_enabled(enabled).await {
                    ui.toast_error("Switching the charging LED", e);
                }
                let group = &ui.charging_led;
                if let Ok(enabled) = control.read().await {
                    show_switch(&ui, &group.switch, enabled);
                }
                if control.has(ChargingLedFeature::Side)
                    && let Ok(side) = control.side().await
                {
                    group.side.set_label(side_label(side));
                }
            },
        );
    }
}
