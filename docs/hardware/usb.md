# Camera and fingerprint reader over USB

Both modules sit on the machine's internal USB bus and are read from
`/sys/bus/usb/devices`: `idVendor`, `idProduct`, `manufacturer`, `product`,
`serial` and `bcdDevice`, all world-readable. The read is
`hardware/src/usb.rs`; the two devices are `hardware/src/device/camera.rs`
and `hardware/src/device/fingerprint.rs`, each matching one vendor's ids
and building its identity through `part::of_usb`.

Each section holds what the kernel, `framework_lib` or this code
establishes, then under **Observed** what was read. The Laptop 13 Pro
(Intel Core Ultra Series 3) was read on the machine; other boards were read
from published probe logs, and each row names the board.

## Contents

Every heading in the file appears here.

- [What a device announces](#what-a-device-announces)
- [Camera](#camera)
- [Fingerprint reader](#fingerprint-reader)
- [Open](#open)
- [Sources](#sources)

## What a device announces

| Attribute | Becomes | Note |
|---|---|---|
| `idVendor`, `idProduct` | the part's id, `usb:<vid>:<pid>` | names the model, not the unit |
| `manufacturer` | the vendor's name | the device's own wording |
| `product` | the model | the device's own wording |
| `serial` | the serial | |
| `bcdDevice` | the firmware version | `usb::release` spells `0111` as `1.1.1`, as `framework_tool` does; `lsusb` writes the same bytes `1.11` |

- `framework_lib` reads the product string and the release through libusb
  behind its `rusb` feature; here they are two sysfs files.

## Camera

| Fact | Detail |
|---|---|
| Vendor id | `32ac`, Framework's own |
| Product ids | `001c` on the Laptop 13 and Laptop 16, `001d` on the Laptop 12; `WEBCAM_PIDS` in `camera.rs` |
| `product` | the marketplace wording for the module |
| `serial` | a Framework serial whose first six characters are the listing's variant code |
| `bcdDevice` | the module's firmware release |

- `Camera::detect` matches Framework's ids and no other: a camera of
  another make announces nothing that names a Framework listing.

### Observed

| Board | Ids | `product` | `bcdDevice` |
|---|---|---|---|
| Laptop 13 Pro | `32ac:001c` | `Laptop Webcam Module (2nd Gen)` | `0111`, shown as 1.1.1 |
| Laptop 12 | `32ac:001d` | | |

## Fingerprint reader

| Fact | Detail |
|---|---|
| Vendor id | `27c6`, Goodix |
| Product id | `609c`; `READER_PID` in `fingerprint.rs` |
| `product` | the maker's wording, naming no Framework listing |
| `serial` | the sensor's own identifier, not a Framework one |
| Fitted to | the power button, whose LED the EC's fingerprint commands drive; see [the power LED](led.md#power-led-levels) |

- Framework sells the reader as a kit per machine, and one Goodix id covers
  the Laptop 13 and the Laptop 16. Consequence: what the reader announces
  cannot name the kit.
- The Laptop 12 is the exception twice: the 13th Gen Intel board has no
  reader, and the Core Series 3 refresh carries a FocalTech sensor, vendor
  id `2808`. libfprint 1.94.100 added four FocalTech product ids, and which
  is the Laptop 12's is not published.

### Observed

| Board | Ids | `product` | `serial` | `bcdDevice` |
|---|---|---|---|---|
| Laptop 13 Pro | `27c6:609c` | `Goodix Fingerprint USB Device` | `UID…_MOC_B0` | `0100`, shown as 1.0.0 |
| Laptop 13, 11th Gen Intel Core through AMD Ryzen AI 300; Laptop 16 | `27c6:609c` | | | |

- No other Goodix product id appears in any Framework probe log read.

## Open

- What the webcam modules of the 11th to 13th Gen Intel Laptop 13 announce,
  and whether they are on Framework's vendor id at all.
- The Laptop 12 Core Series 3 reader's FocalTech product id; an `lsusb`
  from that machine settles it.

## Sources

- The Linux kernel's USB device sysfs attributes, as
  `drivers/usb/core/sysfs.c` creates them.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_lib/src/camera.rs` for the webcam ids and the release
  spelling.
- libfprint 1.94.100's release notes, for the FocalTech ids.
- Probe logs of Framework laptops published at linux-hardware.org — the ids
  of every board other than the Laptop 13 Pro.
