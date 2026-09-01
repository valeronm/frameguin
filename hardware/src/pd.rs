//! What the EC's cached PD controller state means — the version blob and one
//! port's state — apart from [`crate::ec`] so the decoding is testable
//! without an EC.

use frameguin_wire::{CcPolarity, DataRole, Epr, PortPartner, PortState, PowerRole};
use framework_lib::ccgx::AppVersion;
use framework_lib::chromium_ec::commands::EcResponseGetPdPortState;

/// Bits 0 and 1 of the alternate-mode byte, either of which is `DisplayPort`
/// connected in one direction or the other. The rest describe a link that
/// is already up — multi-function, hot plug detect — and say nothing about
/// whether there is one.
const DP_CONNECTED: u8 = 0b11;

/// One controller's version as the EC hands it over: four bytes of base
/// version, then four of application version.
pub const VERSION_LEN: usize = 8;

/// Where the application half starts.
const APPLICATION: usize = 4;

/// The application version, which is the one Framework's own tool prints for
/// a controller and the one that moves between firmware releases — the base
/// version tracks the silicon's own stack.
///
/// Laid out and spelled by `framework_lib`, so a correction to how the vendor
/// numbers a release reaches this by an upgrade rather than by someone
/// noticing two spellings. Its fields are hexadecimal as the vendor writes
/// them, so `1.0.0A` is a version and not a truncated decimal.
///
/// None for the all-zero blob, which is what the EC answers for a controller
/// that never reached its ready state — including one whose module is not
/// installed.
#[must_use]
pub fn version(blob: [u8; VERSION_LEN]) -> Option<String> {
    if blob.iter().all(|&byte| byte == 0) {
        return None;
    }
    Some(AppVersion::from(&blob[APPLICATION..]).to_string())
}

/// One port's state as the EC hands it over. `index` is the port number the
/// EC was asked for; `charging` comes from the EC's own answer about which
/// port is feeding the machine, which it reports per port rather than once.
#[must_use]
pub fn port_state(index: u8, raw: &EcResponseGetPdPortState) -> PortState {
    // Copied out one field at a time: the response is a packed struct, and a
    // reference into an unaligned field is undefined behaviour.
    let (millivolts, milliamps) = (raw.voltage, raw.current);
    PortState {
        index,
        partner: match raw.c_state {
            0 => PortPartner::Nothing,
            1 => PortPartner::Sink,
            2 => PortPartner::Source,
            3 => PortPartner::Debug,
            4 => PortPartner::Audio,
            5 => PortPartner::PoweredAccessory,
            6 => PortPartner::Unsupported,
            _ => PortPartner::Invalid,
        },
        contract: raw.pd_state != 0,
        power_role: match raw.power_role {
            0 => PowerRole::Sink,
            1 => PowerRole::Source,
            _ => PowerRole::Unknown,
        },
        data_role: match raw.data_role {
            0 => DataRole::UpstreamFacing,
            1 => DataRole::DownstreamFacing,
            2 => DataRole::Disconnected,
            _ => DataRole::Unknown,
        },
        millivolts,
        milliamps,
        charging: raw.active_port != 0,
        video: raw.pd_alt_mode_status & DP_CONNECTED != 0,
        vconn: raw.vconn != 0,
        cc: match raw.cc_polarity {
            0 => CcPolarity::Cc1,
            1 => CcPolarity::Cc2,
            2 => CcPolarity::Cc1Debug,
            3 => CcPolarity::Cc2Debug,
            _ => CcPolarity::Unknown,
        },
        epr: match (raw.epr_active != 0, raw.epr_support != 0) {
            (true, _) => Epr::Active,
            (false, true) => Epr::Supported,
            (false, false) => Epr::Unsupported,
        },
    }
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{CcPolarity, DataRole, PortPartner, PowerRole};
    use framework_lib::chromium_ec::commands::EcResponseGetPdPortState;

    use super::{port_state, version};

    /// A 90 W display on a port that is not the one charging, as the EC
    /// reported it: a source partner, `DisplayPort` up, hot plug detect high.
    fn display() -> EcResponseGetPdPortState {
        EcResponseGetPdPortState {
            c_state: 2,
            pd_state: 1,
            power_role: 0,
            data_role: 1,
            vconn: 1,
            epr_active: 0,
            epr_support: 0,
            cc_polarity: 0,
            voltage: 20_000,
            current: 4500,
            active_port: 0,
            pd_alt_mode_status: 0x82,
        }
    }

    #[test]
    fn a_negotiated_port_carries_what_it_settled_on() {
        let port = port_state(2, &display());
        assert_eq!(port.index, 2);
        assert_eq!(port.partner, PortPartner::Source);
        assert!(port.contract);
        assert_eq!(port.power_role, PowerRole::Sink);
        assert_eq!(port.data_role, DataRole::DownstreamFacing);
        assert_eq!((port.millivolts, port.milliamps), (20_000, 4500));
        assert_eq!(port.cc, CcPolarity::Cc1);
        assert!(port.vconn);
        assert!(port.video);
        assert!(!port.charging);
    }

    #[test]
    fn the_port_feeding_the_machine_is_the_one_marked_charging() {
        let raw = EcResponseGetPdPortState {
            active_port: 1,
            pd_alt_mode_status: 0,
            ..display()
        };
        let port = port_state(3, &raw);
        assert!(port.charging);
        assert!(!port.video);
    }

    /// Every bit but the two that mean connected: a link's own details
    /// describe a link that is up, and cannot be what says one is.
    #[test]
    fn alternate_mode_bits_short_of_connected_are_not_video() {
        let raw = EcResponseGetPdPortState {
            pd_alt_mode_status: 0xfc,
            ..display()
        };
        assert!(!port_state(0, &raw).video);
    }

    #[test]
    fn an_empty_port_says_so_through_its_partner() {
        let raw = EcResponseGetPdPortState {
            c_state: 0,
            pd_state: 0,
            voltage: 0,
            current: 0,
            ..display()
        };
        let port = port_state(0, &raw);
        assert_eq!(port.partner, PortPartner::Nothing);
        assert!(!port.contract);
    }

    #[test]
    fn the_application_version_is_read_off_the_blob() {
        assert_eq!(
            version(crate::testing::PD_VERSION).as_deref(),
            Some("1.0.0A")
        );
    }

    #[test]
    fn a_controller_the_ec_never_saw_has_no_version() {
        assert_eq!(version([0; 8]), None);
    }
}
