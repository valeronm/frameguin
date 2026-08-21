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
  policy, desktop entry, AppStream metainfo, icons. `*.in` files carry the
  daemon's absolute path and are rendered per prefix by
  `packaging/render-data.sh`.
- `packaging/` — the `.deb` build (`build-deb.sh`, maintainer scripts,
  Debian changelog), the tarball build (`build-tarball.sh`) and its
  downloader (`get.sh`); cargo-deb metadata lives in `app/Cargo.toml`.

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

## Install

Neither of these needs a Rust toolchain or the `-dev` packages.

**Debian and Ubuntu** — take the `.deb` from the
[latest release](https://github.com/valeronm/frameguin/releases/latest):

```sh
sudo apt install ./frameguin_*.deb
```

apt pulls in GTK 4, libadwaita and polkit, and `apt purge frameguin` removes
the lot again.

**Anything else** — a tarball carrying the same installer the source build
uses:

```sh
curl -fsSL https://raw.githubusercontent.com/valeronm/frameguin/main/packaging/get.sh | sh
```

That downloads and unpacks as your user and runs only the installer under
`sudo`. Read it first if you'd rather; it is
[packaging/get.sh](packaging/get.sh), and doing the same by hand is just
`tar -xzf` then `sudo ./install.sh`.

A tarball declares no dependencies, so GTK 4 and libadwaita must already be
present; the installer checks before writing anything.

## Build from source

Rust 1.97+ (pinned in `mise.toml` for [mise](https://mise.jdx.dev) users)
and the system libraries:

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev libudev-dev pkg-config build-essential
```

```sh
cargo build --release
sudo ./install.sh   # idempotent; re-run after rebuilds (restarts a running app)
```

This installs under `/usr/local`, the FHS slot for software outside the
package manager. `PREFIX` moves the two binaries; polkit, D-Bus and the icon
theme only read from fixed system directories, so those files stay put.

### Building a .deb

Packaged with [cargo-deb](https://github.com/kornelski/cargo-deb), pinned in
`mise.toml` alongside the toolchain, so `mise install` provides it. Cargo has
no manifest field for build-time tools; without mise, `cargo install
cargo-deb`.

```sh
./packaging/build-deb.sh                            # target/debian/
./packaging/build-tarball.sh                        # target/dist/
```

The package installs under `/usr`. Don't run it and `install.sh` on the same
machine: they write competing copies, and dpkg only tracks its own.

Autostart (tray icon only at login, via GIO's `--gapplication-service`) is
per user: toggle **Start at login** in the app, which writes/removes
`~/.config/autostart/io.github.valeronm.Frameguin.desktop`. It names the
binary rather than a path, so it survives a move between install prefixes and
goes inert (via `TryExec`) if Frameguin is uninstalled — no uninstaller can
remove a file from another user's home directory.

### Releasing

The version is spelled in several places that nothing derives from each
other. Bump them all, then tag:

- `version` in the workspace `Cargo.toml` (both crates inherit it)
- `packaging/changelog` — a new entry, since cargo-deb ships this verbatim
- `<release version=… date=…/>` in `data/*.metainfo.xml`
- the `v`-prefixed git tag

```sh
./packaging/build-deb.sh --expect 0.2.0       # the whole gate, before tagging
./packaging/build-tarball.sh --expect 0.2.0
```

`--expect` is what CI runs on a tag, so a release that would fail there fails
here first. Pushing the tag builds both the `.deb` and the tarball and
attaches them, with its checksum, to a GitHub release; the same workflow runs
on every push and pull request without the release step.

## Uninstall

```sh
sudo ./install.sh --uninstall                      # source install
sudo apt purge frameguin                           # package install
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
