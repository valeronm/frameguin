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
- `io.github.valeronm.Frameguin1` is private to those two binaries rather than
  published API. They are built, installed and upgraded as one — `install.sh`
  stops the app and the daemon and brings both back on the new build, and the
  package does the same — so an app talking to a daemon of another version is
  not a state this project has to work in, and nothing outside the pair is a
  caller it answers for. Renaming a method, dropping one or respelling a
  capability is a free change needing no deprecation window. (`busctl` against
  it stays a fine way to inspect a running daemon; it is a debugging tool, not
  a client the interface holds still for.) None of this loosens what `wire/`
  is for: the two ends still restate the interface separately and still meet
  only at runtime, so within one version the vocabularies are what keep them
  from drifting apart.
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
  values behind them and the words those values carry — the chrome around
  them, group and row titles and the sentences a toast makes, stays with the
  widget that is its only site; `caps.rs` holds the probe's answer.
  Neither links GTK or the bus, so the window and the tray cannot disagree
  about what a preset sends or what it is called, and `Capabilities`' private
  field means the app can only offer what the daemon answered with — the
  probe rule, held up by the compiler at this end. `tray.rs` links no GTK
  either, which matters because its menu runs on ksni's own thread; its
  fields are private, so `tray_push` is the only way that state moves.
  `ui.rs` is the largest and stays that way on purpose: `Ui`'s fields are
  private to it, which is what `Sink` and the `apply_*` writes living there
  buys. `battery.rs` is the battery report, the app's one window that only
  reads — no sync guard, no debounce, no tray push — and every field it holds
  is a descendant of the page its subscription hangs on, so nothing in it can
  reach back up the widget tree and outlive the window. `reading.rs` is the
  pack's reading, taken once for however many views show it: the status row
  and the report render the same walk of the same block, and each polling for
  itself made the EC answer twice and let the two windows sit a tick apart, so
  a view subscribes and the feed does the reading; it also holds the two facts
  fixed for a daemon's run that every window wants — the bus connection and
  the probe's answer — so the window, the report and the tray share one of
  each. (`about.rs` dials for itself, deliberately: its report also runs from
  `--debug-info` where no app state exists, and a bug report wants a fresh
  answer rather than one the app is already holding.) `mapped.rs` is
  the rule both timers and subscriptions obey, that nothing repeats while its
  widget is off screen: `while_mapped` takes what `acquire` returns on map and
  drops it on unmap, so stopping is a `Drop` rather than something each caller
  remembers. `board.rs` is the one read
  that bypasses the bus, `about.rs` the report and the dialog that renders it,
  `autostart.rs` the desktop entry whose path and content cannot be written
  apart. `main.rs` keeps what has to know both front-ends — the app id, the
  bus attachment, the lazily built window, the shared reading, and the tray
  event loop, exhaustive over `TrayEvent` so a new variant fails to build
  until it is handled there — along with the application-wide odds and ends
  that belong nowhere else: the command-line options and the actions no
  module of its own owns. The no-GTK rules are the ones nothing
  checks: an import is all it takes to lose one.
- The two windows are reached differently, and which way is decided by whether
  the window survives being closed. The main window hides rather than closing
  wherever there is a tray to hide to, so its slot in `AppState` outlives it —
  and the slot is also where the tray finds the `Rc<Ui>` it reports presets
  into, so `window_for` builds it once and both front-ends take it from there.
  (Where the tray failed to spawn there is no second front-end to reach it,
  which is what keeps that slot honest in the session where the window really
  is destroyed on close.) The report is destroyed on close, so a
  slot would hold a dead window: it goes through a `gio` action on the
  application instead, and finds an already-open copy in the application's own
  window list, which GTK keeps accurate for free. Where a window is opened
  from more than one place, the module owning it owns the `ActionEntry` too
  and keeps its builder private, so the action is not merely the agreed way in
  but the only one that compiles — `battery.rs` does this because both
  front-ends reach the report. `about.rs` does not, and needs not: only the
  window's menu opens it, so `main.rs` holding that entry leaves nothing able
  to drift.
