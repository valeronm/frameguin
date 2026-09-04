//! The interfaces served to the `wire` proxies over a socket pair, polkit
//! answering as told.

use std::os::unix::net::UnixStream;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use async_io::Async;
use frameguin_hardware::device::battery::Battery;
use frameguin_hardware::device::ports::Ports;
use frameguin_hardware::device::power_led::PowerLed;
use frameguin_hardware::device::touchpad::Touchpad;
use frameguin_hardware::device::touchscreen::Touchscreen;
use frameguin_hardware::ec::Pack;
use frameguin_hardware::part::Identity;
use frameguin_hardware::testing::{
    Connectors, EC_BOOT, EcCharger, Gauge, Haptic, LedEc, Leds, Memory, Route, battery_identity,
    block, display_identity, mirrors, touchpad_identity,
};
use frameguin_wire::{
    BatteryFeature, ClickForce, DeviceError, FrameguinProxy, NO_CHARGE_CURRENT_LIMIT, PortPartner,
    PowerLedLevel, Proxies, proxy,
};
use futures_lite::future::{block_on, or};
use zbus::{Connection, Guid, connection};

use super::Devices;
use crate::Daemon;
use crate::service::Service;

fn devices() -> Devices {
    let store = Arc::new(Memory::default());
    let mirrors = mirrors(&store, Some(EC_BOOT), None);
    let gauge = Arc::new(Gauge::default());
    Devices {
        battery: Some(Battery::new(
            gauge.clone(),
            Arc::new(EcCharger::default()),
            &mirrors,
            gauge.identity().unwrap(),
        )),
        touchpad: Some(Touchpad::new(
            Box::new(Haptic::default()),
            &mirrors,
            touchpad_identity(),
        )),
        touchscreen: Some(Touchscreen::new(Box::new(Route::default()), &mirrors)),
        power_led: Some(PowerLed::new(
            Arc::new(LedEc::default()),
            Box::new(Leds::default()),
        )),
        ports: Ports::new(Arc::new(Connectors::default())),
    }
}

/// An inventory for the root interface to answer, which it holds verbatim.
fn parts() -> Vec<Identity> {
    vec![battery_identity(), touchpad_identity(), display_identity()]
}

/// The proxies dialled over one end of a socket pair, the other serving
/// every device.
struct Peer {
    proxies: Proxies,
    client: Connection,
}

/// A call over the pair unanswered for this long fails its test.
const CALL_DEADLINE: Duration = Duration::from_secs(10);

/// Runs `future` on `conn`'s own executor and waits for it over a channel:
/// a call polled on any other thread goes unanswered.
fn on_executor<T: Send + 'static>(
    conn: &Connection,
    future: impl Future<Output = T> + Send + 'static,
) -> T {
    let (tx, rx) = mpsc::channel();
    let _task = conn.executor().spawn(
        async move {
            let _ = tx.send(future.await);
        },
        "interface test",
    );
    match rx.recv_timeout(CALL_DEADLINE) {
        Ok(value) => value,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("a call over the pair went unanswered for {CALL_DEADLINE:?}")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the body panicked"),
    }
}

impl Peer {
    fn run<T: Send + 'static, F: Future<Output = T> + Send + 'static>(
        &self,
        body: impl FnOnce(Proxies) -> F,
    ) -> T {
        on_executor(&self.client, body(self.proxies.clone()))
    }
}

fn serve(authorized: bool) -> Peer {
    serve_devices(authorized, devices())
}

