//! The machines the fake daemon can answer as: `frameguin_hardware`'s own
//! devices over its stub roles, the stubs set from that crate's tables where
//! those settle a board and from `docs/hardware/boards.md` for the rest.

use std::sync::{Arc, Mutex};

use frameguin_contract::{Board, DeckState, PartKind, Platform, VENDOR};
use frameguin_hardware::device::battery::Battery;
use frameguin_hardware::device::charging_led::ChargingLed;
use frameguin_hardware::device::chassis::Chassis;
use frameguin_hardware::device::ports::Ports;
use frameguin_hardware::device::power_led::PowerLed;
use frameguin_hardware::device::privacy_switches::PrivacySwitches;
use frameguin_hardware::device::thermal::Thermal;
use frameguin_hardware::device::touchpad::Touchpad;
use frameguin_hardware::device::touchscreen::Touchscreen;
use frameguin_hardware::device::usb::Usb;
use frameguin_hardware::device::{Detected, Devices};
use frameguin_hardware::mirror::Mirrors;
use frameguin_hardware::part;
use frameguin_hardware::testing::{
    Connectors, Cover, EC_BOOT, EcCharger, Gauge, HOST_BOOT, Haptic, Hub, LedEc, Leds, Memory,
    Route, Sides, Sliders, Vents, battery_identity, display_identity, fans, mirrors, product,
    reader, touchpad_identity, webcam,
};

/// Which of a touch panel's two routes a board has: the pad's level reads
/// back, the panel's own command does not.
#[derive(Clone, Copy)]
enum Touch {
    Pad,
    Panel,
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is one thing a board has or lacks"
)]
struct Shape {
    platform: Platform,
    power_led_custom: bool,
    charging_led: bool,
    touch: Option<Touch>,
    haptic_touchpad: bool,
    pd_controllers: u8,
    pd_ports: u8,
    input_deck: bool,
    fan_duty: bool,
    second_fan: bool,
}

const BOARDS: &[(&str, Shape)] = &[
    (
        "laptop13-pro",
        Shape {
            platform: Platform::Laptop13ProUltra3,
            power_led_custom: true,
            charging_led: true,
            touch: Some(Touch::Pad),
            haptic_touchpad: true,
            pd_controllers: 2,
            pd_ports: 4,
            input_deck: true,
            fan_duty: true,
            second_fan: false,
        },
    ),
    (
        "laptop13-gen11",
        Shape {
            platform: Platform::Laptop13Gen11,
            power_led_custom: false,
            charging_led: false,
            touch: None,
            haptic_touchpad: false,
            pd_controllers: 0,
            pd_ports: 0,
            input_deck: false,
            fan_duty: false,
            second_fan: false,
        },
    ),
    (
        "laptop13-amd-ai300",
        Shape {
            platform: Platform::Laptop13AmdAi300,
            power_led_custom: true,
            charging_led: true,
            touch: None,
            haptic_touchpad: false,
            pd_controllers: 2,
            pd_ports: 4,
            input_deck: false,
            fan_duty: false,
            second_fan: false,
        },
    ),
    (
        "laptop12",
        Shape {
            platform: Platform::Laptop12Gen13,
            power_led_custom: true,
            charging_led: true,
            touch: Some(Touch::Panel),
            haptic_touchpad: false,
            pd_controllers: 2,
            pd_ports: 4,
            input_deck: false,
            fan_duty: true,
            second_fan: false,
        },
    ),
    (
        "laptop16",
        Shape {
            platform: Platform::Laptop16Amd7040,
            power_led_custom: true,
            charging_led: true,
            touch: None,
            haptic_touchpad: false,
            pd_controllers: 3,
            pd_ports: 5,
            input_deck: true,
            fan_duty: false,
            second_fan: true,
        },
    ),
];

const OTHER_VENDOR: &str = "other-vendor";

pub(crate) fn names() -> impl Iterator<Item = &'static str> {
    BOARDS
        .iter()
        .map(|(name, _)| *name)
        .chain(std::iter::once(OTHER_VENDOR))
}

