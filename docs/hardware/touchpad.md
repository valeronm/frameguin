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
- [Windows](#windows)
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
| Firmware version | registers `0xb2` (low) and `0xb3` (high), read through feature report `0x43` | fwupd's `pixart-tp` plugin, which reads it the same way |

- Consequence: the pad names no maker in words, and the id worth trusting
  is the HID vendor id. `Touchpad::detect` keys on it and the product id
  rather than on the board, so a haptic pad retrofitted into an older
  laptop is recognized.
- A register is read by sending `[0x43, address, 0x10, 0]` and then
  `GET_FEATURE` on `0x43`, whose last byte is the value.

### Observed

| Setup | Reading |
|---|---|
| `fwupdmgr get-devices` | firmware version `0x1310` |

## Settings

| Setting | Report | Values | Default |
|---|---|---|---|
| Haptic intensity | feature report `0x09` | `0, 25, 50, 75, 100`, `HAPTIC_INTENSITY_LEVELS`; the descriptor advertises 0 to 100 | 75, `DEFAULT_HAPTIC_INTENSITY` |
| Click force | feature report `0x08` | low, medium, high, codes `1, 2, 3`; the descriptor gives them as 110, 150 and 190 g | medium, `DEFAULT_CLICK_FORCE` |

- Both are write-only: the firmware answers `GET_FEATURE` with an empty
  report rather than the current setting. Consequence: what is set is
  knowable only from the `Mirror` the device keeps, declared
  `Lifetime::Permanent`.
- `framework_lib` states the five steps are the pad's firmware, not the
  descriptor's, and `set_haptic_intensity` refuses any other value.
- The descriptor's intensity collection holds the Intensity usage alone,
  `0x0E`/`0x23`, logical 0 to 100 in 8 bits: nothing in it marks the steps.

### Observed

| Setup | Reading |
|---|---|
| `HIDIOCGFEATURE` on either report, on the hidraw node | 0 bytes returned |

## Persistence

| Event | Setting | Source |
|---|---|---|
| Suspend | kept | the pad keeps its settings in its own flash |
| Reboot | kept | same |
| EC restart | kept | the EC is not on the path |
| Windows boot | replaced | Windows sends the pad its own saved settings; see [Windows](#windows) |

- Nothing needs re-applying after a resume. That independence is no help to
  a host that forgot what it set: the interface answers nothing.
- Consequence: on a machine that also boots Windows, the mirror holds only
  where the daemon writes it to the pad again at boot, which `Restore` does
  whether or not the restore switch is on.

### Observed

| Setup | Reading |
|---|---|
| Click force high and intensity 100 set from Linux, then Windows booted | the click light from Windows' first boot on, and still light back in Linux |

## Windows

What Windows' own touchpad settings send, from Microsoft's haptics
implementation and touchpad tuning guides:

| Setting | Windows value | Settings step | Reaches the pad as |
|---|---|---|---|
| Intensity, `FeedbackIntensity` | 0 to 100, default 50 | 25 | scaled linearly onto the descriptor's logical range, 0 meaning no feedback: 0, 25, 50, 75, 100 here |
| Click sensitivity, `ClickForceSensitivity` | 0 to 100, default 50 | 50 | the descriptor's logical minimum, default and maximum as low, medium and high: codes 1, 2, 3 here |

- The steps are Windows' for every touchpad, not read from the pad; the
  firmware's five intensity steps are the values Windows' step of 25 lands
  on over a 0 to 100 range.
- The guide requires a logical maximum of at least 4 for intensity, so a pad
  declaring 0 to 4 receives the same five settings as this one.
- Windows sends both settings whenever one changes, on a user switch, and
  when the pad enumerates or resets, which is why a Windows boot replaces
  what Linux set.

## Open

- Whether intensity takes values between the five steps. Framework's
  knowledge base gives the Linux write as any value from 0 to 100, against
  `framework_lib`'s statement that the firmware implements only the five.
- Whether the pad resets within a running session, and whether a reset
  returns it to its defaults: Windows sends its settings again on a reset,
  and the daemon resends only at boot and on resume.

## Sources

- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_lib/src/touchpad.rs` for the vendor id, the report ids, the
  five intensity steps and the click forces.
- The pad's report descriptor, for the click force's physical range and
  the intensity collection's contents.
- Microsoft's
  [Input Device Haptics Implementation Guide](https://learn.microsoft.com/en-us/windows-hardware/design/component-guidelines/haptic-touchpad-implementation-guide),
  for the intensity scaling, the button press threshold's three levels and
  when Windows sends the settings; and
  [Precision touchpad tuning](https://learn.microsoft.com/en-us/windows-hardware/design/component-guidelines/touchpad-tuning-guidelines),
  for the settings' ranges, defaults and steps.
- Framework's knowledge base,
  [How to adjust Haptic Touchpad Settings](https://knowledgebase.frame.work/how-to-adjust-haptic-touchpad-settings-BJjHJdU6bl),
  for the Linux writes and the 0 to 100 intensity range.
- fwupd's `plugins/pixart-tp`, for the register access and the version
  registers.
- The USB-IF vendor id registry and udev's `20-acpi-vendor.hwdb`, for who
  holds `093a` and `PIXA`.