fn serve_devices(authorized: bool, devices: Devices) -> Peer {
    let (server_end, client_end) = UnixStream::pair().unwrap();
    let guid = Guid::generate();
    let end = |stream| {
        let stream = Async::new(stream).unwrap();
        connection::Builder::authenticated_socket(stream, guid.clone())
            .unwrap()
            .p2p()
            .internal_executor(false)
    };
    let server = block_on(end(server_end).build()).unwrap();
    let client = block_on(end(client_end).build()).unwrap();
    // zbus's own executor thread runs under async-io's `block_on`, which
    // contending with a second for the reactor misses the wakeup for a
    // message just written; this one parks on nothing but its waker.
    let (driven_server, driven_client) = (server.clone(), client.clone());
    std::thread::spawn(move || {
        block_on(async {
            loop {
                or(
                    driven_server.executor().tick(),
                    driven_client.executor().tick(),
                )
                .await;
            }
        });
    });
    let service = Arc::new(Service::answering(authorized));
    let serving = server.clone();
    let root = Daemon {
        service: service.clone(),
        parts: parts(),
    };
    on_executor(&server, async move {
        super::serve_all(serving.object_server(), root, devices).await
    })
    .unwrap();
    let dialling = client.clone();
    let proxies = on_executor(&client, async move { Proxies::dial(&dialling).await }).unwrap();
    Peer { proxies, client }
}

fn denied<T>(reply: zbus::Result<T>) -> bool {
    reply.is_err_and(|e| matches!(DeviceError::from(e), DeviceError::AccessDenied(_)))
}

fn invalid<T>(reply: zbus::Result<T>) -> bool {
    reply.is_err_and(|e| matches!(DeviceError::from(e), DeviceError::InvalidArgs(_)))
}

fn absent<T>(reply: zbus::Result<T>) -> bool {
    reply.is_err_and(|e| matches!(DeviceError::from(e), DeviceError::Absent(_)))
}

#[test]
fn every_getter_answers_through_its_proxy() {
    let peer = serve(true);
    peer.run(|p| async move {
        assert_eq!(p.battery.get_info().await.unwrap(), block());
        assert_eq!(p.battery.get_condition().await.unwrap().decicelsius, 301);
        assert_eq!(
            p.battery.get_features().await.unwrap(),
            [
                BatteryFeature::Condition,
                BatteryFeature::ChargeLimit,
                BatteryFeature::ChargeCurrentLimit
            ]
        );
        assert_eq!(p.battery.get_charge_limit().await.unwrap(), 100);
        assert_eq!(
            p.battery.get_charge_current_limit().await.unwrap(),
            NO_CHARGE_CURRENT_LIMIT
        );
        assert_eq!(
            p.power_led.get_brightness().await.unwrap(),
            (55, PowerLedLevel::High)
        );
        assert_eq!(p.power_led.get_levels().await.unwrap(), PowerLedLevel::ALL);
        assert_eq!(p.touchpad.get_haptic_intensity().await.unwrap(), 75);
        assert_eq!(
            p.touchpad.get_click_force().await.unwrap(),
            ClickForce::Medium
        );
        assert!(p.touchscreen.get_enabled().await.unwrap());
        let ports = p.ports.get_ports().await.unwrap();
        assert_eq!(ports.len(), 4);
        assert_eq!(ports[0].index, 0);
        assert!(ports[0].charging);
        assert_eq!(ports[0].partner, PortPartner::Source);
        assert!(!ports[1].charging);
        assert_eq!(ports[1].partner, PortPartner::Nothing);
    });
}

#[test]
fn every_setter_writes_when_polkit_allows() {
    let peer = serve(true);
    peer.run(|p| async move {
        assert!(p.battery.set_charge_limit(80).await.unwrap());
        assert_eq!(p.battery.get_charge_limit().await.unwrap(), 80);
        assert!(!p.battery.set_charge_limit(80).await.unwrap());
        assert!(p.battery.set_charge_current_limit(1_500).await.unwrap());
        assert_eq!(p.battery.get_charge_current_limit().await.unwrap(), 1_500);
        p.power_led.set_level(PowerLedLevel::Low).await.unwrap();
        assert_eq!(
            p.power_led.get_brightness().await.unwrap().1,
            PowerLedLevel::Low
        );
        p.power_led.set_brightness(20).await.unwrap();
        assert_eq!(
            p.power_led.get_brightness().await.unwrap(),
            (20, PowerLedLevel::Custom)
        );
        p.power_led.set_level(PowerLedLevel::Off).await.unwrap();
        assert_eq!(
            p.power_led.get_brightness().await.unwrap().1,
            PowerLedLevel::Off
        );
        p.touchpad.set_haptic_intensity(25).await.unwrap();
        assert_eq!(p.touchpad.get_haptic_intensity().await.unwrap(), 25);
        p.touchpad.set_click_force(ClickForce::High).await.unwrap();
        assert_eq!(
            p.touchpad.get_click_force().await.unwrap(),
            ClickForce::High
        );
        p.touchscreen.set_enabled(false).await.unwrap();
        assert!(!p.touchscreen.get_enabled().await.unwrap());
    });
}

