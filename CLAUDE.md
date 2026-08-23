# Frameguin — working notes for Claude Code

Read README.md first for what the project is. This file covers how to work on
it and the non-obvious constraints.

## Layout and contracts

- Three crates, two binaries: `daemon/` runs as root and links the hardware
  libraries (`framework_lib`, `hidapi`); `app/` is the GTK4/libadwaita UI and
  links no hardware code; `wire/` is the D-Bus vocabulary both share. The
  split is the security model — the root process carries no GUI, the GUI
  process has no hardware access — and the D-Bus interface
  `io.github.valeronm.Frameguin1` is their only bridge. Nothing that touches
  hardware may enter `wire/`: the app links it, so a dependency added there
  lands in the unprivileged process too.
- `wire/` holds the proxy the app calls through, the bus name and object
  path, and the vocabularies as enums serializing as `s`. The daemon's
  `#[interface]` impl cannot move there — it is an impl on the type owning
  `CrosEc` — so the method set is still two declarations meeting only at
  runtime, in the bus. What the enums buy is the other half: a capability,
  level or click force the two ends spell differently used to be a
  well-formed string that meant nothing to the receiver, and is now a
  compile error. Adding a control means: one variant in `wire`, one probe +
  get/set methods in the daemon, one gated UI group in the app — and nothing
  in between, because the app holds the probe's answer as a set rather than
  unpacking it into a flag per capability.
- Inside `app/`, a module boundary is drawn where it makes a class of mistake
  impossible, not where a file got long. `format.rs` holds the presets, the
  values behind them and every label; `caps.rs` holds the probe's answer.
  Neither links GTK or the bus, so the window and the tray cannot disagree
  about what a preset sends or what it is called, and `Capabilities`' private
  field means the app can only offer what the daemon answered with — the
  probe rule, held up by the compiler at this end. `tray.rs` links no GTK
  either, which matters because its menu runs on ksni's own thread; its
  fields are private, so `tray_push` is the only way that state moves.
  `ui.rs` is the largest and stays that way on purpose: `Ui`'s fields are
  private to it, which is what `Sink` and the `apply_*` writes living there
  buys. `board.rs` is the one read that bypasses the bus, `about.rs` the
  report and the dialog that renders it, `autostart.rs` the desktop entry
  whose path and content cannot be written apart. `main.rs` keeps only what
  has to know both front-ends: the app id, the bus attachment, the lazily
  built window, and the tray event loop — exhaustive over `TrayEvent`, so a
  new variant fails to build until it is handled there. The no-GTK rules are
  the ones nothing checks: an import is all it takes to lose one.
- Inside `daemon/`, the modules are drawn by how a control reaches the
  hardware, so the filename answers which way: `ec.rs` the EC, `led.rs` the
  kernel's LED class, `touchpad.rs` the pad's own HID transport, with `fp.rs`
  for the arbitration the first two make necessary — the fingerprint LED has
  two possible drivers and one at a time. The rest divide by job: `board.rs`
  the DMI vendor read deciding whether there is an EC to open, `state.rs` the
  mirror for what cannot be read back, `probe.rs` the probe rule beside the
  code it governs. Each module's own doc carries why; what is worth saying
  here is what none of them can. `ec.rs` is not every EC call — a plain
  command is sent from the bus method that wants it, and what lands in `ec.rs`
  is a call needing more than the raw command: a correction, a cache, or a
  translation into the wire's terms. `main.rs` keeps the `Daemon` object,
  polkit, the idle exit and the `#[interface]` surface, and the hardware calls
  stay inline there for the same reason — the order around them (validate,
  skip a write already in place, authorize, write) is the policy, and
  splitting the write out would leave that order legible at neither end.
- A control the tray can also set gets an `apply_*` function owning the whole
  write: the daemon call, the toast, the tray's copy, and moving the widget to
  match. Both the window's handler and the tray preset call it; neither writes
  around it. Controls only the window sets (backlight, touchpad) write inline
  in their handler — one caller needs no shared function, and giving it one
  would be ceremony. Writing *by* moving the widget, which the tray used to
  do, makes state the command channel: a widget already showing the requested
  value emits no change, so the write is silently dropped — which is what a
  tray click on a stale window hits. Debouncing stays with the widget that
  needs it; a tray click is discrete.
- The two charge setters skip a write whose value is already set, and skip it
  before the polkit call for the same reason argument checks come first —
  nobody should answer a prompt for a write that won't happen. The skip
  belongs in the daemon rather than in a caller: `set_charge_limit` asks the
  EC and `set_charge_current_limit` asks its own mirror, the closest either
  can get to the truth, where a client could only consult its own stale idea
  of it. The rest write unconditionally, because only this app moves their
  values — bar the keyboard backlight, which the EC writes too and which the
  window therefore polls.
- D-Bus types name the value, not either end's convenience: a percentage is
  `y` (`u8`), never GTK's f64. The daemon validates every argument because
  any client can call it — an app-side clamp is UI convenience, not the check.
- In the daemon's `#[interface]` impl the signature carries meaning: `async`
  means the method awaits polkit, `fdo::Result` that it can fail — neither
  implies it touches the EC (`get_capabilities` does, and is neither). zbus
  boxes sync and async alike, so never reach for `async` to get concurrency.
- The daemon's connection runs on one executor thread and the EC calls block
  rather than await, so a slow EC read stalls every other task on that
  connection — the cold `get_capabilities` probe most of all. The fix is to
  move the call off the executor onto a blocking pool, not more `async`.

