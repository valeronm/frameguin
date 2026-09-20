# Storage over NVMe

A drive is read from its controller under `/sys/class/nvme`: `model`,
`serial` and `firmware_rev`, all world-readable. The read is
`hardware/src/nvme.rs`; the maker's and model's names for its PCI ids come
from udev's cached record, `hardware/src/udev.rs`.

Each section holds what the kernel, the NVMe specification or this code
establishes, then under **Observed** what was read. Every observation is
from the Laptop 13 Pro (Intel Core Ultra Series 3).

## Contents

Every heading in the file appears here.

- [What the class carries](#what-the-class-carries)
- [Who made it](#who-made-it)
- [Which drives are parts](#which-drives-are-parts)
- [Open](#open)
- [Sources](#sources)

## What the class carries

| Attribute | Reads | Note |
|---|---|---|
| `model` | the Identify controller's model number | space-padded to the field's width; what the drive is ordered by, not what it is sold as |
| `serial` | the Identify controller's serial number | space-padded |
| `firmware_rev` | the running firmware revision | space-padded |
| `transport` | `pcie` for a drive on the bus | `nvme::read` skips any other |
| `device` | a link to the PCI function | where `vendor`, `device` and `removable` are read |

- `nvme::read` trims the padding from every string.
- Capacity is the block layer's `size` under the controller's namespace,
  in 512-byte sectors.

### Observed

| Attribute | Reading |
|---|---|
| `model` | `SD PC SN7100S SDFPNSL-1T00` |
| `firmware_rev` | `7612M000` |
| udev `ID_MODEL_FROM_DATABASE` for the function | `WD_BLACK SN7100/WD PC SN7100S M.2 2280 NVMe SSD` |

- Consequence: nothing the drive answers connects its model number to the
  retail name; the udev database does, so `Drive::of` takes the database's
  model and keeps the drive's own as the part number.

## Who made it

NVMe names no maker in words. Three identifiers carry one, and they can
disagree:

| Identifier | Where | Registry |
|---|---|---|
| PCI vendor id | the function's `vendor` | PCI-SIG |
| IEEE OUI | the first three bytes of the namespace's EUI-64 | IEEE |
| Subsystem NQN | `subsysnqn` | the maker's own domain; a drive with none is given `nqn.2014.08.org.nvmexpress:…` by the kernel |

### Observed

| Identifier | Reading | Names |
|---|---|---|
| PCI vendor id | `0x15b7` | SanDisk; udev spells it `Sandisk Corp` |
| IEEE OUI | `00:1B:44` | SanDisk |
| Subsystem NQN | `nqn.2023-01.com.wdc:…` | Western Digital |

## Which drives are parts

- The class lists every NVMe controller the kernel drives, a drive in a
  Thunderbolt enclosure beside one in an M.2 slot.
- The kernel marks a PCI device below an external-facing port `removable`
  in the function's `removable` attribute. `nvme::read` skips a controller
  whose function reads so.
- A drive on a USB bridge, the storage expansion card among them,
  enumerates as SCSI and does not appear in the class.

### Observed

| Setup | Reading |
|---|---|
| The board's own drive, `removable` | not `removable` |
| A drive in a Thunderbolt enclosure | listed in the class beside the board's own |

## Open

- Whether an enclosure's drive reads `removable`. The skip rests on the
  kernel's rule, not on a reading.

## Sources

- NVM Express Base Specification — the Identify controller data structure
  (model number, serial number, firmware revision), the EUI-64 and the NQN.
- The Linux kernel's NVMe controller sysfs attributes, as
  `drivers/nvme/host/sysfs.c` creates them, and
  `Documentation/ABI/testing/sysfs-devices-removable` for `removable`,
  which each bus fills in its own way and PCI from firmware.
- udev's `/run/udev/data` records, for the names behind PCI ids.
