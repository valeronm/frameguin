//! The tray icon (`StatusNotifierItem`; shown by GNOME's `AppIndicator`
//! extension) and the values its menu renders from.
//!
//! It draws with no window open, so it keeps its own copy of everything the
//! menu names, pushed in by whoever last read or wrote it.

use std::rc::Rc;

use frameguin_model::control::Controls;
use frameguin_model::control::battery::{
    charge_limit_at, charge_limit_labels, charge_limit_preset_row, charge_speed_at,
    charge_speed_names, charge_speed_preset_row,
    reading::{amps, battery_summary, percent_label},
};
use frameguin_model::control::ports::supply_summary;
use frameguin_model::control::power_led;
use frameguin_model::control::touchscreen::{state_at, state_labels, state_row};
use frameguin_wire::{BatteryFeature, BatteryState, PortState, PowerLedLevel};

use crate::APP_ID;
use crate::board;
use crate::bus::Bus;
use crate::daemon::Daemon;

/// `Copy` so a menu item can be built from the event it sends: the boxed
/// handler runs more than once, and every variant here is a value rather
/// than something owned.
#[derive(Clone, Copy)]
pub(crate) enum TrayEvent {
    Show,
    /// The battery report, which the menu's own reading heads. Carries
    /// nothing: the report reads for itself rather than being handed the
    /// summary the menu happens to hold.
    ShowBatteryDetails,
    /// The USB-C ports report, which the menu's supply line heads. Carries
    /// nothing, as [`TrayEvent::ShowBatteryDetails`] does.
    ShowPorts,
    Refresh,
    SetChargeLimit(u8),
    /// Already resolved to milliamps against the capacity the menu was drawn
    /// from, so applying it needs nothing the window has to supply.
    SetChargeSpeed(u32),
    SetPowerLedLevel(PowerLedLevel),
    /// The state to move to, which is what the row that was clicked names.
    /// The menu's mark can be a moment stale — the pad is where the truth is
    /// and a suspend moves it — so "off" has to still mean off when it
    /// arrives, rather than inverting whatever the app believes by then.
    SetTouchscreen(bool),
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
    /// Which limits the charger takes, pushed in with the detected controls;
    /// None until then, which a machine with no pack never changes.
    battery_features: Option<Vec<BatteryFeature>>,
    /// The USB-C ports as the daemon last reported them, pushed in from the
    /// app like the pack above and moving on their own for the same reason.
    ports: Option<Vec<PortState>>,
    /// Currently applied charge limit, pushed in from the app so the radio
    /// group can mark it; None until the first daemon read.
    charge_limit: Option<u8>,
    /// The charge current limit in mA and the battery capacity that names the
    /// speeds, both pushed in from the app. Without the capacity the submenu
    /// stays out: a fraction then has no rate to show or to send.
    charge_current_limit: Option<u32>,
    design_capacity: Option<u32>,
    /// The levels the LED's board has that a click can apply, pushed in with
    /// the detected controls; None until then, which a machine with no LED to
    /// set never changes.
    power_led_presets: Option<Vec<PowerLedLevel>>,
    /// Current power button LED level, pushed in from the app; Custom marks no
    /// radio option.
    power_led_level: Option<PowerLedLevel>,
    /// Whether the touch panel is on, pushed in from the app; None until the
    /// first read, which a machine with no panel to switch never makes.
    touchscreen: Option<bool>,
}

