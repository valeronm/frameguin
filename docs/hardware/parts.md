# Parts

This file lists every kind of part detection finds, and the transport each
is found over.

A part is something a person bought and can replace as a unit, which is
[`architecture.md`'s definition](../architecture.md#vocabulary). The processor and
the EC are soldered and cannot be ordered separately, so they are reported as
rows on the mainboard rather than as parts of their own.

Each transport has a file in this folder holding how detection reads it
and what every part found over it reports. [`controls.md`](controls.md) is
the matching index by control.

| Part | Transport |
|---|---|
| **Mainboard** | [`dmi`](dmi.md#board-detection) |
| **Memory** | [`dmi`](dmi.md#memory-detection) |
| **Battery** | [`ec`](battery.md) |
| **Storage** | [`nvme`](nvme.md) |
| **Wi-Fi** | [`wireless`](wireless.md) |
| **Display** | [`drm`](drm.md) |
| **Camera** | [`usb`](usb.md#camera) |
| **Fingerprint reader** | [`usb`](usb.md#fingerprint-reader) |
| **Touchpad** | [`touchpad`](touchpad.md) |

## The transports

Each is a module in `hardware/`, whose crate doc lists what every one of them
reaches and how — that list lives there alone, so a module added cannot
leave a copy here stale.

Two carry a fact the column above cannot. `dmi` reads the mainboard from
fields the kernel publishes world-readable and the memory from raw
structures it keeps root-only, so an unprivileged process can identify the
board and sees no memory. The battery uses two of the EC's three routes: the
memory map for its block, and I²C passthrough for the pack's own registers.

## Transport notes

**Two transports need a full bus walk.** Building a `hidapi::HidApi`
enumerates the whole HID bus, and the USB bus is walked in one pass as well.
`detect()` performs each walk once and passes the result to the devices that
need it: the touchpad and the touch panel for HID, the camera and the
fingerprint reader for USB.

**Two transports find two parts each.** `dmi` finds the mainboard and the
memory, at the two access levels above. `usb` finds the camera and the
fingerprint reader. The camera reports Framework's own vendor id and the
marketplace product name; the fingerprint reader reports only Goodix's id and
nothing that identifies a Framework listing.

**The mainboard is the only part that is also a controller.** It carries the
EC, the processor and the PD controllers, so every control in the project is
reached through a component on it. Its identity carries the BIOS, EC and PD
controller firmware versions.

## Announced but not a part

Detection reads several things it does not list as parts.

- **Expansion cards.** They report the names they are sold under, under
  Framework's USB vendor id, and are listed as
  [attached devices](usb-c.md#attached-devices) rather than parts. A
  card is a thing in a slot rather than a component of the machine, and the
  same reading covers a third-party device in the same slot.
- **Disks and network interfaces behind a USB bridge.** These sit under the
  bridge's USB interface and are read there. A storage expansion card is
  therefore not a Storage part: it enumerates as SCSI and does not appear in
  the NVMe class.
- **The PD controllers.** They are reported as firmware versions on the
  mainboard. Nothing orderable sits behind them, except on the Laptop 16,
  where the third controller is on the expansion bay module.
- **Bluetooth.** On a CNVi board it is the same silicon as the Wi-Fi radio and
  equally anonymous. No USB companion on the bus identifies a module.
- **The audio board.** It answers an ID resistor during the EC's input-deck
  detection. That is a presence reading, not an identity.

## Gaps

**The mainboard's identity spans two transports.** Its board and BIOS come
over `dmi`, its EC and PD controller firmware versions over `ec`, which
[`ec.md`](ec.md#which-board-the-ec-tree-calls-this-machine)
carries and the table above cannot show in one row.

**The keyboard backlight has a file but no part and no control.** It is
read and set over the EC like the LEDs. Nothing in the project implements it,
so it appears in neither this list nor the controls.
