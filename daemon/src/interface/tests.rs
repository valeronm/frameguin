//! The interfaces served to the `wire` proxies over a socket pair, polkit
//! answering as told.

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use frameguin_hardware::device::battery::Battery;
use frameguin_hardware::device::power_led::PowerLed;
use frameguin_hardware::device::touchpad::Touchpad;
use frameguin_hardware::device::touchscreen::Touchscreen;
use frameguin_hardware::ec::Pack;
use frameguin_hardware::lifetime::EcBoot;
use frameguin_hardware::part;
use frameguin_hardware::testing::{
    EcCharger, Fp, Gauge, Leds, Log, Memory, Pad, Route, block, mirrors,
};
use frameguin_wire::{
    BatteryFeature, BatteryProxy, ClickForce, DeviceError, NO_CHARGE_CURRENT_LIMIT, PartKind,
    PowerLedLevel, PowerLedProxy, TouchpadProxy, TouchscreenProxy, proxy,
};
use zbus::{Connection, Guid, block_on, connection};

use super::Devices;
use crate::service::Service;

/// Every device over a role that takes every write; the touch panel on the
/// pad route, whose reading is what lets a write already in place skip.
fn devices() -> Devices {
    let store = Arc::new(Memory::default());
    let ec_boot = EcBoot::from_clocks(500_000, 1_000_000);
    let mirrors = mirrors(&store, Some(ec_boot), None);
    let gauge = Arc::new(Gauge { answering: true });
    let log: Log = Arc::default();
    Devices {
        touchpad: Some(Touchpad::new(
            Box::new(Pad { refusing: false }),
            &mirrors,
            part::hid(PartKind::Touchpad, 0x093a, 0x1343, "PixArt", "", ""),
        )),
        touchscreen: Some(Touchscreen::new(
            Box::new(Route {
                level: Mutex::new(Some(true)),
                refusing: false,
            }),
            &mirrors,
            part::hid(PartKind::Touchscreen, 0x2c68, 0x0100, "", "", ""),
        )),
        power_led: Some(PowerLed::new(
            Arc::new(Fp {
                level: Mutex::new((55, PowerLedLevel::High)),
                custom: true,
                refusing: false,
                log: log.clone(),
            }),
            Box::new(Leds {
                node: Some(PathBuf::from("/sys/class/leds/power")),
                dark: Mutex::new(false),
                log,
            }),
        )),
        battery: Some(Battery::new(
            gauge.clone(),
            Arc::new(EcCharger {
                limit: Mutex::new(100),
                caps: true,
                refusing: false,
                written: Mutex::new(Vec::new()),
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