impl TrayIcon {
    /// Everything the menu draws from arrives later, over `tray_push`.
    pub(crate) fn new(tx: async_channel::Sender<TrayEvent>) -> Self {
        Self {
            tx,
            battery: None,
            battery_features: None,
            ports: None,
            charge_limit: None,
            charge_current_limit: None,
            design_capacity: None,
            power_led_presets: None,
            power_led_level: None,
            touchscreen: None,
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
    /// Asking is all it can do: ksni publishes the menu the moment this
    /// returns, so the values land after the menu that needed them and the
    /// open menu keeps whatever it drew with. They are in place for the next
    /// open, which is why a control something else moved — the touchscreen
    /// across a lid, a limit the EC's extender lowered — reads one menu
    /// behind rather than staying wrong.
    fn menu_about_to_show(&mut self) {
        self.send(TrayEvent::Refresh);
    }

    /// The menu, grouped by subsystem, in the order the window's own page
    /// puts them — what is coming in, then the pack's own reading, then the
    /// controls that shape it, with the way in and the way out bracketing the
    /// lot. The array
    /// below is the list; naming the groups here as well would be a second
    /// place to update every time one is added.
    ///
    /// Each item decides for itself whether this board can offer it, so the
    /// separators are placed by asking which groups came back with anything
    /// rather than by restating those conditions here — a board with no
    /// battery controls draws no separator around the gap where they would
    /// have been.
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let groups: [Vec<ksni::MenuItem<Self>>; 5] = [
            vec![
                StandardItem {
                    label: "Open".into(),
                    activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::Show)),
                    ..Default::default()
                }
                .into(),
            ],
            [
                self.supply_item(),
                self.battery_item(),
                self.charge_limit_item(),
                self.charge_speed_item(),
            ]
            .into_iter()
            .flatten()
            .collect(),
            self.power_led_level_item().into_iter().collect(),
            self.touchscreen_item().into_iter().collect(),
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
/// value in force, holding a radio group over the presets. `selected` is None
/// when the hardware sits on no preset, which leaves the group unmarked and
/// the title naming `unlisted` — the value's own spelling, where the control
/// has one, and nothing where it hasn't or nothing has been read yet.
/// One shape for every line that shows a reading and opens the report behind
/// it, beside [`radio_submenu`] for the same reason: the convention is that
/// such a line carries no state of its own and the report reads for itself.
fn report_item(label: String, open: TrayEvent) -> ksni::MenuItem<TrayIcon> {
    ksni::menu::StandardItem {
        label,
        activate: Box::new(move |tray: &mut TrayIcon| tray.send(open)),
        ..Default::default()
    }
    .into()
}

fn radio_submenu(
    name: &str,
    selected: Option<usize>,
    unlisted: Option<&str>,
    labels: Vec<String>,
    select: impl Fn(&mut TrayIcon, usize) + Send + 'static,
) -> ksni::MenuItem<TrayIcon> {
    use ksni::menu::{RadioGroup, RadioItem, SubMenu};
    let current = selected.map(|row| labels[row].as_str()).or(unlisted);
    SubMenu {
        label: current.map_or_else(|| name.to_string(), |current| format!("{name} ({current})")),
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
    /// The reading heading the battery group, and the way into the full
    /// report. Gated on the reading itself: a board that has a pack still has
    /// nothing to show until the first one arrives.
    ///
    /// Unnamed, unlike the supply below it: a percentage and a direction say
    /// what they are, where a bare "Disconnected" would not.
    fn battery_item(&self) -> Option<ksni::MenuItem<Self>> {
        Some(report_item(
            battery_summary(self.battery?),
            TrayEvent::ShowBatteryDetails,
        ))
    }

    /// What is powering the machine, and the way into the ports report.
    /// Gated on a reading, as the battery's line is: a board with ports still
    /// has nothing to say about them until one arrives.
    ///
    /// Above the pack's own line, as the window's row is above its Status,
    /// and reading the board rather than being pushed it: the product name is
    /// settled once for the process and answers on any thread, where pushing
    /// it would be a field that never changes travelling the push protocol.
    ///
    /// Named here, as the window names its row and as `radio_submenu`'s
    /// callers name theirs: what the line is about is the menu's to say, and
    /// a menu row has no title of its own to say it in.
    fn supply_item(&self) -> Option<ksni::MenuItem<Self>> {
        let supply = supply_summary(self.ports.as_ref()?, board::product());
        Some(report_item(
            format!("Charger: {supply}"),
            TrayEvent::ShowPorts,
        ))
    }

    fn offers(&self, feature: BatteryFeature) -> bool {
        self.battery_features
            .as_ref()
            .is_some_and(|features| features.contains(&feature))
    }

    fn charge_limit_item(&self) -> Option<ksni::MenuItem<Self>> {
        if !self.offers(BatteryFeature::ChargeLimit) {
            return None;
        }
        let labels = charge_limit_labels();
        let selected = self.charge_limit.and_then(charge_limit_preset_row);
        // A ceiling dialled in from the window sits on no row; the raw value
        // is named rather than dropped, as for the speed below.
        let unlisted = self.charge_limit.map(percent_label);
        Some(radio_submenu(
            "Charge limit",
            selected,
            unlisted.as_deref(),
            labels,
            |tray, row| {
                if let Some(percent) = charge_limit_at(row) {
                    tray.send(TrayEvent::SetChargeLimit(percent));
                }
            },
        ))
    }

    fn charge_speed_item(&self) -> Option<ksni::MenuItem<Self>> {
        if !self.offers(BatteryFeature::ChargeCurrentLimit) {
            return None;
        }
        // Still needed to turn the chosen speed into the milliamps the daemon
        // takes, even though the menu names speeds rather than currents.
        let design_capacity = self.design_capacity?;
        // Bare preset names, not the window's `charge_speed_labels`: those
        // carry the rate in brackets, which would nest inside the submenu
        // title's own brackets.
        let labels = charge_speed_names();
        let selected = self
            .charge_current_limit
            .and_then(|milliamps| charge_speed_preset_row(design_capacity, milliamps));
        // Named by its preset where there is one, and by the current itself
        // where there isn't — a menu that can only show presets would say
        // nothing at all about a limit dialled in from the window.
        let unlisted = self.charge_current_limit.map(amps);
        Some(radio_submenu(
            "Charge speed",
            selected,
            unlisted.as_deref(),
            labels,
            move |tray, row| {
                if let Some(milliamps) = charge_speed_at(design_capacity, row) {
                    tray.send(TrayEvent::SetChargeSpeed(milliamps));
                }
            },
        ))
    }

    /// Gated on the presets having arrived: the LED's device answers for
    /// itself, and which levels it has is part of that answer.
    fn power_led_level_item(&self) -> Option<ksni::MenuItem<Self>> {
        let levels = self.power_led_presets.clone()?;
        let selected = self
            .power_led_level
            .and_then(|level| levels.iter().position(|l| *l == level));
        let options = power_led::labels(&levels);
        Some(radio_submenu(
            "Power button LED",
            selected,
            None,
            options,
            move |tray, row| {
                tray.send(TrayEvent::SetPowerLedLevel(levels[row]));
            },
        ))
    }

    /// Two states named as presets, through the same submenu the rest use.
    /// A checkmark would say as much in less room, but it draws in a column
    /// the submenus around it do not have, so the row sits out of line with
    /// every other control. Gated on a reading: the panel's device answers
    /// for itself, and the app hears of it by reading it.
    fn touchscreen_item(&self) -> Option<ksni::MenuItem<Self>> {
        let enabled = self.touchscreen?;
        let options = state_labels();
        let row = state_row(enabled)?;
        Some(radio_submenu(
            "Touchscreen",
            Some(row),
            None,
            options,
            |tray, row| {
                if let Some(enabled) = state_at(row) {
                    tray.send(TrayEvent::SetTouchscreen(enabled));
                }
            },
        ))
    }
}

/// What a caller knows about the tray's state. A field left None is one this
/// caller cannot speak for, and the menu keeps what it already holds — a
/// write speaks for its own value and nothing else, and the capacity the
/// speeds resolve against comes only from a reload or the tray's own read.
#[derive(Clone, Default)]
pub(crate) struct TrayValues {
    pub(crate) battery: Option<BatteryState>,
    pub(crate) battery_features: Option<Vec<BatteryFeature>>,
    pub(crate) ports: Option<Vec<PortState>>,
    pub(crate) charge_limit: Option<u8>,
    pub(crate) design_capacity: Option<u32>,
    pub(crate) charge_current_limit: Option<u32>,
    pub(crate) power_led_presets: Option<Vec<PowerLedLevel>>,
    pub(crate) power_led_level: Option<PowerLedLevel>,
    pub(crate) touchscreen: Option<bool>,
}

impl TrayValues {
    /// What the board offers: what each detected control has to say about
    /// its rows. Pushed once, the answer being fixed for the daemon's run.
    pub(crate) fn offered(controls: &Controls<Bus>) -> Self {
        Self {
            battery_features: controls
                .battery
                .as_ref()
                .map(|battery| battery.features().to_vec()),
            power_led_presets: controls.power_led.as_ref().map(|led| led.presets()),
            ..Self::default()
        }
    }

    pub(crate) fn charge_limit(percent: u8) -> Self {
        Self {
            charge_limit: Some(percent),
            ..Self::default()
        }
    }

    pub(crate) fn charge_speed(milliamps: u32) -> Self {
        Self {
            charge_current_limit: Some(milliamps),
            ..Self::default()
        }
    }

    pub(crate) fn power_led_level(level: PowerLedLevel) -> Self {
        Self {
            power_led_level: Some(level),
            ..Self::default()
        }
    }

    pub(crate) fn touchscreen(enabled: bool) -> Self {
        Self {
            touchscreen: Some(enabled),
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
        tray.battery = values.battery.or(tray.battery);
        tray.battery_features = values.battery_features.or(tray.battery_features.take());
        tray.ports = values.ports.or(tray.ports.take());
        tray.charge_limit = values.charge_limit.or(tray.charge_limit);
        tray.design_capacity = values.design_capacity.or(tray.design_capacity);
        tray.charge_current_limit = values.charge_current_limit.or(tray.charge_current_limit);
        tray.power_led_presets = values.power_led_presets.or(tray.power_led_presets.take());
        tray.power_led_level = values.power_led_level.or(tray.power_led_level);
        tray.touchscreen = values.touchscreen.or(tray.touchscreen);
    });
}

/// Reads the values the menu renders from. The tray keeps its own copies
/// because it has to draw with no window open, so they are pulled from the
/// daemon rather than from the window's widgets.
pub(crate) async fn refresh_tray(handle: &ksni::blocking::Handle<TrayIcon>, daemon: &Rc<Daemon>) {
    // One write at the end. Every `update` blocks this thread on the tray's
    // own, and makes it rebuild the entire menu and signal it over D-Bus, so
    // a field-at-a-time refresh would do that once per field.
    // `Daemon`'s answer rather than a detection of the tray's own, and asked
    // unconditionally: it is a cached value after the first ask, where reading
    // the menu's copy would cost a hop onto ksni's thread to save nothing. The
    // menu keeps a copy because it draws over there, not because it caches.
    let Ok(controls) = daemon.controls().await else {
        return;
    };
    let mut values = TrayValues::offered(&controls);
    if let Some(pack) = &controls.battery {
        // The one block read in the app that does not go through the feed:
        // the feed's may pull the pack's condition with it, which a menu
        // opening cannot wait on. A second walk of the memmap is the cheaper
        // half of that trade.
        let info = pack.read().await.ok();
        values.battery = info.as_ref().map(|info| info.state);
        values.design_capacity = info.as_ref().map(|info| info.design_capacity);
        if pack.has(BatteryFeature::ChargeLimit) {
            values.charge_limit = pack.charge_limit().await.ok();
        }
        // Without a capacity the speeds have no rate to name, so the menu
        // leaves the submenu out and the limit goes unasked.
        if pack.has(BatteryFeature::ChargeCurrentLimit) && values.design_capacity.is_some() {
            values.charge_current_limit = pack.charge_current_limit().await.ok();
        }
    }
    if let Some(ports) = &controls.ports {
        values.ports = ports.read().await.ok();
    }
    if let Some(led) = &controls.power_led {
        values.power_led_level = led.read().await.ok().map(|snapshot| snapshot.level);
    }
    if let Some(touchscreen) = &controls.touchscreen {
        values.touchscreen = touchscreen.read().await.ok();
    }
    tray_push(handle, values);
}