#[test]
fn a_refused_write_leaves_the_device_untouched() {
    let peer = serve(false);
    peer.run(|p| async move {
        assert!(denied(p.battery.set_charge_limit(80).await));
        assert!(denied(p.battery.set_charge_current_limit(1_500).await));
        assert_eq!(p.battery.get_charge_limit().await.unwrap(), 100);
        assert_eq!(
            p.battery.get_charge_current_limit().await.unwrap(),
            NO_CHARGE_CURRENT_LIMIT
        );
        assert!(denied(p.power_led.set_level(PowerLedLevel::Low).await));
        assert!(denied(p.power_led.set_brightness(20).await));
        assert!(denied(p.power_led.set_level(PowerLedLevel::Off).await));
        assert_eq!(
            p.power_led.get_brightness().await.unwrap(),
            (55, PowerLedLevel::High)
        );
        assert!(denied(p.touchpad.set_haptic_intensity(25).await));
        assert!(denied(p.touchpad.set_click_force(ClickForce::High).await));
        assert_eq!(p.touchpad.get_haptic_intensity().await.unwrap(), 75);
        assert_eq!(
            p.touchpad.get_click_force().await.unwrap(),
            ClickForce::Medium
        );
        assert!(denied(p.touchscreen.set_enabled(false).await));
        assert!(p.touchscreen.get_enabled().await.unwrap());
    });
}

#[test]
fn a_bad_argument_and_a_write_in_place_never_reach_polkit() {
    let peer = serve(false);
    peer.run(|p| async move {
        assert!(invalid(p.battery.set_charge_limit(5).await));
        assert!(invalid(p.battery.set_charge_current_limit(0).await));
        assert!(!p.battery.set_charge_limit(100).await.unwrap());
        assert!(
            !p.battery
                .set_charge_current_limit(NO_CHARGE_CURRENT_LIMIT)
                .await
                .unwrap()
        );
        assert!(invalid(p.power_led.set_brightness(0).await));
        assert!(invalid(p.power_led.set_level(PowerLedLevel::Custom).await));
        assert!(invalid(p.touchpad.set_haptic_intensity(33).await));
        p.touchscreen.set_enabled(true).await.unwrap();
    });
}

#[test]
fn the_root_interface_answers_the_inventory_and_the_build() {
    let peer = serve(true);
    let conn = peer.client.clone();
    on_executor(&peer.client, async move {
        let daemon: FrameguinProxy = proxy(&conn).await.unwrap();
        assert_eq!(daemon.get_devices().await.unwrap(), parts());
        assert_eq!(
            daemon.get_build().await.unwrap().0,
            env!("CARGO_PKG_VERSION")
        );
    });
}

#[test]
fn a_device_detection_did_not_find_is_not_on_the_bus() {
    let peer = serve_devices(
        true,
        Devices {
            battery: None,
            ..devices()
        },
    );
    peer.run(|p| async move {
        assert!(absent(p.battery.get_charge_limit().await));
        assert!(p.touchscreen.get_enabled().await.is_ok());
        assert!(p.touchpad.get_click_force().await.is_ok());
        assert!(p.power_led.get_brightness().await.is_ok());
        assert!(p.ports.get_ports().await.is_ok());
    });
}

/// A board whose EC answers for no port serves no ports interface, which is
/// the same absence a machine without the command has.
#[test]
fn a_board_with_no_ports_serves_no_ports_interface() {
    let peer = serve_devices(
        true,
        Devices {
            ports: None,
            ..devices()
        },
    );
    peer.run(|p| async move {
        assert!(absent(p.ports.get_ports().await));
        assert!(p.battery.get_charge_limit().await.is_ok());
    });
}
