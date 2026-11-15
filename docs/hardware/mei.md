# The Management Engine version

An Intel board's Management Engine reports its firmware version through the
kernel's `mei` class, `/sys/class/mei/<device>/fw_ver`, world-readable. The
read is `hardware/src/mei.rs`, and the version is listed as firmware on the
mainboard.

Each section holds what the kernel, `framework_lib` or this code
establishes, then under **Observed** what was read. Every observation is
from the Laptop 13 Pro (Intel Core Ultra Series 3).

## Contents

Every heading in the file appears here.

- [What the class carries](#what-the-class-carries)
- [Open](#open)
- [Sources](#sources)

## What the class carries

| Attribute | Reads |
|---|---|
| `kind` | `mei` for the Management Engine; a graphics card's firmware interface is listed in the same class under another kind |
| `fw_ver` | up to three lines, one per firmware block, each `<platform>:<version>` |
| `dev_state` | `ENABLED` once the interface is up |

- `mei::version` reads the first device whose `kind` is `mei`, and takes
  the first line of its `fw_ver` without the platform number.
- `framework_lib`'s `csme_from_sysfs` reads the three lines as the running
  firmware's version, the recovery image's and the FITC's, in that order.
- A board with no Management Engine has no device in the class, and its
  mainboard lists no such firmware.

### Observed

| Attribute | Reading |
|---|---|
| `kind` | `mei` |
| `fw_ver` | `0:21.0.6.1503` on each of three lines |
| the [ESRT](esrt.md)'s Management Engine entry | `1503` |

- Consequence: the ESRT's entry carries the last field of the version and
  nothing else, so the class is the fuller source.

## Open

- A board where the three lines differ is unread.

## Sources

- The Linux kernel's `mei` class attributes, as `drivers/misc/mei/main.c`
  creates them.
- `framework_lib` — `csme.rs`.
