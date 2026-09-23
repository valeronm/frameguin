# Battery and charging over the EC

The pack is read two ways: the EC's own copy of it in the memory map, and
the pack's registers over I²C passthrough, both routes of
[`ec.md`](ec.md#routes). Charging is set by host command. The EC calls are
`hardware/src/ec.rs`; the pack's registers and their words are
`hardware/src/sbs.rs`; the extender's command is `hardware/src/extender.rs`;
the device is `hardware/src/device/battery.rs`.

Each section holds what the EC tree, `framework_lib`, the gauge's manual or
this code establishes, then under **Observed** what was read on a machine.
Every observation is from the Laptop 13 Pro (Intel Core Ultra Series 3) and
its 74 Wh pack unless its row names another board.

## Contents

Every heading in the file appears here.

- [The EC's battery block](#the-ecs-battery-block)
- [Telling the packs apart](#telling-the-packs-apart)
- [The flag byte](#the-flag-byte)
- [The pack over I²C](#the-pack-over-ic)
- [Cycle count](#cycle-count)
- [Charge percentage](#charge-percentage)
- [Temperature](#temperature)
- [Status bits](#status-bits)
- [Health verdicts](#health-verdicts)
- [Charge limit](#charge-limit)
- [Charge current limit](#charge-current-limit)
- [Battery extender](#battery-extender)
- [The charger](#the-charger)
- [Persistence](#persistence)
- [Open](#open)
- [Sources](#sources)

## The EC's battery block

One memory-map region, `0x40` to `0x7f`, in two halves the EC refreshes
differently:

| Offset | Field | Unit | Half |
|---|---|---|---|
| `0x40` | present voltage | mV | dynamic |
| `0x44` | present rate | mA | dynamic |
| `0x48` | remaining capacity | mAh | dynamic |
| `0x4c` | flags | bits, [below](#the-flag-byte) | dynamic |
| `0x4d` | battery count | | |
| `0x4e` | battery index | | |
| `0x50` | design capacity | mAh | static |
| `0x54` | design voltage | mV | static |
| `0x58` | last full charge capacity | mAh | dynamic |
| `0x5c` | cycle count | | static |
| `0x60` | manufacturer | 8 bytes | static |
| `0x68` | model | 8 bytes | static |
| `0x70` | serial | 8 bytes | static |
| `0x78` | type | 8 bytes | static |

- Every charger pass refreshes the dynamic half. The static half is filled
  by `update_static_battery_info` only while the charger task's
  `need_static` flag is set, which happens on a battery presence change and
  on the paths that wake, deep-charge or revive a pack, and the flag clears
  on the first successful read.
- An 8-byte string field holds seven characters and a terminator,
  `EC_MEMMAP_TEXT_MAX`.

### Observed

| Setup | Reading |
|---|---|
| Model field | `FRANEDA`; the pack's own `DeviceName` register reads the same, and the `FRANEDAC00` on its label exists only there |

## Telling the packs apart

The pack's `DeviceName`, cut to seven characters on its way to the host,
is what separates the packs the EC tree knows:

| `DeviceName` | Seven-character form | Devicetree compatible | Boards declaring it | `board_get_battery_type` |
|---|---|---|---|---|
| `Framework Laptop` | `Framewo` | `atl,framework55w` | `azalea`, `marigold` | `FWK_BATT_NVT_55W` |
| `FRANGWAT01` | `FRANGWA` | `atl,framework61w` | `azalea`, `marigold` | `FWK_BATT_NVT_61W` |
| `FRANEDA` | `FRANEDA` | `atc,framework75w` | `marigold`, and `sakura` through it | `FWK_BATT_ATC_75W` |
| `FRANDBAT01` | `FRANDBA` | `atl,framework85w` | `lotus` | `FWK_BATT_NVT_85W` |
| `FRANDZG` | `FRANDZG` | `atc,framework50w` | `sunflower` | none; reads `FWK_BATT_UNKNOWN` |

- The firmware itself compares at seven characters:
  `board_get_battery_type` uses `strncmp` with `EC_MEMMAP_TEXT_MAX - 1`.
  Its enum names a maker and a wattage, `ATC_75W` for the 74 Wh pack, and
  the devicetree compatible names the maker differently, `atl` where the
  enum reads `NVT`.
- A fuller name is `EC_CMD_BATTERY_GET_STATIC`: v0 answers the same 8-byte
  fields, v1 adds 12-byte `_ext` fields holding 11 characters, and v2
  widens every string to `SBS_MAX_STR_OBJ_SIZE`, 32 bytes. The command
  exists only on battery API v2; `common/battery_v1.c`, which `hx20` and
  `hx30` build, declares no host command. The 55 Wh pack fits machines on
  both sides of that split, so the seven-character form is the one name
  every machine agrees on, and `model::part` keys on it.

## The flag byte

| Bit | Flag |
|---|---|
| `0x01` | `AC_PRESENT` |
| `0x02` | `BATT_PRESENT` |
| `0x04` | `DISCHARGING` |
| `0x08` | `CHARGING` |
| `0x10` | `LEVEL_CRITICAL` |
| `0x20` | `INVALID_DATA` |
| `0x40` | `CUT_OFF` |
| `0x80` | `LIMIT_ACTIVE` |

- `DISCHARGING` is the pack reporting zero charge current, not the pack
  supplying the machine. A full pack on a connected charger sets it, and
  `framework_tool --power` prints "Battery discharging" for it.
- Neither direction flag set is a state the sustainer produces: on reaching
  its ceiling it switches to `CHARGE_CONTROL_IDLE` and clears both, which is
  what ACPI's charge-limiting convention asks so the host stops claiming a
  direction. The charge current then decays for as long as a minute.
- Consequence: the flags alone never settle the direction. `charge_flow` in
  `ec.rs` reads charging where `CHARGING` is set, idle where a charger is
  present and the rate is zero or `DISCHARGING` is clear, and discharging
  otherwise.

## The pack over I²C

- Every pack in the EC's devicetree is `battery-smart` at the same place:
  EC I²C port 3, 7-bit address `0x0b`, the `0x16` the datasheet writes in
  8-bit form.
- `hx20` and `hx30`, with no devicetree, put the pack on port 1
  (`I2C_PORT_BATTERY` is `MCHP_I2C_PORT1` in their `board.h`); their port
  3 is the thermal sensor bus. `battery_i2c_port` in `ec.rs` names those
  boards.
- `sunflower`'s `atc,framework50w` pack is three cells in series, its
  `voltage_max` 13440 mV against the four-cell packs' 17600 and up.
  Consequence: `sbs::cell_millivolts` drops a cell register reading 0.
- The gauge is a TI bq40z50. Its Smart Battery registers are generic; its
  `ManufacturerAccess` map, safety status, permanent-failure status, state of
  health and the lifetime blocks, is that part's alone.

Registers read, all plain word reads:

| Register | Name | Unit | In `sbs.rs` |
|---|---|---|---|
| `0x08` | Temperature | 0.1 K | `TEMPERATURE` |
| `0x16` | BatteryStatus | bits, [below](#status-bits) | `BATTERY_STATUS` |
| `0x17` | CycleCount | | `CYCLE_COUNT` |
| `0x1b` | ManufactureDate | day in bits 0 to 4, month in 5 to 8, years since 1980 above | `MANUFACTURE_DATE` |
| `0x3f`, `0x3e`, `0x3d`, `0x3c` | CellVoltage 1 to 4 | mV; the registers run backwards against the cell numbering | `CELL_VOLTAGES` |

- The gauge's firmware version is a `ManufacturerAccess` block command: a
  write of the subcommand to `0x00`, then a block read from `0x44`. Nothing
  above needs a write.
- A sealed pack answers the generic registers and returns zeros or empty
  blocks for safety status, permanent-failure status and the lifetime data.
  Unsealing is itself a write.

## Cycle count

- The EC's copy is in the static half of the block, refreshed only when
  `need_static` is set, and the EC outlives host reboots. Consequence: the
  published count is the one taken when the EC last initialized the
  battery, and `Ec::cycle_count` reads the pack's register instead.
- Everything else in the static half is fixed, design capacity and the
  strings, so the cycle count is the one value that both moves and is
  published as static.

### Observed

| Setup | Reading |
|---|---|
| EC block against the gauge's `CycleCount` | 3 against 8 |

## Charge percentage

- The EC's block carries no percentage, only remaining and last full
  charge capacity. `framework_lib`'s `charge_percentage` divides the two
  and truncates, and a pack reporting a last full charge of zero panics
  inside the library. The kernel's ACPI battery `capacity` rounds the same
  two to the nearest percent with `DIV_ROUND_CLOSEST_ULL`, and
  `ec::charge_percent` does likewise.
- The gauge computes its own, `RelativeStateOfCharge` (`0x0d`), which
  `framework_tool --smartbattery` prints beside `RemainingCapacity`
  (`0x0f`) and `FullChargeCapacity` (`0x10`). This code does not read it.
  It rounds up, so it runs up to a percent ahead of a percentage computed
  from the capacities.

### Observed

| Remaining / full, mAh | Exact | Reading |
|---|---|---|
| 2012 / 4730 | 42.54 % | gauge 43, the desktop's battery indicator, which reads the kernel, 43, `charge_percentage` 42 |
| 2002 / 4730 | 42.33 % | gauge 43 |
| 1994 / 4730 | 42.16 % | gauge 43, kernel `capacity` and UPower 42 |
| 1985 / 4730 | 41.97 % | gauge 42 |

## Temperature

- Some boards' thermal sensor array carries a `cros-ec,temp-sensor-battery`
  entry at the pack's address, whose binding describes it as the last polled
  battery temperature: the gauge's sensor relayed, not a second one. Its
  index differs per board, and the AMD and Desktop boards have no such entry.
- The array's encoding: Kelvin offset by `EC_TEMP_SENSOR_OFFSET`, 200, with
  the top four byte values reserved:

| Value | Meaning |
|---|---|
| `0xfc` | not calibrated |
| `0xfd` | not powered |
| `0xfe` | error |
| `0xff` | not present |

- Freezing is 73 in that encoding, so a decode is signed; `framework_lib`'s
  `t - 73` underflows below 0 °C.
- Consequence: `sbs::decicelsius` reads the gauge's register, which is in
  tenths of a degree, current rather than last polled, and present on every
  board.

## Status bits

`BatteryStatus`, `0x16`, in the two groups the EC's console prints:

| Bits | Group | Names |
|---|---|---|
| 4 to 7 | states | `FD` fully discharged, `FC` fully charged, `DSG` discharging, `INIT` initialized |
| 8 to 15 | alarms | `RTA`, `RCA`, `TDA`, `OTA`, `TCA`, `OCA` and two reserved |

- `INIT` set is the gauge having finished its power-on self-test and
  calibration, so its readings can be trusted.
- The bq40z50 technical reference gives every alarm's set conditions:

| Alarm | Set by | Fault on its own |
|---|---|---|
| `OCA` overcharged | safety and permanent-failure conditions only | yes |
| `OTA` overtemperature | safety and permanent-failure conditions only | yes |
| `TCA` terminate charge | those, plus a `GaugingStatus` condition at every ordinary full charge | no |
| `TDA` terminate discharge | those, plus a `GaugingStatus` condition at every ordinary empty | no |
| `RCA`, `RTA` remaining capacity and time | thresholds the host sets | no; the OS already warns |

- `TCA` and `TDA` together are a fault: their gauging conditions need charge
  mode and discharge mode respectively, so both at once come only from a
  safety alert, a permanent failure, or the pack reporting itself absent.
  `sbs::alarms` raises `OCA`, `OTA` and that pair, which is the only
  visibility into over-current and cell-undervoltage faults without
  unsealing. `FD` has a `GaugingStatus` condition too.

## Health verdicts

- `framework_tool --smartbattery` ends with a health analysis. On a sealed
  pack its safety-status and permanent-failure reads come back zero and its
  lifetime blocks empty, and the code cannot tell "nothing wrong" from
  "could not look", so a sealed `HEALTHY` rests on the alarm bits, capacity
  retention and cell balance alone.
- Capacity retention, last full against design capacity, can exceed 100% on
  a new pack and carries nothing about internal resistance or cell balance. The
  EC publishes only the pack total, so cell spread comes from the gauge.

## Charge limit

| | |
|---|---|
| What | a ceiling on state of charge, held by the EC's battery sustainer |
| Command | `EC_CMD_CHARGE_LIMIT_CONTROL`, `0x3E03`, in `battery_extender.c` |
| Storage | `SYSTEM_BBRAM_IDX_CHARGE_LIMIT_MAX`, battery-backed RAM |
| Window | `battery_sustainer_set(max(20, limit - 5), limit)` |
| Readback | the BBRAM value |

- Sitting at the ceiling is what produces the direction the
  [flag byte](#the-flag-byte) cannot express.
- `CHG_LIMIT_OVERRIDE`, bit 7 of the same command's modes, lifts the
  ceiling in RAM without touching BBRAM, and holds through a full pack and
  an unplug until the next `DISABLE`, `SET_LIMIT` or EC restart.
- `CHG_LIMIT_GET_LIMIT` reloads the RAM value from BBRAM before answering,
  so on every Zephyr board a read cancels the override. The old EC's
  handler was fixed not to
  ([EmbeddedController #7](https://github.com/FrameworkComputer/EmbeddedController/pull/7));
  the Zephyr rewrite never took the fix.
- The response carries only the BBRAM percentages, so the override has no
  read either.

## Charge current limit

| | |
|---|---|
| What | a ceiling on the current drawn while charging, with no bearing on where charging stops |
| Command | `EC_CMD_CHARGE_CURRENT_LIMIT`, `0x00A1`; v0 takes the limit, v1 the limit and a state-of-charge threshold |
| Storage | `user_current_limit` and its pending value, statics in `common/charge_state.c`, initialized to no limit |
| Readback | none, in any version ([framework-system #180](https://github.com/FrameworkComputer/framework-system/issues/180)) |

- The v1 threshold latches inside the EC: once applied it is never
  re-evaluated, so a later threshold cannot lift it
  ([framework-system #342](https://github.com/FrameworkComputer/framework-system/issues/342)).
  `Ec::set_charge_current_limit` sends the unconditional form.
- A rate in C converts against design capacity: the design capacity in mAh
  is the 1C current in mA, which `framework_lib::set_charge_rate_limit`
  prints as "Design Current".
- Consequence of no readback: the device holds a `Mirror` of the last
  accepted write, with the lifetime of the EC that took it.

## Battery extender

Framework's own addition beside the charge limit, in `battery_extender.c`.

| Stage | Reached | Sustainer window |
|---|---|---|
| 1 | `trigger_days` after the EC starts or the extender last reset; 5 by default | `min(90, lower)` to `min(95, upper)` |
| 2 | two days after stage 1 | `min(85, lower)` to `min(87, upper)` |

- A reset is `reset_minutes` continuously off the charger, 30 by default.
  Being on the charger pushes the reset's deadline forward every second, so
  only an unplugged stretch counts; a reset returns the extender to holding
  nothing with its countdown restarted. The countdown runs whether or not a
  charger is attached.
- Each stage takes the lower of its own window and the charge limit's, so a
  limit at or under 95% leaves stage 1 nothing to change and one at or under
  87% stage 2. The charge limit command answers the BBRAM value throughout.
- `BATTERY_EXTENDER_STAGE1_VOLTAGE` and `STAGE2_VOLTAGE` are defined and
  used by nothing; the charge voltage is not what the extender moves.
- `EC_CMD_BATTERY_EXTENDER`, `0x3E24`: sub-command 1 reads the stage, the
  disable flag, both settings and the time left to stage 1 and to the reset;
  sub-command 0 writes, taking `disable` as given, so an all-zero request
  switches a disabled extender back on. Nothing reports the time left to
  stage 2. `framework_lib` does not implement the command. Every Framework
  branch carries the handler, `hx20` and `hx30` included.

## The charger

| Board | Part | Driver |
|---|---|---|
| `sakura` | RAA489108 | the ISL9238C driver, whose register map it shares; `CONFIG_PLATFORM_EC_CHARGER_ISL9241=n` |
| `azalea`, `marigold`, `sunflower`, `lotus` | ISL9241 | ISL9241 |
| `tulip` | BQ25770 and RAA489300 | both |

- No measured input current exists on the Laptop 13 Pro: the ISL923x
  driver reads the charger's AMON pin only through an EC ADC channel named
  `ADC_AMON_BMON`, behind `CONFIG_CMD_CHARGER_ADC_AMON_BMON`, and no
  Framework board's devicetree declares that channel. Consequence: the
  current arriving from the wall is a limit the EC set, never a reading, and
  the four ports feed one adapter node, so even a reading would not name a
  port.
- `EC_CMD_CHARGE_STATE`, `0x00A0`: its get-state sub-command copies the
  charge loop's cached values, whether a charger is attached and the pack's
  charge, both already in the block, and three charger registers holding
  what the charger was told rather than anything it measured. Its set-param
  sub-command writes the charger's voltage, current, input limit and options,
  refused only on locked firmware.
- The input current limit is 95% of the negotiated contract's current,
  `charge_ma * 95 / 100` in `sakura/src/charger.c`'s `board_set_charge_limit`. The charge current is what the pack asks for,
  lowered to the charge current limit. The charge voltage is what the pack
  asks for while charging; while the loop asks for nothing, `charge_request`
  sets it to the pack's present voltage plus one charger step, the ISL9238C
  driver selecting `CHARGER_NARROW_VDC`, which keeps the system rail above
  the pack.

### Observed

| Setup | Reading |
|---|---|
| Charge current over seconds, pack near full | moves between 1C, 0.5C and nothing as the pack changes its request |

## Persistence

| Control | Suspend | Reboot | EC restart | Source |
|---|---|---|---|---|
| Charge limit | kept | lost | kept | BBRAM survives the EC; UEFI setup re-sends its own stored value at every POST, so the standing value is setup's |
| Charge current limit | kept | kept | lost | plain statics that `charger_init` leaves alone, re-initialized on every EC boot; setup has no option for it, so nothing re-asserts one at POST |

- The EC runs straight through a host reboot and a suspend, so neither
  costs it anything of its own.
- The old EC on `hx20` and `hx30` lifts the charge current limit in
  `reset_current_limit`, hooked to `HOOK_CHIPSET_SUSPEND` and
  `HOOK_CHIPSET_SHUTDOWN` in `common/charge_state_v2.c`; the Zephyr
  `common/charge_state.c` has no such hook. Consequence: on those boards
  the device's mirror lasts only while the host stays awake.

### Observed

| Setup | Reading |
|---|---|
| One reboot with a charge limit, a power LED level and a charge current limit set from the OS, the EC surviving | the charge limit came back at setup's value and the LED level at setup's; the current limit was still the one written and the pack still charged at it |
| Watching for `HC 0x00a1` in `framework_tool --console recent` through a boot | not possible: the ring is about 4 KB and POST fills it with paired `event set` and `PORT80:` lines at roughly fifty a second, so it holds some two seconds of boot against a POST that ended ten seconds before the earliest userspace read |

- Consequence: the reboot row for the current limit rests on the contrast
  with the two controls setup owns, the control having no readback. The
  console still shows a write landing while the machine is up, as
  `HC 0x00a1` with the `charge_request(<mV>, <mA>)` it produces, and its
  timestamps are EC uptime, so a dump spanning two host boots is itself
  proof the EC did not restart.

## Open

- Whether `FRANDZG`'s absence from `board_get_battery_type` matters on
  `sunflower`, which is the one board declaring it.

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — `include/ec_commands.h` for the memory map, the flag bits,
  `EC_CMD_BATTERY_GET_STATIC`, `EC_CMD_CHARGE_CURRENT_LIMIT`,
  `EC_CMD_CHARGE_STATE` and the thermal encoding; `common/charge_state.c`
  for `need_static`, the sustainer and `user_current_limit`;
  `common/battery_v1.c` and `battery_v2.c` for the API split;
  `zephyr/program/framework/src/battery_extender.c` for the extender and
  the charge limit command, `src/board_function.c` for the battery types,
  each board's `battery.dtsi` for the packs, `sakura/src/charger.c` for
  the input current limit, `driver/charger/isl923x.c` for the AMON read;
  `zephyr/dts/bindings/temp/`
  for the battery sensor binding.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_lib/src/power.rs` for the percentage and the temperature
  decode, `chromium_ec/mod.rs` for `set_charge_rate_limit` and
  `set_charge_current_limit`, `smart_battery.rs` for the health analysis
  and the gauge's percentage, and issues #180 and #342.
- [torvalds/linux](https://github.com/torvalds/linux) —
  `drivers/acpi/battery.c` for the kernel's `capacity`.
- TI bq40z50 technical reference manual — the register map and every
  status bit's set conditions. SLUUA43A covers the R2 revision and SLUUBU5A
  the R3, which differ in their `ManufacturerAccess` status bits. The
  datasheet, SLUSBS8, is the electrical specification and defers to the
  manual for registers.
- Smart Battery Data Specification — the generic register set the gauge
  implements.
