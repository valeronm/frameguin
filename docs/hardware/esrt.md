# Firmware versions over the ESRT

The EFI System Resource Table lists the firmware a UEFI capsule can update,
each entry a GUID with a version. The kernel publishes it under
`/sys/firmware/efi/esrt/entries`, every attribute root-only. The mainboard's
USB4 retimer versions are read from it, by `hardware/src/esrt.rs` through
`framework_lib`'s reader and GUID table.

Each section holds what the UEFI specification, the kernel, `framework_lib`
or this code establishes, then under **Observed** what was read. Every
observation is from the Laptop 13 Pro (Intel Core Ultra Series 3).

## Contents

Every heading in the file appears here.

- [What an entry carries](#what-an-entry-carries)
- [Which entries are read](#which-entries-are-read)
- [The retimer version](#the-retimer-version)
- [Open](#open)
- [Sources](#sources)

## What an entry carries

| Attribute | Reads |
|---|---|
| `fw_class` | the GUID naming what the entry updates |
| `fw_type` | 1 for system firmware, 2 for device firmware |
| `fw_version` | the running version, a 32-bit number in decimal |
| `lowest_supported_fw_version` | the oldest version a capsule may install |
| `last_attempt_version` | the version the last capsule carried |
| `last_attempt_status` | how that attempt ended |

- The table is in no fixed order: `esrt_from_sysfs` in `framework_lib`
  reads the directory as the kernel lists it.
- An unprivileged process reads none of it, so only the daemon can.

### Observed

| Entry | `fw_type` | `fw_version` |
|---|---|---|
| the retimer on ports 2 and 3 | 2 | `0x82802` |
| the Management Engine | 2 | `0x5DF` (1503) |
| the retimer on ports 0 and 1 | 2 | `0x82802` |
| the BIOS | 1 | `0x304` |

## Which entries are read

- `framework_lib`'s `match_guid_kind` names a GUID per Intel generation
  for each retimer, the Management Engine and the BIOS, and one BIOS GUID
  per AMD board. No AMD board has a retimer or Management Engine entry in
  it.
- A retimer GUID is named for the pair of ports it serves, `Retimer01` or
  `Retimer23`. A PD controller drives the same pair, so
  `esrt::retimers` gives a retimer its PD controller's number through
  `pd_controller::controller_of`: 0 for ports 0 and 1, 1 for ports 2 and 3.
- `framework_tool --versions` prints `Retimer01` as Right and `Retimer23`
  as Left, as it does the PD controllers.
- The BIOS entry is not read: [`dmi`](dmi.md#board-detection) gives the
  BIOS version as the firmware spells it.
- The Management Engine entry is not read: its version is the build number
  alone, and [`mei`](mei.md) gives all four fields.

## The retimer version

- `esrt::retimers` reports the version in hexadecimal, as
  `framework_tool` prints it. Nothing in `framework_lib` divides it into
  fields.

### Observed

| Attribute | Reading |
|---|---|
| a retimer's `fw_version` | `0x82802` |
| the same entry's `last_attempt_version` | `0x2802` |

- Consequence: the two attributes of one entry disagree in the digit above
  the low sixteen bits, so the number is not a plain version and no dotted
  spelling of it is known to be right.

## Open

- What the retimer version's fields are, and what the digit above the low
  sixteen bits means.
- Whether the kernel's Thunderbolt bus reports a retimer's NVM version on
  any board. `/sys/bus/thunderbolt/devices` listed no retimer here with
  nothing plugged in.
- Every board other than the Laptop 13 Pro is unread.

## Sources

- UEFI Specification — the EFI System Resource Table and its
  `EFI_SYSTEM_RESOURCE_ENTRY`.
- The Linux kernel's ESRT sysfs attributes, as
  `drivers/firmware/efi/esrt.c` creates them.
- `framework_lib` — `esrt/mod.rs` for the reader and the GUID table, and
  `commandline/mod.rs` for which retimer `framework_tool` prints on which
  side.
