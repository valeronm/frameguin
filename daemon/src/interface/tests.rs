//! The interfaces served to the `wire` proxies over a socket pair, polkit
//! answering as told.

use std::collections::BTreeMap;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use frameguin_hardware::device::battery::Battery;
use frameguin_hardware::device::power_led::PowerLed;
use frameguin_hardware::device::touchpad::Touchpad;
use frameguin_hardware::device::touchscreen::Touchscreen;
use frameguin_hardware::ec::{Charger, Pack, PowerLedEc};
use frameguin_hardware::led::LedClass;
use frameguin_hardware::lifetime::{EcBoot, Holders};
use frameguin_hardware::mirror::Mirrors;
use frameguin_hardware::part;
use frameguin_hardware::state::Store;
use frameguin_hardware::touchpad::HapticPad;
use frameguin_hardware::touchscreen::TouchSwitch;
use frameguin_wire::{
    BatteryCondition, BatteryFeature, BatteryInfo, BatteryProxy, BatteryState, ChargeFlow,
    ClickForce, DeviceError, DeviceResult, Identity, NO_CHARGE_CURRENT_LIMIT, PartKind,
    PowerLedLevel, PowerLedProxy, TouchpadProxy, TouchscreenProxy, proxy,
};
use zbus::{Connection, Guid, block_on, connection};

use super::Devices;
use crate::service::Service;

#[derive(Default)]
struct Memory(Mutex<BTreeMap<String, String>>);

impl Store for Memory {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
    }

    fn set(&self, key: &str, value: Option<String>) {
        let mut entries = self.0.lock().unwrap();
        match value {
            Some(value) => entries.insert(key.to_owned(), value),
            None => entries.remove(key),
        };
    }
}

fn block() -> BatteryInfo {
    BatteryInfo {
        state: BatteryState {
            percent: 80,
            flow: ChargeFlow::Idle,
            milliamps: 0,
            millivolts: 15_000,
        },
        remaining_capacity: 3_000,
        last_full_capacity: 3_600,
        design_capacity: 3_900,
        design_millivolts: 15_400,
        cycle_count: 12,
        charger_connected: true,
        critical: false,
        manufacturer: "NVT".into(),
        model: "FRANGWA".into(),
        serial: "0001".into(),
        chemistry: "LION".into(),
        manufactured: "2026-01-01".into(),
    }
}

struct Gauge;

impl Pack for Gauge {
    fn identity(&self) -> Option<Identity> {
        Some(Identity {
            kind: PartKind::Battery,
            vendor: "NVT".into(),
            model: "FRANGWA".into(),
            serial: "0001".into(),
            id: "sbs:FRANGWA".into(),
            firmware: Vec::new(),
        })
    }

    fn info(&self) -> Option<BatteryInfo> {
        Some(block())
    }

    fn condition(&self) -> Option<BatteryCondition> {
        Some(BatteryCondition {
            cell_millivolts: vec![3_750; 4],
            alarms: Vec::new(),
            decicelsius: 301,
        })
    }
}

struct Ec {
    limit: Mutex<u8>,
}

impl Charger for Ec {
    fn charge_limit(&self) -> DeviceResult<u8> {
        Ok(*self.limit.lock().unwrap())
    }

    fn set_charge_limit(&self, percent: u8) -> DeviceResult<()> {
        *self.limit.lock().unwrap() = percent;
        Ok(())
    }

    fn set_charge_current_limit(&self, _milliamps: u32) -> DeviceResult<()> {
        Ok(())
    }

    fn charge_current_limit_supported(&self) -> bool {
        true
    }
}

struct Fp {
    level: Mutex<(u8, PowerLedLevel)>,
}

impl PowerLedEc for Fp {
    fn power_led_level(&self) -> DeviceResult<(u8, PowerLedLevel)> {
        Ok(*self.level.lock().unwrap())
    }

    fn set_power_led_level(&self, level: PowerLedLevel) -> DeviceResult<()> {
        self.level.lock().unwrap().1 = level;
        Ok(())
    }

    fn set_power_led_percentage(&self, percent: u8) -> DeviceResult<()> {
        *self.level.lock().unwrap() = (percent, PowerLedLevel::Custom);
        Ok(())
    }

    fn custom_power_led_levels(&self) -> bool {
        true
    }
}

struct Leds {
    dark: Mutex<bool>,
}

impl LedClass for Leds {
    fn controllable(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/sys/class/leds/power"))
    }

    fn held_dark(&self) -> Option<PathBuf> {
        self.dark
            .lock()
            .unwrap()
            .then(|| self.controllable())
            .flatten()
    }

