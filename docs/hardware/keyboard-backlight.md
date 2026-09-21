# Keyboard backlight over the EC

Read and set by host command. Nothing in this project implements it: it
appears as neither a part nor a control, and this file records what the
firmware does with it for whoever adds one.

Each section holds what the EC tree or `framework_lib` establishes, then
under **Observed** what was read on a machine. Every observation is from
the Laptop 13 Pro (Intel Core Ultra Series 3).

## Contents

Every heading in the file appears here.

- [Commands](#commands)
- [Second writers](#second-writers)
- [Persistence](#persistence)
- [Open](#open)
- [Sources](#sources)

## Commands

| Command | Id | Answers |
|---|---|---|
| `EC_CMD_PWM_GET_KEYBOARD_BACKLIGHT` | `0x0022` | the stored percentage exactly |
| `EC_CMD_PWM_SET_KEYBOARD_BACKLIGHT` | `0x0023` | sets it |

- `framework_lib::get_keyboard_backlight` reads through
  `EC_CMD_PWM_GET_DUTY` instead and floors twice, percent to duty in the EC
  and duty back to percent in the library, so most values come back one
  low.

### Observed

| Setup | Reading |
|---|---|
| 5% set, read through `framework_lib` | 4% |

## Second writers

- Fn+Space changes the level in the EC.
- Newer boards have a firmware auto mode, `kb_als_auto_brightness`, stepped
  by the ambient light sensor.
- Consequence: anything showing the value re-reads rather than trusting what
  it last wrote.

## Persistence

| Event | Level | Source |
|---|---|---|
| Suspend | kept | the EC stays up and the save is never reached |
| Reboot | kept | `fnkey_shutdown`, on `HOOK_CHIPSET_SHUTDOWN`, writes the level into `SYSTEM_BBRAM_IDX_KBSTATE`, or `KEYBOARD_BL_BRIGHTNESS_AUTO` where auto is on; `board_kblight_init` restores it |
| EC restart | kept | the same BBRAM byte survives the EC |

- The Fn-lock state shares that byte, `KB_FN_LOCKED` in its top bit, so the
  level occupies the low seven.

### Observed

| Setup | Reading |
|---|---|
| Boot captures of the EC console | one `HC 0x0023` write, landing after the kernel's own EC probe, so the host restoring a saved level rather than firmware |

## Open

- Whether UEFI setup re-sends the level at POST, as it does the charge limit
  and the power LED level. POST itself cannot be watched from the console
  ring ([battery.md](battery.md#persistence) carries why). The control has
  a getter, so the test is the one that settled those two: set a distinctive
  level, reboot, read it back.

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — `include/ec_commands.h` for the two commands,
  `zephyr/program/framework/src/keyboard_customization_13.c` for the BBRAM
  save and restore and the Fn-lock bit, `common/keyboard_backlight.c` and
  `common/pwm.c` for the duty path.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_lib/src/chromium_ec/mod.rs` for `get_keyboard_backlight`.
