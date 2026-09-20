# Reaching the EC

The embedded controller is reached through the kernel's `cros_ec` device,
by way of `framework_lib`. Three routes go through it, and which one a
value comes from decides what it costs and how fresh it is. The crate's one
holder of the connection is `hardware/src/ec.rs`, opened only where the DMI
vendor is Framework's; how the EC's boot instant is weighed against a
stored one is `hardware/src/lifetime.rs`.

Each section holds what the EC tree, `framework_lib` or this code
establishes, then under **Observed** what was read on a machine. Every
observation is from the Laptop 13 Pro (Intel Core Ultra Series 3).

## Contents

Every heading in the file appears here.

- [Routes](#routes)
- [Opening the EC](#opening-the-ec)
- [The uptime clock](#the-uptime-clock)
- [EC restarts](#ec-restarts)
- [Which board the EC tree calls this machine](#which-board-the-ec-tree-calls-this-machine)
- [Open](#open)
- [Sources](#sources)

## Routes

| Route | Mechanism | Cost | Carries |
|---|---|---|---|
| Memory map | a region the EC keeps updated, read by `CROS_EC_DEV_IOCRDMEM` against `/dev/cros_ec` with no command round trip | cheapest | the battery block, thermal sensors, fan speeds |
| Host command | a request and response over the same device | one round trip | everything that sets something, and the reads the memory map has no room for |
| I²C passthrough | `EC_CMD_I2C_PASSTHRU`, a host command carrying an I²C transaction the EC performs on the host's behalf | a round trip plus a bus transaction | a device the EC is itself driving, the battery gauge among them |

- The pack hangs off EC I²C port 3 on every Framework board,
  `BATTERY_I2C_PORT` in `ec.rs`.
- Whether the firmware implements a command at a version is asked with
  `EC_CMD_GET_CMD_VERSIONS`, `Ec::offers`, which is the probe for a
  write-only control.

## Opening the EC

- `framework_lib`'s `CrosEc::new` takes the first available driver and
  panics where the list is empty, an aarch64 machine with no `/dev/cros_ec`
  for one. `Ec::open` constructs it only where `Board::is_framework` holds.
- `Ec` is the one holder of the `CrosEc`, behind a mutex that does not
  re-enter; its module doc carries the lock discipline.

## The uptime clock

`EC_CMD_GET_UPTIME_INFO` answers:

| Field | Meaning |
|---|---|
| `time_since_ec_boot_ms` | milliseconds since the EC booted, 32 bits; a sysjump does not reset it |
| `ap_resets_since_ec_boot` | how many times the EC reset the host |
| `ec_reset_flags` | why the EC last reset |
| `recent_ap_reset` | the last four host resets and their causes |

- Nothing in it is an identity: no boot id, no restart counter. Whether the
  EC is still the one that took a write is answered by comparing how far its
  clock has advanced against the host's, `EcBoot::from_clocks`.
- The counter wraps at 49.7 days. An EC up longer reads as one that
  restarted.
- The EC's own comment on the field: the timebase is like
  `CLOCK_MONOTONIC_RAW` with 1% or more frequency error against the host's.
  `EcBoot::same_as` allows one twentieth of the uptime, and never less than
  a minute, before a boot instant reads as a different boot.

## EC restarts

- `power_chipset_init` on every Framework board forces the chipset to G3 and
  starts power sequencing there, so an EC restart takes the host down with
  it. No running system sees one; what a value's persistence across an EC
  restart asks is whether it outlives the EC that held it.
- With no adapter attached the EC runs off the pack, so a pack drained to
  empty restarts the EC and clears everything it held in RAM.
- Consequence: the EC's uptime shows that it restarted and not why, so a
  boot that followed a flat battery is no evidence about a reboot.

## Which board the EC tree calls this machine

The firmware version string opens with the name of the board's directory
in the EC tree. Nothing in the tree maps that name to the DMI strings a
machine reports, so the string is the one thing that settles which
directory answers for a machine: the connector maps, the controller count, the pack,
the LED colors and the charger part are all per board, and boards differ in
which drivers they compile at all.

`EC_CMD_GET_BUILD_INFO` answers `build_info` from `common/version.c`:

| Piece | Source | Note |
|---|---|---|
| version | `VERSION`, opening with the board name | |
| CrOS FWID | `CROS_FWID32` | only on firmware built with `CONFIG_CROS_FWID_VERSION` |
| build stamp | `DATE` | two space-separated fields, date and time, with no zone; `util/getversion.sh` takes a `git log` date or a file's mtime and cuts the offset off either |
| builder | `BUILDER` | the machine the firmware was compiled on, no relation to the hardware |

- A reproducible build replaces the stamp with the literal
  `STATIC_VERSION_DATE` and the builder with `reproducible@build`.

The board codenames, from the firmware map in the README on the tree's
default branch, `framework-readme`:

| Machine | Codename | Branch | Family option in `project.conf` |
|---|---|---|---|
| Laptop 12, 13th Gen Intel Core | `sunflower` | `fwk-sunflower-*` | `LAPTOP_12` |
| Laptop 13, 11th Gen Intel Core | `hx20` | `fwk-hx20-hx30-*` | pre-Zephyr, under `board/` |
| Laptop 13, 12th and 13th Gen Intel Core | `hx30` | `fwk-hx20-hx30-*` | pre-Zephyr, under `board/` |
| Laptop 13, AMD Ryzen 7000 Series | `azalea` | `fwk-lotus-azalea-*` | `LAPTOP_13` |
| Laptop 13, Intel Core Ultra Series 1 | `marigold` | `fwk-marigold-*` | `LAPTOP_13` |
| Laptop 13, AMD Ryzen AI 300 | `lilac` | `fwk-lilac-*` | |
| Laptop 13 Pro, Intel Core Ultra Series 3 | `sakura` | `fwk-sakura-*` | `LAPTOP_13`; includes `marigold`'s devicetree |
| Laptop 16, AMD Ryzen 7000 Series | `lotus` | `fwk-tulip-*` | `LAPTOP_16` |
| Laptop 16, AMD Ryzen AI 300 | `tulip` | `fwk-tulip-*` | `LAPTOP_16` |
| Desktop, AMD Ryzen AI Max 300 | `dogwood` | `fwk-dogwood-*` | `FRAMEWORK_MINI_PC`, in `desktop_program.conf` |

- The tree branches per board and per snapshot, the branch name ending in a
  number that changes with each; no branch holds every board. A `sakura`
  checkout carries the `azalea`, `marigold`, `sunflower`, `lotus` and
  `tulip` directories beside its own and not `lilac`, `dogwood`, `hx20` or
  `hx30`.

### Observed

| Setup | Reading |
|---|---|
| `EC_CMD_GET_BUILD_INFO` | `sakura-3.0.2-cf48815 2026-05-26 04:34:57 lotus@ip-172-26-3-226`: no FWID, and a builder that is a host named after another board |

## Open

Nothing.

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — `include/ec_commands.h` for `EC_CMD_GET_UPTIME_INFO`'s response and its
  comment on the timebase; `common/version.c` and `util/getversion.sh` for
  the build string; each board's `src/power_sequence.c` for
  `power_chipset_init`; `zephyr/program/framework/*/project.conf` and
  `desktop_program.conf` for the family options; the README on the
  `framework-readme` branch for the codename map.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_lib/src/chromium_ec/mod.rs` for `CrosEc::new` and
  `version_info`, `cros_ec.rs` for the memory-map read.
