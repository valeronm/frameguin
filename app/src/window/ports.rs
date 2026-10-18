//! The USB-C ports group: a switch per port its controller can turn off,
//! each subtitled with what the port is doing.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use frameguin_contract::{Attached, PortMask, PortState};
use frameguin_model::control::ports;
use frameguin_model::reading::Request;
use frameguin_modelview::port;
use frameguin_modelview::ports::switch_line;
use frameguin_wire::Bus;
use gtk4::glib;

use crate::reading::show_while_mapped;
use crate::window::Ui;
use crate::window::widgets::{connect_switch, show_switch};

pub(crate) type Ports = ports::Ports<Bus>;

const PARTNER_DETECTED: Duration = Duration::from_millis(500);

pub(crate) struct Group {
    pub(crate) widget: adw::PreferencesGroup,
    /// By the order the ports sit in, each with its EC port number; built
    /// when gated, the switchable ports being fixed for the daemon's run.
    rows: RefCell<Vec<(u8, adw::SwitchRow)>>,
    control: RefCell<Option<Rc<Ports>>>,
}

impl Group {
    pub(crate) fn build() -> Self {
        let widget = adw::PreferencesGroup::builder()
            .title("USB-C ports")
            .description("A port stays off through restarts until it is switched back on")
            .build();
        Self {
            widget,
            rows: RefCell::default(),
            control: RefCell::default(),
        }
    }

    pub(crate) fn gate(&self, control: Option<&Rc<Ports>>) {
        let mut rows = self.rows.borrow_mut();
        for (_, row) in rows.drain(..) {
            self.widget.remove(&row);
        }
        self.control.replace(control.cloned());
        let Some(control) = control else {
            self.widget.set_visible(false);
            return;
        };
        let placement = control.placement();
        let mut indices: Vec<u8> = control.switchable().indices().collect();
        indices.sort_by_key(|&index| placement.order(index));
        for index in indices {
            let row = adw::SwitchRow::builder()
                .title(port::label(placement, index))
                .sensitive(false)
                .build();
            self.widget.add(&row);
            rows.push((index, row));
        }
        self.widget.set_visible(!rows.is_empty());
    }

    pub(crate) fn has_fed_rows(&self) -> bool {
        self.widget.is_visible()
    }

    /// Fed rather than read on load: a device plugged in moves the subtitle,
    /// and another client turning a port off moves the switch.
    pub(crate) fn watch(&self, ui: &Rc<Ui>) {
        let row_ui = ui.clone();
        let request = Request {
            ports: true,
            usb: true,
            ..Request::default()
        };
        show_while_mapped(&ui.feed, &self.widget, request, move |reading| {
            if let Some(ports) = &reading.ports {
                let devices = reading.usb.as_deref().unwrap_or_default();
                row_ui.ports.show(&row_ui, ports, devices);
            }
        });
    }

    fn show(&self, ui: &Ui, ports: &[PortState], devices: &[Attached]) {
        let control = self.control.borrow();
        let Some(control) = control.as_ref() else {
            return;
        };
        let placement = control.placement();
        ui.sync(|| {
            for (index, row) in self.rows.borrow().iter() {
                let Some(state) = ports.iter().find(|state| state.index == *index) else {
                    continue;
                };
                row.set_active(!matches!(state.mask, PortMask::Off));
                row.set_sensitive(state.mask.refusal().is_none());
                row.set_subtitle(&switch_line(placement, state, devices));
            }
        });
    }

    /// A refused write puts the switch back: a port shown in a state it is
    /// not in is one someone plugs into or leaves alone on its word. Every
    /// write is followed by a reading, since the lock moves with the port
    /// switched.
    pub(crate) fn connect(&self, ui: &Rc<Ui>, control: &Rc<Ports>) {
        for (index, row) in self.rows.borrow().iter() {
            let (index, switched) = (*index, row.downgrade());
            connect_switch(ui, control, row, move |ui, control, enabled| {
                let switched = switched.clone();
                async move {
                    let written = {
                        let _writing = ui.feed.writing();
                        control.set_enabled(index, enabled).await
                    };
                    let turned_on = enabled && written.is_ok();
                    if let Err(e) = written {
                        ui.toast_error("Switching a USB-C port", e);
                        if let Some(row) = switched.upgrade() {
                            show_switch(&ui, &row, !enabled);
                        }
                    }
                    // Its own failure is the next tick's to report.
                    let _ = ui.feed.fill(&ui.ports.widget).await;
                    if turned_on {
                        // A port turned on reads nothing attached until its
                        // controller has detected the partner.
                        glib::timeout_future(PARTNER_DETECTED).await;
                        let _ = ui.feed.fill(&ui.ports.widget).await;
                    }
                }
            });
        }
    }
}
