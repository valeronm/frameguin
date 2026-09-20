# Haptic touchpad over HID

The pad is reached over its own HID reports, through `framework_lib`, and
not through the EC. The transport is `hardware/src/touchpad.rs`; the device
and the mirror that answers for its two settings are
`hardware/src/device/touchpad.rs`.

Each section holds what `framework_lib`, the pad's descriptor or this code
establishes, then under **Observed** what was read on a machine. Every
observation is from the Laptop 13 Pro (Intel Core Ultra Series 3).

## Contents

Every heading in the file appears here.

- [Identity](#identity)
- [Settings](#settings)
- [Persistence](#persistence)
- [Open](#open)
- [Sources](#sources)

## Identity

| Id | Value | Names |
|---|---|---|
| HID vendor id | `093a` | PixArt Imaging, USB-IF registry; `PIX_VID` in `framework_lib` |
| HID product id | `1343` | the haptic pad; `HAPTIC_PIDS` in `touchpad.rs` |
| ACPI id | `PIXA3854` | the I²C device it enumerates under; the `PIXA` prefix is registered to Pixie Tech, an unrelated holder |
| Manufacturer string | empty | |
| Product string | `PIXA3854:00 093A:1343` | the I²C-HID device name, not a product |

- Consequence: the pad names no maker in words, and the id worth trusting
  is the HID vendor id. `Touchpad::detect` keys on it and the product id
  rather than on the board, so a haptic pad retrofitted into an older
  laptop is recognized.

## Settings

| Setting | Report | Values | Default |
|---|---|---|---|
| Haptic intensity | feature report `0x09` | `0, 25, 50, 75, 100`, `HAPTIC_INTENSITY_LEVELS`; the descriptor advertises 0 to 100 | 75, `DEFAULT_HAPTIC_INTENSITY` |
| Click force | a feature report | low, medium, high | medium, `DEFAULT_CLICK_FORCE` |

- Both are write-only: the firmware acknowledges `GET_FEATURE` with zeros
  rather than the current setting. Consequence: what is set is knowable only
  from the `Mirror` the device keeps, declared `Lifetime::Permanent`.
- The five steps are the pad's firmware, not the descriptor's;
  `framework_lib::set_haptic_intensity` refuses any other value.

### Observed

| Setup | Reading |
|---|---|
| `GET_FEATURE` on either report after a write | zeros |

## Persistence

| Event | Setting | Source |
|---|---|---|
| Suspend | kept | the pad keeps its settings in its own flash |
| Reboot | kept | same |
| EC restart | kept | the EC is not on the path |

- Nothing needs re-applying after a resume. That independence is no help to
  a host that forgot what it set: the interface answers nothing.

## Open

Nothing.

## Sources

- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_lib/src/touchpad.rs` for the vendor id, the report ids, the
  five intensity steps and the click forces.
- The USB-IF vendor id registry and udev's `20-acpi-vendor.hwdb`, for who
  holds `093a` and `PIXA`.
