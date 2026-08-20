#!/bin/bash
# Installs (default) or removes (--uninstall) the frameguin daemon and app,
# system-wide. Idempotent: re-run after editing any file to update the
# install. Run with sudo.
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "run with sudo" >&2
    exit 1
fi

src="$(cd "$(dirname "$0")" && pwd)"
app_id="io.github.valeronm.Frameguin"
# /usr/local is the FHS slot for software installed outside the package
# manager. Only the binaries follow the prefix: polkit, D-Bus and the icon
# theme read from fixed system directories regardless of where this installs.
prefix="${PREFIX:-/usr/local}"

# Staged outside the build tree so a sudo run leaves no root-owned files in it.
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
data="$staging/data"

# mode:source:destination — the single list both modes use.
files=(
    "755:$src/target/release/frameguin-daemon:$prefix/libexec/frameguin-daemon"
    "755:$src/target/release/frameguin:$prefix/bin/frameguin"
    "644:$data/$app_id.conf:/etc/dbus-1/system.d/$app_id.conf"
    "644:$data/$app_id.service:/usr/share/dbus-1/system-services/$app_id.service"
    # /etc, not /usr/lib: this is a local-admin install, and a packaged one
    # must not find a competing unit shadowing its own.
    "644:$data/frameguin-daemon.service:/etc/systemd/system/frameguin-daemon.service"
    "644:$data/$app_id.policy:/usr/share/polkit-1/actions/$app_id.policy"
    "644:$data/$app_id.desktop:/usr/share/applications/$app_id.desktop"
    "644:$data/$app_id.metainfo.xml:/usr/share/metainfo/$app_id.metainfo.xml"
    "644:$data/icons/$app_id.svg:/usr/share/icons/hicolor/scalable/apps/$app_id.svg"
    "644:$data/icons/$app_id-symbolic.svg:/usr/share/icons/hicolor/symbolic/apps/$app_id-symbolic.svg"
)

if [ "${1:-}" = "--uninstall" ]; then
    # Stop running instances first: the resident tray app would otherwise
    # linger as a broken ghost after its binary and daemon are removed.
    pkill -f "^$prefix/bin/frameguin" 2>/dev/null || true
    systemctl stop frameguin-daemon.service 2>/dev/null || true
    for entry in "${files[@]}"; do
        rm -f "${entry##*:}"
    done
    rm -rf /var/lib/frameguin
    systemctl daemon-reload
    gtk-update-icon-cache -f /usr/share/icons/hicolor 2>/dev/null || true
    echo "uninstalled. Per-user autostart entries remain; remove yours with:"
    echo "  rm -f ~/.config/autostart/$app_id.desktop"
    exit 0
fi

# Warn (but don't refuse — containers, CI, and pre-swap installs are all
# legitimate) when this doesn't look like Framework hardware.
if [ "$(cat /sys/class/dmi/id/sys_vendor 2>/dev/null)" != "Framework" ]; then
    echo "warning: this does not look like a Framework laptop; installing anyway" >&2
    echo "         (the app will show no controls on unsupported hardware)" >&2
fi

if [ ! -x "$src/target/release/frameguin-daemon" ] || [ ! -x "$src/target/release/frameguin" ]; then
    echo "binaries missing — build first (as your user, not root): cargo build --release" >&2
    exit 1
fi

# Stop running app instances so the update takes effect immediately, and
# remember whether the invoking user had one — only that user's instance can
# be restarted (a GUI is launched inside its owner's session).
app_was_running=""
if [ -n "${SUDO_USER:-}" ] && pgrep -u "$SUDO_USER" -f "^$prefix/bin/frameguin" >/dev/null 2>&1; then
    app_was_running=1
fi
pkill -f "^$prefix/bin/frameguin" 2>/dev/null || true

"$src/packaging/render-data.sh" "$prefix/libexec" "$data"

for entry in "${files[@]}"; do
    IFS=: read -r mode source dest <<<"$entry"
    install -Dm"$mode" "$source" "$dest"
done
gtk-update-icon-cache -f /usr/share/icons/hicolor 2>/dev/null || true

systemctl daemon-reload
# Pick up the new bus policy without restarting the bus.
systemctl reload dbus 2>/dev/null || busctl call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig || true
# Restart the daemon if a previous version is running; activation restarts it on demand.
systemctl stop frameguin-daemon.service 2>/dev/null || true

if [ -n "$app_was_running" ] && [ -n "${SUDO_USER:-}" ]; then
    if systemd-run --machine="$SUDO_USER@.host" --user --collect \
        "$prefix/bin/frameguin" --gapplication-service >/dev/null 2>&1; then
        echo "installed and restarted (tray)."
        exit 0
    fi
    echo "note: could not restart the app; launch it from the app grid" >&2
fi

echo "installed. Launch with: frameguin (or from the app grid)"