    fn darken(&self, _dir: &Path) -> DeviceResult<()> {
        *self.dark.lock().unwrap() = true;
        Ok(())
    }

    fn release(&self, _dir: &Path) -> DeviceResult<()> {
        *self.dark.lock().unwrap() = false;
        Ok(())
    }
}

struct Pad;

impl HapticPad for Pad {
    fn set_haptic_intensity(&self, _percent: u8) -> DeviceResult<()> {
        Ok(())
    }

    fn set_click_force(&self, _force: ClickForce) -> DeviceResult<()> {
        Ok(())
    }
}

/// The pad route, whose reading is what lets a write already in place skip.
struct Route {
    enabled: Mutex<bool>,
}

impl TouchSwitch for Route {
    fn reading(&self) -> DeviceResult<Option<bool>> {
        Ok(Some(*self.enabled.lock().unwrap()))
    }

    fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        *self.enabled.lock().unwrap() = enabled;
        Ok(())
    }
}

fn devices() -> Devices {
    let store: Arc<dyn Store> = Arc::new(Memory::default());
    let ec_boot = EcBoot::from_clocks(500_000, 1_000_000);
    let mirrors = Mirrors::new(store, Holders::new(Some(ec_boot), None));
    let gauge = Arc::new(Gauge);
    Devices {
        touchpad: Some(Touchpad::new(
            Box::new(Pad),
            &mirrors,
            part::hid(PartKind::Touchpad, 0x093a, 0x1343, "PixArt", "", ""),
        )),
        touchscreen: Some(Touchscreen::new(
            Box::new(Route {
                enabled: Mutex::new(true),
            }),
            &mirrors,
            part::hid(PartKind::Touchscreen, 0x2c68, 0x0100, "", "", ""),
        )),
        power_led: Some(PowerLed::new(
            Arc::new(Fp {
                level: Mutex::new((55, PowerLedLevel::High)),
            }),
            Box::new(Leds {
                dark: Mutex::new(false),
            }),
        )),
        battery: Some(Battery::new(
            gauge.clone(),
            Arc::new(Ec {
                limit: Mutex::new(100),
            }),
            &mirrors,
            gauge.identity().unwrap(),
        )),
    }
}

/// The proxies, dialled as the app dials them, on one end of a socket pair
/// whose other end serves every device.
struct Peer {
    battery: BatteryProxy<'static>,
    led: PowerLedProxy<'static>,
    touchpad: TouchpadProxy<'static>,
    touchscreen: TouchscreenProxy<'static>,
    _server: Connection,
    _turn: MutexGuard<'static, ()>,
}

/// Two socket pairs alive on two threads at once can leave a connection
/// parked for good, so the tests here take their turn.
static TURN: Mutex<()> = Mutex::new(());

fn serve(authorized: bool) -> Peer {
    let turn = TURN.lock().unwrap_or_else(PoisonError::into_inner);
    let (server_end, client_end) = UnixStream::pair().unwrap();
    let guid = Guid::generate();
    let end = |stream| {
        let stream = async_io::Async::new(stream).unwrap();
        connection::Builder::authenticated_socket(stream, guid.clone())
            .unwrap()
            .p2p()
    };
    let server = block_on(end(server_end).build()).unwrap();
    let client = block_on(end(client_end).build()).unwrap();
    let service = Arc::new(Service::answering(authorized));
    block_on(async {
        devices().serve(server.object_server(), &service).await?;
        Ok::<_, zbus::Error>(Peer {
            battery: proxy(&client).await?,
            led: proxy(&client).await?,
            touchpad: proxy(&client).await?,
            touchscreen: proxy(&client).await?,
            _server: server,
            _turn: turn,
        })
    })
    .unwrap()
}

fn denied<T>(reply: zbus::Result<T>) -> bool {
    reply.is_err_and(|e| matches!(DeviceError::from(e), DeviceError::AccessDenied(_)))
}

fn invalid<T>(reply: zbus::Result<T>) -> bool {
    reply.is_err_and(|e| matches!(DeviceError::from(e), DeviceError::InvalidArgs(_)))
}