pub(crate) fn detected(name: &str, refusing_writes: bool) -> Option<Detected> {
    if name == OTHER_VENDOR {
        return Some(other_vendor());
    }
    BOARDS
        .iter()
        .find(|(board, _)| *board == name)
        .map(|(_, shape)| machine(shape, refusing_writes))
}

/// Both holders answer, so a mirror keeps what it is given for the run.
fn holding() -> Mirrors {
    mirrors(&Arc::new(Memory::default()), Some(EC_BOOT), Some(HOST_BOOT))
}

fn machine(shape: &Shape, refusing: bool) -> Detected {
    let mirrors = holding();
    let devices = Devices {
        battery: Some(Battery::new(
            Arc::new(Gauge::default()),
            Arc::new(EcCharger {
                refusing,
                ..EcCharger::on(shape.platform)
            }),
            &mirrors,
            battery_identity(),
        )),
        touchpad: shape.haptic_touchpad.then(|| {
            Touchpad::new(
                Arc::new(Haptic {
                    refusing,
                    ..Haptic::default()
                }),
                &mirrors,
                touchpad_identity(),
            )
        }),
        touchscreen: shape.touch.map(|touch| {
            let level = match touch {
                Touch::Pad => Some(true),
                Touch::Panel => None,
            };
            Touchscreen::new(
                Box::new(Route {
                    level: Mutex::new(level),
                    refusing,
                    ..Route::default()
                }),
                &mirrors,
            )
        }),
        power_led: Some(PowerLed::new(
            Arc::new(LedEc {
                refusing,
                ..LedEc::on(shape.platform, shape.power_led_custom)
            }),
            Box::new(Leds::default()),
            &mirrors,
        )),
        charging_led: shape
            .charging_led
            .then(|| ChargingLed::new(Box::new(Leds::default()), Arc::new(Sides::default())))
            .flatten(),
        ports: Ports::new(
            Arc::new(Connectors {
                controllers: shape.pd_controllers,
                count: shape.pd_ports,
                ..Connectors::default()
            }),
            shape.platform,
        ),
        chassis: Chassis::new(Arc::new(Cover {
            deck: shape.input_deck.then_some(DeckState::On),
            ..Cover::default()
        })),
        privacy_switches: PrivacySwitches::new(Arc::new(Sliders::default())),
        thermal: Thermal::new(Arc::new(Vents {
            fans: fans(if shape.second_fan {
                &[2400, 2600]
            } else {
                &[2400]
            }),
            fan_duty: shape.fan_duty.then_some(45),
            ..Vents::default()
        })),
        usb: Usb::new(Arc::new(Hub::default())),
    };
    Detected {
        board: Board::new(
            VENDOR.to_owned(),
            product(shape.platform).to_owned(),
            shape.platform,
        ),
        devices,
        parts: parts(shape),
        restore: mirrors.restore(),
    }
}

/// The stubs' panel, camera and reader are the Laptop 13 Pro's.
fn parts(shape: &Shape) -> Vec<part::Identity> {
    let mut parts = vec![battery_identity()];
    if shape.haptic_touchpad {
        parts.push(touchpad_identity());
    }
    if shape.platform == Platform::Laptop13ProUltra3 {
        parts.push(display_identity());
        parts.push(part::of_usb(PartKind::Camera, &webcam()));
        parts.push(part::of_usb(PartKind::Fingerprint, &reader()));
    }
    parts
}

fn other_vendor() -> Detected {
    Detected {
        board: Board::new(
            "LENOVO".to_owned(),
            "ThinkPad X1 Carbon Gen 9".to_owned(),
            Platform::Unknown,
        ),
        devices: Devices {
            battery: None,
            touchpad: None,
            touchscreen: None,
            power_led: None,
            charging_led: None,
            ports: None,
            chassis: None,
            privacy_switches: None,
            thermal: None,
            usb: None,
        },
        parts: Vec::new(),
        restore: holding().restore(),
    }
}
