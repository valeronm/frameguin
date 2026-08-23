//! The tray icon (`StatusNotifierItem`; shown by GNOME's `AppIndicator`
//! extension) and the values its menu renders from.
//!
//! It draws with no window open, so it keeps its own copy of everything the
//! menu names, pushed in by whoever last read or wrote it.

use frameguin_wire::{BatteryState, Capability, FpLevel, FrameguinProxy};

use crate::APP_ID;
use crate::caps::{Capabilities, fp_presets};
use crate::format::{
    CHARGE_SPEED_LABELS, amps, battery_summary, charge_limit_labels, charge_limit_percent,
    charge_limit_position, charge_speed_milliamps, charge_speed_position, fp_level_labels,
};

pub(crate) enum TrayEvent {
    Show,
    Refresh,
    SetChargeLimit(u8),
    /// Already resolved to milliamps against the capacity the menu was drawn
    /// from, so applying it needs nothing the window has to supply.
    SetChargeSpeed(u32),
    SetFingerprintLevel(FpLevel),
    Quit,
}

pub(crate) struct TrayIcon {
    tx: async_channel::Sender<TrayEvent>,
    /// The pack as the daemon last reported it, pushed in from the app. The
    /// one value here that moves on its own, so the menu carries it only
    /// because opening the menu is what asks for it — and a refresh that
    /// fails to read leaves the last one standing, the push protocol having
    /// no way to say "no longer known".
    battery: Option<BatteryState>,
    /// Currently applied charge limit, pushed in from the app so the radio
    /// group can mark it; None until the first daemon read.
    charge_limit: Option<u8>,
    /// The charge current limit in mA and the battery capacity that names the
    /// speeds, both pushed in from the app. Without the capacity the submenu
    /// stays out: a fraction then has no rate to show or to send.
    charge_current_limit: Option<u32>,
    design_capacity: Option<u32>,
    /// Current fingerprint LED level, pushed in from the app; Custom marks no
    /// radio option.
    fp_level: Option<FpLevel>,
    /// Pushed in once the app reads the daemon's probe, and fixed for the
    /// daemon's run thereafter. None until then, which leaves the menu at
    /// Open/Quit.
    caps: Option<Capabilities>,
}

impl TrayIcon {
    /// Everything the menu draws from arrives later, over `tray_push`.
    pub(crate) fn new(tx: async_channel::Sender<TrayEvent>) -> Self {
        Self {
            tx,
            battery: None,
            charge_limit: None,
            charge_current_limit: None,
            design_capacity: None,
            fp_level: None,
            caps: None,
        }
    }

    fn send(&self, event: TrayEvent) {
        let _ = self.tx.send_blocking(event);
    }
}

impl ksni::Tray for TrayIcon {
    fn id(&self) -> String {
        APP_ID.into()
    }

    fn icon_name(&self) -> String {
        format!("{APP_ID}-symbolic")
    }

    fn title(&self) -> String {
        "Frameguin".into()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayEvent::Show);
    }

    /// The menu renders from values pushed in earlier, which the EC and other
    /// tools can invalidate at any time, so opening it asks for fresh ones.
    /// They arrive a moment later and ksni re-renders once they land.
    fn menu_about_to_show(&mut self) {
        self.send(TrayEvent::Refresh);
    }

    /// The menu, grouped by subsystem: the window, then the battery with its
    /// reading above the controls that shape it, then the fingerprint LED,
    /// then the way out.
    ///
    /// Each item decides for itself whether this board can offer it, so the
    /// separators are placed by asking which groups came back with anything
    /// rather than by restating those conditions here — a board with no
    /// battery controls draws no separator around the gap where they would
    /// have been.
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let groups: [Vec<ksni::MenuItem<Self>>; 4] = [
            vec![
                StandardItem {
                    label: "Open".into(),
                    activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::Show)),
                    ..Default::default()
                }
                .into(),
            ],
            [
                self.battery_item(),
                self.charge_limit_item(),
                self.charge_speed_item(),
            ]
            .into_iter()
            .flatten()
            .collect(),
            self.fp_level_item().into_iter().collect(),
            vec![
                StandardItem {
                    label: "Quit Frameguin".into(),
                    activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::Quit)),
                    ..Default::default()
                }
                .into(),
            ],
        ];
        let mut items: Vec<ksni::MenuItem<Self>> = Vec::new();
        for group in groups.into_iter().filter(|group| !group.is_empty()) {
            if !items.is_empty() {
                items.push(ksni::MenuItem::Separator);
            }
            items.extend(group);
        }
        items
    }
}