- Inside `daemon/`, the modules are drawn by how a control reaches the
  hardware, so the filename answers which way: `ec.rs` the EC, `led.rs` the
  kernel's LED class, `touchpad.rs` the pad's own HID transport, `panel.rs`
  the touch panel's, `gpio.rs` a pad on the processor through the GPIO
  character device. Two of those pairs need an arbitration, and the two
  arbitrations are not alike: `power_led.rs` because the power button LED has
  two possible drivers and one at a time, so what it settles is a handover and
  the order to make it in; `touchscreen.rs` because the panel has two possible
  routes and a machine has only one, so what it settles is a precedence — and
  then the thing that precedence decides, which is whether the control can be
  read at all. The rest divide by job: `board.rs`
  the DMI reads — the vendor deciding whether there is an EC to open, the
  product name deciding which board's pads are which — `host.rs` what is true
  of this *run* where `board.rs` answers for the machine, which is the
  difference between a fact a reboot changes and one it does not, `state.rs`
  the mirror for what cannot be read back, `probe.rs` the probe rule beside
  the code it governs. Each module's own doc carries why; what is worth saying
  here is what none of them can. `ec.rs` is every EC call: `Ec` is the only
  holder of the `CrosEc`, one method per operation, each taking the lock and
  releasing it before returning and none reaching the handle through another —
  `Mutex` does not re-enter, so a method wanting two commands under one lock
  issues both against the guard it holds. `main.rs` keeps the `Daemon` object,
  polkit, the idle exit and the `#[interface]` surface, and what stays inline
  there is the *order* — validate, skip a write already in place, authorize,
  write. That order is the policy, and splitting the write out would leave it
  legible at neither end.
- A control the tray can also set gets an `apply_*` function owning the whole
  write: the daemon call, the toast, the tray's copy, and moving the widget to
  match. Both the window's handler and the tray item call it; neither writes
  around it. Controls only the window sets (backlight, touchpad) write inline
  in their handler — one caller needs no shared function, and giving it one
  would be ceremony. Writing *by* moving the widget, which the tray used to
  do, makes state the command channel: a widget already showing the requested
  value emits no change, so the write is silently dropped — which is what a
  tray click on a stale window hits. Debouncing stays with the widget that
  needs it; a tray click is discrete.
- Every window widget is already carrying the user's choice when its handler
  runs — `notify` fires after the move, for a combo as much as for a switch —
  so a refused write leaves the window asserting a state the hardware never
  took. Correct it where both hold: the prior value is recoverable without a
  read, and the wrong assertion is one a reader acts on rather than merely
  looks at. The touchscreen is the case that meets them, its prior value
  being the negation and its wrong claim being "touch is off"; everything
  else here would have to capture a value before the write or re-read after
  it, for a stale row that outlives nothing worse than the next reload.
- The tray offers one menu item per control, not per value, and every control
  takes the same shape: a submenu over the states it offers, named after the
  one in force. A value dialled in from the window is not among them, so no
  row is marked — but whether the title still names it is the control's own
  call, and they differ deliberately: the charge ones spell the raw value,
  since a menu that said nothing about a limit set from the window would be
  worse than one naming a row it cannot mark. Two-state controls go through
  the same shape rather than drawing a checkmark; `touchscreen_item` carries
  why. What a row sends is a *state*, never a gesture: a click saying only
  "toggle" would invert whatever
  the app believed by the time it landed, which is the command-channel mistake
  above in its other form — there a widget's position stood in for the
  command, here the gesture would.
- A setter skips a write already in place where a client's idea of the value
  can be stale, and skips it before the polkit call for the same reason
  argument checks come first — nobody should answer a prompt for a write that
  won't happen. The skip belongs in the daemon rather than in a caller, asked
  of the closest thing to the truth each one has: `set_charge_limit` asks the
  EC, `set_charge_current_limit` its own mirror, `set_touchscreen_enabled` the
  pad — and on the route with no pad, nothing: it skips no write at all. A
  mirror is worth skipping on only where the event that invalidates it is the
  one its stamp catches, which holds for the charge current limit and not
  here: the panel's mirror is dated against the boot, and what moves the panel
  inside a boot is dated by nothing, so within one it is no fresher than the
  client's own idea. What decides is
  whether a client can be stale, not whether the value is
  readable: the keyboard backlight is both readable and written by the EC, and
  skips nothing, because the window polls it while mapped and so is not.
