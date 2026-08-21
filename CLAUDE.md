# Frameguin — working notes for Claude Code

Read README.md first for what the project is. This file covers how to work on
it and the non-obvious constraints.

## Layout and contracts

- Two crates, two binaries: `daemon/` runs as root and links the hardware
  libraries (`framework_lib`, `hidapi`); `app/` is the GTK4/libadwaita UI and
  links no hardware code. The split is the security model — the root process
  carries no GUI, the GUI process has no hardware access — and the D-Bus
  interface `io.github.valeronm.Frameguin1` is their only bridge.
- Capability names (`charge-limit`, `keyboard-backlight`, `fp-brightness`,
  `fp-brightness-custom`, `haptic-touchpad`) are the wire vocabulary and are
  currently spelled in both crates; a shared crate was judged not worth it
  for a handful of strings. Adding a control means: one probe + get/set
  methods in the daemon, one `Capabilities` field and one gated UI group in
  the app.
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

`get_capabilities` in the daemon documents it: one capability per exposed
operation, and a probe vouches for an operation only by a side-effect-free
exercise of that operation's own code path, or by curated knowledge — not by
an adjacent, easier check. The reason is concrete: the touchscreen's version
read succeeds on hardware whose enable command does not, so a version-based
probe would have offered a control that silently does nothing. Where no
harmless same-path probe exists (write-only controls), the support condition
is hardcoded with a comment explaining why.

## Hardware facts that shape the code

- Keyboard backlight: read via `EcRequestPwmGetKeyboardBacklight` because
  `framework_lib::get_keyboard_backlight()` goes through PWM duty and floors
  twice (5% reads as 4%). The EC is also a second writer (Fn+Space, a
  firmware auto mode on newer boards), which is why the slider polls the EC
  while mapped.
- Charge limit: the EC persists it in BBRAM, but UEFI setup re-sends its own
  stored value at every POST, so an app-set limit lasts until reboot and the
  standing value lives in BIOS setup.
- Fingerprint LED: 1–100 (0 rejected — it doubles as the power indicator).
  Percentage and the ultra-low/auto levels need command v1, which older EC
  firmware lacks (framework-system issue #211), so they sit behind the
  `fp-brightness-custom` capability probed with `cmd_version_supported`.
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