/// One shape for every preset menu the tray offers: a submenu named after the
/// active option, holding a radio group over all of them. `selected` is None
/// when the hardware sits on no preset, which leaves the group unmarked.
fn radio_submenu(
    title: String,
    selected: Option<usize>,
    labels: Vec<String>,
    select: impl Fn(&mut TrayIcon, usize) + Send + 'static,
) -> ksni::MenuItem<TrayIcon> {
    use ksni::menu::{RadioGroup, RadioItem, SubMenu};
    SubMenu {
        label: title,
        submenu: vec![
            RadioGroup {
                selected: selected.unwrap_or(usize::MAX),
                select: Box::new(select),
                options: labels
                    .into_iter()
                    .map(|label| RadioItem {
                        label,
                        ..Default::default()
                    })
                    .collect(),
            }
            .into(),
        ],
        ..Default::default()
    }
    .into()
}

impl TrayIcon {
    /// The reading heading the battery group, disabled so that it reads as
    /// the line of text it is and no click can land on it. Asks the
    /// capability like every other item, and the value on top of it: a board
    /// that has the reading still has nothing to show until the first one
    /// arrives.
    fn battery_item(&self) -> Option<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        if !self.caps?.has(Capability::BatteryState) {
            return None;
        }
        Some(
            StandardItem {
                label: battery_summary(self.battery?),
                enabled: false,
                ..Default::default()
            }
            .into(),
        )
    }

    fn charge_limit_item(&self) -> Option<ksni::MenuItem<Self>> {
        if !self.caps?.has(Capability::ChargeLimit) {
            return None;
        }
        let labels = charge_limit_labels();
        // Spelled here rather than read off the labels, the way the other
        // two titles name their selected option: the row for 100% reads "No
        // limit", which as a title would come out "Charge limit (No limit)".
        // Each word is the better one where it sits, so the two differ.
        let title = match self.charge_limit {
            Some(100) => "Charge limit (Off)".into(),
            Some(limit) => format!("Charge limit ({limit}%)"),
            None => "Charge limit".into(),
        };
        let selected = self.charge_limit.and_then(charge_limit_position);
        Some(radio_submenu(title, selected, labels, |tray, index| {
            tray.send(TrayEvent::SetChargeLimit(charge_limit_percent(index)));
        }))
    }

    fn charge_speed_item(&self) -> Option<ksni::MenuItem<Self>> {
        if !self.caps?.has(Capability::ChargeCurrentLimit) {
            return None;
        }
        // Still needed to turn the chosen speed into the milliamps the daemon
        // takes, even though the menu names speeds rather than currents.
        let design_capacity = self.design_capacity?;
        // Bare preset names, not the window's `charge_speed_labels`: those
        // carry the rate in brackets, which would nest inside the submenu
        // title's own brackets.
        let labels = CHARGE_SPEED_LABELS
            .iter()
            .map(|l| (*l).to_string())
            .collect();
        let selected = self
            .charge_current_limit
            .and_then(|milliamps| charge_speed_position(design_capacity, milliamps));
        // Named by its preset where there is one, and by the current itself
        // where there isn't — a menu that can only show presets would say
        // nothing at all about a limit dialled in from the window.
        let title = match (selected, self.charge_current_limit) {
            (Some(index), _) => format!("Charge speed ({})", CHARGE_SPEED_LABELS[index]),
            (None, Some(milliamps)) => format!("Charge speed ({})", amps(milliamps)),
            (None, None) => "Charge speed".into(),
        };
        Some(radio_submenu(title, selected, labels, move |tray, index| {
            tray.send(TrayEvent::SetChargeSpeed(charge_speed_milliamps(
                design_capacity,
                index,
            )));
        }))
    }

    fn fp_level_item(&self) -> Option<ksni::MenuItem<Self>> {
        let caps = self.caps?;
        if !caps.has(Capability::FpBrightness) {
            return None;
        }
        let levels = fp_presets(caps);
        let selected = self
            .fp_level
            .and_then(|level| levels.iter().position(|l| *l == level));
        let options = fp_level_labels(&levels);
        let title = match selected {
            Some(index) => format!("Fingerprint LED ({})", options[index]),
            None => "Fingerprint LED".into(),
        };
        Some(radio_submenu(title, selected, options, move |tray, index| {
            tray.send(TrayEvent::SetFingerprintLevel(levels[index]));
        }))
    }
}

