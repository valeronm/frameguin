# Frameguin

A small GNOME (GTK4/libadwaita) app for Framework laptop hardware controls,
written in Rust and split into an unprivileged GUI and a root D-Bus daemon.

This is a community project, not affiliated with or endorsed by Framework
Computer Inc. "Framework" and the gear logo are trademarks of Framework
Computer Inc. Licensed under the [MIT License](LICENSE).

<img src="screenshot.png" alt="Frameguin window showing battery, keyboard, fingerprint, and touchpad controls" width="420">


## Requirements

- A Framework laptop. On other hardware the app runs but shows
  "No Framework hardware detected" and no controls.
- A Linux desktop. On stock GNOME the tray icon needs the
  [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/)
  (preinstalled on Ubuntu); KDE and Xfce support it natively.

## What it does

- **Battery charge limit** — via `framework_lib` speaking EC host commands
  directly (the kernel does not expose Framework's charge limit). Quick
  presets (60/80/100%) in the tray menu.
- **Keyboard backlight** — via `framework_lib`; the slider follows changes
  made elsewhere (Fn+Space, firmware auto mode).
- **Fingerprint LED brightness** — level presets (auto/high/medium/low/
  ultra-low) plus a custom percentage, also in the tray menu. Older EC
  firmware supports only high/medium/low; the app detects this and offers
  exactly what the firmware accepts.
- **Haptic touchpad** — click feedback intensity (off/25/50/75/100%) and
  click force (low/medium/high), via `framework_lib` HID feature reports.
  Write-only firmware controls, so the daemon remembers what it set (mirrored
  to /var/lib/frameguin — the touchpad itself persists across reboots).

## Architecture

Controls are capability-driven: the daemon probes the hardware once
(`GetCapabilities`) and the app only shows the groups the board supports, so
new boards work without code changes. The board name in the header comes
from DMI sysfs. Frameguin is a desktop companion to `framework_tool` (the
CLI built on the same `framework_lib`): the same hardware access behind a
resident GNOME UI, no sudo per invocation. It currently covers the everyday
controls, with more of the CLI's surface planned.

Cargo workspace with two crates:

- `daemon/` (`frameguin-daemon`) — owns
  `io.github.valeronm.Frameguin` on the **system bus**, runs as root,
  links `framework_lib` (default features off plus `hidapi`, used by the
  haptic touchpad support and its capability probe).
  D-Bus-activated via systemd (`Type=dbus`), exits after 5 minutes idle.
  Setters are polkit-gated (see Security model below).
- `app/` (`frameguin`) — gtk4-rs + libadwaita GUI talking to the
  daemon over zbus.
- `data/` — D-Bus system bus policy + activation file, systemd unit, polkit
  policy, desktop entries (app grid + autostart), icons.

## Security model

The daemon runs as root (required for `/dev/cros_ec` and hidraw access),
started only by systemd via D-Bus activation, with `ProtectHome`,
`PrivateTmp`, and a systemd-owned `StateDirectory`. Authorization is
layered:

- **Name ownership** — the bus policy allows only root to own
  `io.github.valeronm.Frameguin`, so the daemon can't be impersonated.
- **Reach** — any local user or process may call the daemon; filtering
  happens per method, not per connection.
- **Setters** — every state-changing method asks polkit to authorize the
  message sender (kernel-verified identity) against
  `io.github.valeronm.frameguin.manage`: an **active local session**
  is allowed without a password (the same trust GNOME grants screen
  brightness — someone at the console has the hardware keys anyway);
  inactive sessions are denied; everything else (SSH, daemons) needs admin
  authentication via a polkit agent.
- **Getters** are unauthenticated: they expose only hardware metadata
  (current levels, capabilities, EC version).
- **Surface** — a fixed set of operations with all inputs validated against
  hardware-accepted values; no raw command passthrough.

## Build & install

Rust 1.97+ (pinned in `mise.toml` for [mise](https://mise.jdx.dev) users)
and the system libraries:

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev libudev-dev pkg-config build-essential
```

```sh
cargo build --release
sudo ./install.sh   # idempotent; re-run after rebuilds (restarts a running app)
```

Autostart (tray icon only at login, via GIO's `--gapplication-service`) is
per user: toggle **Start at login** in the app, which writes/removes
`~/.config/autostart/io.github.valeronm.Frameguin.desktop` — or install
the entry by hand:

```sh
install -Dm644 data/io.github.valeronm.Frameguin.autostart.desktop \
    ~/.config/autostart/io.github.valeronm.Frameguin.desktop
```

## Uninstall

```sh
sudo ./install.sh --uninstall
rm -f ~/.config/autostart/io.github.valeronm.Frameguin.desktop
```

## Debugging

```sh
# call the daemon directly
busctl call io.github.valeronm.Frameguin /io/github/valeronm/Frameguin \
    io.github.valeronm.Frameguin1 GetChargeLimit
# daemon logs
sudo journalctl -u frameguin-daemon.service
```

Issues and PRs welcome — capability reports from boards other than the
Laptop 13 Pro are especially useful.
