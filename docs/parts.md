# Parts

This file lists every kind of part detection finds, and the transport each
is found over.

A part is something a person bought and can replace as a unit, which is
[`architecture.md`'s definition](architecture.md#vocabulary). The processor and
the EC are soldered and cannot be ordered separately, so they are reported as
rows on the mainboard rather than as parts of their own.

`hardware.md` documents what each part reports and how to read it. The Findings
column links to the relevant chapter.

| Part | Detection transport | Findings |
|---|---|---|
| **Mainboard** | [`dmi`](dmi.md#board-detection) | [Which board the EC tree calls this machine](hardware.md#which-board-the-ec-tree-calls-this-machine) |
| **Battery** | `ec` | [Battery](hardware.md#battery) |
| **Memory** | [`dmi`](dmi.md#memory-detection) | [Memory detection](dmi.md#memory-detection) |
| **Storage** | `nvme` | [Storage](hardware.md#storage) |
| **Wi-Fi** | `wireless` | [Wi-Fi](hardware.md#wi-fi) |
| **Display** | `drm` | [Display panel](hardware.md#display-panel) |
| **Camera** | `usb` | [Camera](hardware.md#camera) |
| **Touchpad** | `touchpad` | [Haptic touchpad](hardware.md#haptic-touchpad) |
| **Fingerprint reader** | `usb` | [Fingerprint reader](hardware.md#fingerprint-reader) |

## The transports

Each transport is a module in `hardware/`. The module name states how the
machine is reached.

- `dmi` — the firmware's SMBIOS table. It has two halves with different
  permissions. The kernel publishes some fields world-readable under
  `/sys/class/dmi/id`, and the mainboard is identified there. It keeps the raw
  structures under `/sys/firmware/dmi/entries` root-only, and the memory
  modules are there. An unprivileged process can identify the board but cannot
  see the memory.
- `drm` — the kernel's DRM class. The panel reports its own EDID block.
- `ec` — the embedded controller. It offers three routes and the battery uses
  two: the memory map carries the battery block, and I²C passthrough reaches
  the pack's own registers.
- `nvme` — the kernel's NVMe class. Only a drive in the board's M.2 slot
  appears here.
- `touchpad` — the touchpad's own HID transport. The EC is not involved.
- `usb` — the kernel's USB bus, including internal ports. The camera and the
  fingerprint reader are on an internal bus.
- `wireless` — the kernel's ieee80211 class. Each phy links to the PCI
  function its radio is driven on.

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
  [attached devices](hardware.md#what-sits-in-a-slot) rather than parts. A
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

**The mainboard has no chapter in `hardware.md`.** Nothing documents what the
DMI fields carry or whether a board number identifies a processor. The
Findings column points at the EC's version string because that is the closest
existing section, and [`dmi.md`](dmi.md#board-detection) covers the fields
themselves.

**The keyboard backlight has a chapter but no part and no control.** It is
read and set over the EC like the LEDs. Nothing in the project implements it,
so it appears in neither this list nor the controls.