/// What a caller knows about the tray's state. A field left None is one this
/// caller cannot speak for, and the menu keeps what it already holds — which
/// is what makes a write from a window that has not read the battery yet safe
/// to apply: it knows the milliamps but not the capacity, and the menu's own
/// copy is the better one.
#[derive(Clone, Copy, Default)]
pub(crate) struct TrayValues {
    pub(crate) caps: Option<Capabilities>,
    pub(crate) battery: Option<BatteryState>,
    pub(crate) charge_limit: Option<u8>,
    pub(crate) design_capacity: Option<u32>,
    pub(crate) charge_current_limit: Option<u32>,
    pub(crate) fp_level: Option<FpLevel>,
}

impl TrayValues {
    pub(crate) fn caps(caps: Capabilities) -> Self {
        Self {
            caps: Some(caps),
            ..Self::default()
        }
    }

    pub(crate) fn charge_limit(percent: u8) -> Self {
        Self {
            charge_limit: Some(percent),
            ..Self::default()
        }
    }

    pub(crate) fn fp_level(level: FpLevel) -> Self {
        Self {
            fp_level: Some(level),
            ..Self::default()
        }
    }
}

/// The tray-push protocol, owned in one place: the window's `Ui`, the write
/// sinks and the tray-only startup path all come through here. Everything a
/// caller knows travels in one `update`, because each one blocks on the tray's
/// thread and makes it rebuild and re-signal the whole menu.
pub(crate) fn tray_push(handle: &ksni::blocking::Handle<TrayIcon>, values: TrayValues) {
    handle.update(move |tray| {
        tray.caps = values.caps.or(tray.caps);
        tray.battery = values.battery.or(tray.battery);
        tray.charge_limit = values.charge_limit.or(tray.charge_limit);
        tray.design_capacity = values.design_capacity.or(tray.design_capacity);
        tray.charge_current_limit = values.charge_current_limit.or(tray.charge_current_limit);
        tray.fp_level = values.fp_level.or(tray.fp_level);
    });
}

/// Reads the values the menu renders from. The tray keeps its own copies
/// because it has to draw with no window open, so they are pulled from the
/// daemon rather than from the window's widgets.
pub(crate) async fn refresh_tray(
    handle: &ksni::blocking::Handle<TrayIcon>,
    proxy: &FrameguinProxy<'static>,
) {
    // One read and one write. Every `update` blocks this thread on the tray's
    // own, and makes it rebuild the entire menu and signal it over D-Bus, so
    // a field-at-a-time refresh would do that four times for one menu.
    // Capabilities and the battery's capacity are both fixed for the daemon's
    // run, so what the menu already holds of them is kept and only the values
    // that can change are asked for again.
    let Some((known_caps, known_capacity)) = handle.update(|tray| (tray.caps, tray.design_capacity))
    else {
        return;
    };
    let caps = if let Some(caps) = known_caps {
        caps
    } else {
        let Ok(names) = proxy.get_capabilities().await else {
            return;
        };
        Capabilities::from_probe(&names)
    };
    let battery = if caps.has(Capability::BatteryState) {
        proxy.get_battery_state().await.ok()
    } else {
        None
    };
    let limit = if caps.has(Capability::ChargeLimit) {
        proxy.get_charge_limit().await.ok()
    } else {
        None
    };
    let mut capacity = known_capacity;
    let mut speed = None;
    if caps.has(Capability::ChargeCurrentLimit) {
        if capacity.is_none() {
            capacity = proxy.get_battery_design_capacity().await.ok();
        }
        if capacity.is_some() {
            speed = proxy.get_charge_current_limit().await.ok();
        }
    }
    let level = if caps.has(Capability::FpBrightness) {
        proxy
            .get_fingerprint_brightness()
            .await
            .ok()
            .map(|(_, level)| level)
    } else {
        None
    };
    tray_push(
        handle,
        TrayValues {
            caps: Some(caps),
            battery,
            charge_limit: limit,
            design_capacity: capacity,
            charge_current_limit: speed,
            fp_level: level,
        },
    );
}