- **A date is best effort, never a gate.** Where a write is dated — the power
  LED's darkness against the EC, the touch panel's against the boot — a date
  that cannot be taken costs the record and not the write. What a stamp buys
  is knowing later whether the value still stands, and that is never worth
  refusing a write the hardware would have taken: it trades a control the user
  asked for against a label that is approximate anyway. The writes this
  applies to are state assertions rather than gestures, so re-asserting one
  already in force costs nothing either. Where the dating read is not even on
  the path the write takes — the power LED darkens through the kernel and its
  stamp comes from the EC — refusing is plainly the wrong trade.
- D-Bus types name the value, not either end's convenience: a percentage is
  `y` (`u8`), never GTK's f64. The daemon validates every argument because
  any client can call it — an app-side clamp is UI convenience, not the check.
- In the daemon's `#[interface]` impl the signature carries meaning: `async`
  means the method awaits polkit, `fdo::Result` that it can fail — neither
  implies it touches the EC (`get_capabilities` does, and is neither). zbus
  boxes sync and async alike, so never reach for `async` to get concurrency.
- The daemon's connection runs on one executor thread and every hardware call
  blocks rather than awaits, so a slow one stalls every other task on that
  connection — the cold `get_capabilities` probe most of all, which now walks
  pinctrl for the touchscreen's pad as well as waking the EC. The fix is to
  move the call off the executor onto a blocking pool, not more `async`.

## The probe rule

`daemon/src/probe.rs` documents it: one capability per exposed
operation, and a probe vouches for an operation only by a side-effect-free
exercise of that operation's own code path, or by curated knowledge — not by
an adjacent, easier check. The reason is concrete, and turned out to be worse
than first understood: the touchscreen's version read succeeds on a panel that
has no enable command at all. What stops touch is a pad on the processor,
which the controller neither knows about nor answers for — so a version-based
probe would have vouched not for a command that fails on some hardware, but
for one this panel does not implement. That the Laptop 12's panel does
implement it is the point sharpened: the read said nothing either way.

Write-only controls have no same-path probe to run, and take one of two other
forms. Asking the firmware whether it implements the exact command the setter
sends (`Ec::command_supported`) is a probe about that command and nothing
else, which is what separates it from the touchscreen trap — there a read
answered for a command the panel turned out not to have. Where even that isn't
available, the condition is hardcoded with a comment explaining why. A probe
may also require more than command support: `charge-current-limit` needs a
readable battery too, because a cap is only offered as a share of what the
pack asks for, and a capability should mean the control works rather than
merely that the write exists.

A probe decides what to *offer*, never what to *accept*. Some are proxies:
`power-led-brightness-custom` asks for command v1, exactly right for the
percentage write but only a stand-in for the ultra-low and auto levels, which
the v0 handler takes on any firmware that has them. Narrowing an offer on a
proxy costs at worst a row nobody could have used; refusing a write on one
denies a call the EC would have honoured — and capabilities are probed once
per daemon lifetime, so one transient read would deny it for the whole run. So
setters validate against the thing itself: `set_power_led_level` looks up
the LED node rather than consulting `power-led-off`.

## What the hardware forces on the code

`docs/hardware.md` is what the machine does, a chapter per subsystem — naming
them here as well would be a second index to keep true. Findings belong there
rather than here, since they stay true whoever is talking to the hardware;
what belongs here is what they force on *this* code. The exception is a
finding that is the evidence for a rule stated here, like the touchscreen's
version read under the probe rule — separating those would leave the rule
asserted and its reason a file away.

- The daemon holds `Ec` as an `Option` behind the DMI vendor check, never
  constructing it speculatively: `CrosEc::new()` panics outright where
  `framework_lib` finds no driver. Nothing the EC answers for is offered
  there — the haptic touchpad still is, being reached over HID, which is why
  its probe sits outside that branch.