## The probe rule

`daemon/src/probe.rs` documents it: one capability per exposed
operation, and a probe vouches for an operation only by a side-effect-free
exercise of that operation's own code path, or by curated knowledge — not by
an adjacent, easier check. The reason is concrete: the touchscreen's version
read succeeds on hardware whose enable command does not, so a version-based
probe would have offered a control that silently does nothing.

Write-only controls have no same-path probe to run, and take one of two other
forms. Asking the firmware whether it implements the exact command the setter
sends (`cmd_version_supported`) is a probe about that command and nothing
else, which is what separates it from the touchscreen trap — that was a
*different* command answering for the one that mattered. Where even that isn't
available, the condition is hardcoded with a comment explaining why. A probe
may also require more than command support: `charge-current-limit` needs a
readable battery too, because a cap is only offered as a share of what the
pack asks for, and a capability should mean the control works rather than
merely that the write exists.

A probe decides what to *offer*, never what to *accept*. Some are proxies:
`fp-brightness-custom` asks for command v1, exactly right for the percentage
write but only a stand-in for the ultra-low and auto levels, which the v0
handler takes on any firmware that has them. Narrowing an offer on a proxy
costs at worst a row nobody could have used; refusing a write on one denies
a call the EC would have honoured — and capabilities are probed once per
daemon lifetime, so one transient read would deny it for the whole run. So
setters validate against the thing itself: `set_fingerprint_level` looks up
the LED node rather than consulting `fp-off`.

## Hardware facts that shape the code

- Keyboard backlight: read via `EcRequestPwmGetKeyboardBacklight` because
  `framework_lib::get_keyboard_backlight()` goes through PWM duty and floors
  twice (5% reads as 4%). The EC is also a second writer (Fn+Space, a
  firmware auto mode on newer boards), which is why the slider polls the EC
  while mapped.
- Charge limit: the EC persists it in BBRAM, but UEFI setup re-sends its own
  stored value at every POST, so an app-set limit lasts until reboot and the
  standing value lives in BIOS setup.
- Battery flags: the EC's discharging flag means "not being charged", not
  "supplying the machine" — a full pack on a connected charger sets it, which
  is a smart battery reporting zero charge current. `framework_tool --power`
  prints "Battery discharging" in that state too. So `charge_flow` ignores it
  and decides from the charging flag, the charger and the rate, which reads a
  clean 0 mA at rest.
- Fingerprint LED: 1–100 (0 rejected — it doubles as the power indicator).
  Percentage and the ultra-low/auto levels need command v1, which older EC
  firmware lacks (framework-system issue #211), so they sit behind the
  `fp-brightness-custom` capability probed with `cmd_version_supported`.
- Fingerprint LED off: the EC has no off — its level command rejects 0, and
  its BBRAM slot reads a 0 back as full brightness, 0 being the uninitialized
  value there. So off is the kernel's LED class instead
  (`/sys/class/leds/chromeos:*:power`), making it the one control that does
  not reach the hardware through the EC and the one whose state nothing can
  read back — hence `EcStamp` dating the darkening, an EC restart returning
  every LED to the EC without the kernel noticing. `led::darken` carries why
  the writes go through the kernel rather than the `EC_CMD_LED_CONTROL` the
  daemon could send itself.
- Haptic touchpad: write-only (firmware ACKs GET_FEATURE with zeros) and
  persists in its own flash across suspend and reboot. The daemon mirrors
  state to `/var/lib/frameguin/state` so it can report what it set; nothing
  is re-applied because the hardware keeps its own state.
- `CrosEc::new()` panics when `framework_lib` finds no driver (aarch64, no
  `/dev/cros_ec`), so the daemon constructs it only behind the DMI vendor
  check and holds it as `Option`, answering with empty capabilities
  elsewhere.

## Build, run, verify

- `cargo build --release`, then `sudo ./install.sh` installs system-wide
  (it kills and restarts a running app). Install and uninstall change system
  files and need sudo, so the user runs them.
- `clippy::pedantic` is on workspace-wide and CI gates on `-D warnings`, so
  both crates build warning-free. CI lints only the binaries, not test code.
- Smoke test: run `target/debug/frameguin`. The app is single-instance, so a
  second launch only activates the resident one — kill it first to exercise
  a fresh build.
- Daemon logs: `sudo journalctl -u frameguin-daemon.service`. Direct calls:
  `busctl call io.github.valeronm.Frameguin /io/github/valeronm/Frameguin
  io.github.valeronm.Frameguin1 GetCapabilities`.
- Non-Framework hardware is a test case in its own right: expected behavior
  is "No Framework hardware detected" in the header, no controls, fast, no
  error toast. Both real regressions so far (port-I/O probe stalls, the
  aarch64 panic) showed up only there.

## Conventions

- Comments explain why, not what, and carry no references to sessions,
  dates, or private context — the repo is public and must read standalone.
- Clippy suppressions live at the site with a `reason`, never in a manifest:
  a manifest allow is invisible where the code is read and blankets the whole
  workspace. `#[expect]` when the suppression is situational, so a stale one
  fails the build; `#[allow]` only when it is permanent by design.
- History is public, so it moves by normal commits; pushed commits are not
  amended.
- "Framework" is Framework Computer Inc.'s trademark. The project name avoids
  using it as a product name, the README carries a non-affiliation
  disclaimer, and "Framework" appears only descriptively ("for Framework
  laptops").