#[test]
fn every_getter_answers_through_its_proxy() {
    let peer = serve(true);
    block_on(async {
        assert_eq!(peer.battery.get_info().await.unwrap(), block());
        assert_eq!(peer.battery.get_condition().await.unwrap().decicelsius, 301);
        assert_eq!(
            peer.battery.get_features().await.unwrap(),
            [
                BatteryFeature::Condition,
                BatteryFeature::ChargeLimit,
                BatteryFeature::ChargeCurrentLimit
            ]
        );
        assert_eq!(peer.battery.get_charge_limit().await.unwrap(), 100);
        assert_eq!(
            peer.battery.get_charge_current_limit().await.unwrap(),
            NO_CHARGE_CURRENT_LIMIT
        );
        assert_eq!(
            peer.led.get_brightness().await.unwrap(),
            (55, PowerLedLevel::High)
        );
        assert_eq!(peer.led.get_levels().await.unwrap(), PowerLedLevel::ALL);
        assert_eq!(peer.touchpad.get_haptic_intensity().await.unwrap(), 75);
        assert_eq!(
            peer.touchpad.get_click_force().await.unwrap(),
            ClickForce::Medium
        );
        assert!(peer.touchscreen.get_enabled().await.unwrap());
    });
}

#[test]
fn every_setter_writes_when_polkit_allows() {
    let peer = serve(true);
    block_on(async {
        assert!(peer.battery.set_charge_limit(80).await.unwrap());
        assert_eq!(peer.battery.get_charge_limit().await.unwrap(), 80);
        assert!(!peer.battery.set_charge_limit(80).await.unwrap());
        assert!(peer.battery.set_charge_current_limit(1_500).await.unwrap());
        assert_eq!(
            peer.battery.get_charge_current_limit().await.unwrap(),
            1_500
        );
        peer.led.set_level(PowerLedLevel::Low).await.unwrap();
        assert_eq!(
            peer.led.get_brightness().await.unwrap().1,
            PowerLedLevel::Low
        );
        peer.led.set_brightness(20).await.unwrap();
        assert_eq!(
            peer.led.get_brightness().await.unwrap(),
            (20, PowerLedLevel::Custom)
        );
        peer.led.set_level(PowerLedLevel::Off).await.unwrap();
        assert_eq!(
            peer.led.get_brightness().await.unwrap().1,
            PowerLedLevel::Off
        );
        peer.touchpad.set_haptic_intensity(25).await.unwrap();
        assert_eq!(peer.touchpad.get_haptic_intensity().await.unwrap(), 25);
        peer.touchpad
            .set_click_force(ClickForce::High)
            .await
            .unwrap();
        assert_eq!(
            peer.touchpad.get_click_force().await.unwrap(),
            ClickForce::High
        );
        peer.touchscreen.set_enabled(false).await.unwrap();
        assert!(!peer.touchscreen.get_enabled().await.unwrap());
    });
}

#[test]
fn a_refused_write_leaves_the_device_untouched() {
    let peer = serve(false);
    block_on(async {
        assert!(denied(peer.battery.set_charge_limit(80).await));
        assert!(denied(peer.battery.set_charge_current_limit(1_500).await));
        assert_eq!(peer.battery.get_charge_limit().await.unwrap(), 100);
        assert_eq!(
            peer.battery.get_charge_current_limit().await.unwrap(),
            NO_CHARGE_CURRENT_LIMIT
        );
        assert!(denied(peer.led.set_level(PowerLedLevel::Low).await));
        assert!(denied(peer.led.set_brightness(20).await));
        assert!(denied(peer.led.set_level(PowerLedLevel::Off).await));
        assert_eq!(
            peer.led.get_brightness().await.unwrap(),
            (55, PowerLedLevel::High)
        );
        assert!(denied(peer.touchpad.set_haptic_intensity(25).await));
        assert!(denied(
            peer.touchpad.set_click_force(ClickForce::High).await
        ));
        assert_eq!(peer.touchpad.get_haptic_intensity().await.unwrap(), 75);
        assert_eq!(
            peer.touchpad.get_click_force().await.unwrap(),
            ClickForce::Medium
        );
        assert!(denied(peer.touchscreen.set_enabled(false).await));
        assert!(peer.touchscreen.get_enabled().await.unwrap());
    });
}

#[test]
fn a_bad_argument_and_a_write_in_place_never_reach_polkit() {
    let peer = serve(false);
    block_on(async {
        assert!(invalid(peer.battery.set_charge_limit(5).await));
        assert!(invalid(peer.battery.set_charge_current_limit(0).await));
        assert!(!peer.battery.set_charge_limit(100).await.unwrap());
        assert!(
            !peer
                .battery
                .set_charge_current_limit(NO_CHARGE_CURRENT_LIMIT)
                .await
                .unwrap()
        );
        assert!(invalid(peer.led.set_brightness(0).await));
        assert!(invalid(peer.led.set_level(PowerLedLevel::Custom).await));
        assert!(invalid(peer.touchpad.set_haptic_intensity(33).await));
        peer.touchscreen.set_enabled(true).await.unwrap();
    });
}