- A value the EC is a second writer for cannot be shown from what was last
  written — the keyboard backlight's slider polls while mapped for that
  reason. A value with no readback at all is mirrored instead, and what dates
  the mirror is whatever holds the state it claims: nothing for the touchpad,
  which keeps its own in flash; the EC's uptime for the power LED's darkness
  and for the charge current limit, whose mirror expires with the EC that took
  it; the host's boot id for the touch panel, whose controller is expected to
  come up reporting. Which holder a mirror belongs to is settled by evidence
  and not by which stamp is nearer: the charge current limit is the EC's to
  hold, firmware having been shown to leave it where the charge limit and the
  LED level are both re-asserted at POST, so its EC stamp wants no boot stamp
  beside it.
  The touchscreen is both, and which it is depends on the
  route: the pad carries the level, so where a pad is the control the getter
  asks the hardware, and where the panel's own command is, there is nothing
  left to ask and `state.rs` answers from the dated mirror. That
  asymmetry is the reason `touchscreen.rs` exists — a call site that reached
  for one account would have to know which machine it was on. Its second
  writer is the platform firmware rather than this daemon — a lid opening
  drives the pad back, as a resume does — and neither is something the app is
  told about, so both front ends can show a value the firmware has already
  moved. The window
  re-reads on being mapped; the tray asks when its menu opens, which is a
  request it cannot wait for, so the first menu after such a change still
  draws the old value and the one after it is right.
- What the pack is asked directly falls into two groups, and the split is why
  one has a capability and the other does not. The temperature, cell voltages
  and alarms have no fallback, so they are one operation behind
  `BatteryCondition`, probed by the getter's own read and nested under a
  readable pack — a mainboard running standalone must not spend transfers
  asking a battery that is not there what it thinks. The cycle count and the
  manufacturing date each fall back to the EC's answer or to nothing, so they
  need no capability and are read inside `battery_info`, once per run and then
  remembered, absence included.
- A direction is never taken from the EC's flags alone; `charge_flow` weighs
  them against the charger and the rate, because the flags do not mean what
  their names suggest and the charge limiter produces a state they cannot
  express.

## Build, run, verify

- `cargo build --release`, then `sudo ./install.sh` installs system-wide
  (it kills and restarts a running app). Install and uninstall change system
  files and need sudo, so the user runs them.
- `clippy::pedantic` is on workspace-wide and CI gates on `-D warnings`, so
  both crates build warning-free. CI lints only the binaries, not test code.
- CI also gates on `cargo fmt --all --check`, with the style edition pinned
  in `rustfmt.toml` rather than inferred from each crate's own edition — so
  an edition bump cannot reformat the tree as a side effect. Run `cargo fmt`
  before pushing; nothing local enforces it.
- A change that moves the window or the tray menu re-shoots `screenshot.png`
  or `screenshot-tray.png` in the same commit: the metainfo serves both from
  `main`, so between the two commits `main` is wrong.
- `packaging/changelog` is written at release time, not accumulated per
  commit, so a change that will deserve a bullet owes nothing when it lands.
- Cutting a release is `docs/release.md`.
- Smoke test: run `target/debug/frameguin`. The app is single-instance, so a
  second launch only activates the resident one — kill it first to exercise
  a fresh build.
- Daemon logs: `sudo journalctl -u frameguin-daemon.service`. Direct calls:
  `busctl call io.github.valeronm.Frameguin /io/github/valeronm/Frameguin
  io.github.valeronm.Frameguin1 GetCapabilities`.
- Non-Framework hardware is a test case in its own right: expected behavior
  is "No Framework hardware detected" in the header, a status page naming the
  vendor where the controls would be, fast, no error toast. Both real
  regressions so far (port-I/O probe stalls, the aarch64 panic) showed up only
  there.
- A window with no controls says which of its three reasons it is — no
  Framework hardware, a daemon it could not reach, a Framework board that
  answered with no capabilities. They look identical as a bare empty window,
  and only one of the three is a bug worth a report, so the page carries the
  distinction rather than a toast that is gone by the time anyone asks.

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
