//! The firmware's ESRT, the table of what a UEFI capsule can update, which
//! the kernel keeps root-only under `/sys/firmware/efi/esrt`. The table is
//! read and its GUIDs matched by `framework_lib`.

use framework_lib::esrt::{self, FrameworkGuidKind};

use crate::pd_controller::controller_of;

/// Each USB4 retimer the table lists, as the number of the PD controller
/// whose ports it serves and its version, in that number's order. Empty
/// where the table is absent or withheld as much as where it lists none.
pub(crate) fn retimers() -> Vec<(u8, String)> {
    retimers_among(
        esrt::get_esrt()
            .into_iter()
            .flat_map(|table| table.entries)
            .map(|entry| (esrt::match_guid_kind(&entry.fw_class), entry.fw_version)),
    )
}

/// The version's fields are undocumented, so it is spelled whole.
fn retimers_among(entries: impl Iterator<Item = (FrameworkGuidKind, u32)>) -> Vec<(u8, String)> {
    let mut retimers: Vec<_> = entries
        .filter_map(|(kind, version)| {
            let controller = controller_of(retimer_port(&kind)?);
            Some((u8::try_from(controller).ok()?, format!("0x{version:X}")))
        })
        .collect();
    retimers.sort_unstable_by_key(|(controller, _)| *controller);
    retimers
}

/// The first of the pair of ports a retimer's entry is named for. Every
/// kind is listed so that one the library adds fails to build until it is
/// placed here.
fn retimer_port(kind: &FrameworkGuidKind) -> Option<u8> {
    use FrameworkGuidKind as Kind;
    match kind {
        Kind::TglRetimer01
        | Kind::AdlRetimer01
        | Kind::RplRetimer01
        | Kind::MtlRetimer01
        | Kind::PtlRetimer01
        | Kind::WclRetimer01 => Some(0),
        Kind::TglRetimer23
        | Kind::AdlRetimer23
        | Kind::RplRetimer23
        | Kind::MtlRetimer23
        | Kind::PtlRetimer23
        | Kind::WclRetimer23 => Some(2),
        Kind::TglBios
        | Kind::AdlBios
        | Kind::RplBios
        | Kind::MtlBios
        | Kind::Fw12RplBios
        | Kind::PtlBios
        | Kind::WclBios
        | Kind::RplCsme
        | Kind::RplUCsme
        | Kind::MtlCsme
        | Kind::PtlCsme
        | Kind::WclCsme
        | Kind::Fl16Bios
        | Kind::Amd16Ai300Bios
        | Kind::Amd13Ryzen7040Bios
        | Kind::Amd13Ai300Bios
        | Kind::DesktopAmdAi300Bios
        | Kind::WinUx
        | Kind::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use framework_lib::esrt::FrameworkGuidKind;

    use super::retimers_among;

    #[test]
    fn retimers_are_listed_by_pd_controller_with_the_version_in_hex() {
        let table = [
            (FrameworkGuidKind::PtlRetimer23, 0x8_2802),
            (FrameworkGuidKind::PtlCsme, 0x5DF),
            (FrameworkGuidKind::PtlRetimer01, 0xAB),
            (FrameworkGuidKind::PtlBios, 0x304),
        ];
        assert_eq!(
            retimers_among(table.into_iter()),
            [(0, "0xAB".to_owned()), (1, "0x82802".to_owned())]
        );
    }
}
