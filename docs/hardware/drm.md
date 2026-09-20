# Display panel over DRM

The panel is read from its connector under `/sys/class/drm`: the `status`
attribute for whether a panel is on it, and the `edid` attribute for the
panel's own block. Both are world-readable, so no root, no ioctl and no
compositor is involved. The read is `hardware/src/drm.rs`; the block's
decoding is `hardware/src/edid.rs`; the vendor's name for the PNP id comes
from udev's ACPI vendor list, `hardware/src/udev.rs`.

Each section holds what the kernel, the EDID specification or this code
establishes, then under **Observed** what was read. The Laptop 13 Pro
(Intel Core Ultra Series 3) was read on the machine; every other board's
panel was read from a published probe log, and its row names the board.

## Contents

Every heading in the file appears here.

- [Which connectors](#which-connectors)
- [What an EDID guarantees](#what-an-edid-guarantees)
- [Refresh rate](#refresh-rate)
- [Size](#size)
- [Date](#date)
- [Panels by board, observed](#panels-by-board-observed)
- [Open](#open)
- [Sources](#sources)

## Which connectors

- A connector named `eDP` is wired to the board; anything else is a monitor
  on a port. `drm::panels` keeps connectors whose name contains `-eDP-`.
- `status` is the kernel's answer to whether a panel is attached. `edid`
  reads empty where there is none, and also where a panel answered no
  block, so `status` is read first.
- i915 settles an eDP connector's `status` when it initializes the panel,
  not from the lid switch.

### Observed

| Setup | Reading |
|---|---|
| Laptop 13 Pro booted docked on a DisplayPort monitor, lid shut throughout | `card1-eDP-1` reads `connected` with its whole EDID; `enabled` reads `disabled`, the compositor having left the panel dark |

## What an EDID guarantees

| Field | Guaranteed | Where |
|---|---|---|
| Maker | yes | a three-letter PNP id in the header, naming nobody without a registry |
| Product code | yes | 16 bits in the header |
| Product name | no | an optional descriptor tagged `0xfc` |
| Serial number | no | an optional descriptor tagged `0xff` |
| Free-form text | no | descriptors tagged `0xfe`, any content |

- Consequence: a panel is always distinguishable, by maker and product
  code, and not always nameable.
- A model string may sit in a free-form descriptor and not in the
  product-name one, or in both, or in neither, and this varies between
  product codes of one model.
- The block may carry extensions after the 128-byte base. `edid::parse`
  reads the base block alone.

## Refresh rate

- The first detailed timing descriptor is the mode the panel prefers.
- A range-limits descriptor, tagged `0xfd`, carries the vertical rates the
  panel accepts. A panel need not carry one.
- Consequence: a panel quoted at its highest rate is quoted from the range
  limits or from a timing in an extension block, not from the preferred
  timing.

## Size

- The header carries the size in whole centimeters; the preferred timing
  carries it in millimeters. `edid::size` takes the timing's and falls back
  to the header's.
- The two differ by rounding. The Laptop 13 Pro's header reads 28 × 19 cm,
  a 13.3 inch diagonal; its timing reads 285 × 190 mm, the 13.5 inches the
  panel is sold as.

## Date

- The year byte counts from 1990. A zero byte is an unfilled field, not
  1990, and `edid::year` answers None for it.
- The week byte decides what the year means: `0xff` marks a model year, any
  other value leaves a manufacture year, with 0 meaning the week is not
  stated.
- Consequence: a model year and a manufacture year cannot be shown under
  one heading.

## Panels by board, observed

| Board | Maker, product | Model string | Where the model sits | Resolution | Size, mm | Range limits |
|---|---|---|---|---|---|---|
| Laptop 13 Pro | `CSW` `0x1322` | `MND508ZB1-1` | product-name descriptor; free-form reads `CSOT T3` | 2880 × 1920 | 285 × 190 | 30 to 120 Hz |
| Laptop 13 | `BOE` `0x095f` | `NE135FBM-N41` | free-form descriptor only | 2256 × 1504 | 285 × 190 | none |
| Laptop 13 | `BOE` `0x0cb4` | `NE135A1M-NY1` | product-name descriptor | 2880 × 1920 | 285 × 190 | 30 to 120 Hz |
| Laptop 12 | `BOE` `0x0d56` | `NV122WUM-N42` | product-name descriptor | 1920 × 1200 | 263 × 164 | 40 to 60 Hz |
| Laptop 16 | `BOE` `0x0bc9` | `NE160QDM-NZ6` | free-form descriptor, beside a second reading `BOE CQ` | 2560 × 1600 | 345 × 215 | none; 165 Hz as a detailed timing in an extension |
| Laptop 16 | `BOE` `0x0d79` | `NE160QDM-NZ6` | product-name descriptor | 2560 × 1600 | 345 × 215 | none; 165 Hz as a detailed timing in an extension |

- Every panel above prefers 60 Hz.
- No panel above carries a serial-number descriptor.
- The Laptop 16's `0x0d79` carries an adaptive-sync data block with fixed
  average and adaptive V-Total; its `0x0bc9` carries none.
- The Laptop 13 Pro's block is 384 bytes: the base and two extensions,
  CTA-861 and DisplayID. Its week reads 0 against a year of 2025.
- The PNP id `CSW` is CSOT's, in udev's ACPI vendor list as China Star
  Optoelectronics Technology.

## Open

- Which Laptop 13 board generations ship which of the two BOE panels.

## Sources

- VESA E-EDID standard, release A revision 2 — the base block layout, the
  descriptor tags, the week and year encoding, and the size fields.
- The Linux kernel's DRM connector sysfs attributes, `status`, `enabled`
  and `edid` under each connector of `/sys/class/drm`, as
  `drivers/gpu/drm/drm_sysfs.c` creates them; the eDP `status` behavior is
  the i915 driver's.
- udev's `20-acpi-vendor.hwdb` — the names for PNP ids.
- Probe logs of Framework laptops published at linux-hardware.org — the
  EDID blocks of every board other than the Laptop 13 Pro.
