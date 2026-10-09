#!/bin/bash
# Runs the app against a fake daemon answering as another board, on a private
# bus standing in for the system bus, so nothing of this machine is read or
# written. The arguments are the fake daemon's; run it with none to list
# them.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

# The app is single-instance over the session bus, which this leaves alone:
# a resident one would take the launch and show the real machine.
if pgrep -x frameguin >/dev/null; then
    echo "quit the running frameguin first" >&2
    exit 1
fi

# Building the daemon's package with the feature would put the stubs in
# target/debug/frameguin-daemon as well.
cargo build -p frameguin -p frameguin-daemon --features frameguin-daemon/fake \
    --bin frameguin --bin frameguin-fake-daemon

# A socket path is capped at 108 bytes.
dir="$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/frameguin-fake.XXXXXX")"
started=()
cleanup() {
    # Either may have exited already, or never started.
    kill "${started[@]}" 2>/dev/null || true
    rm -rf "$dir"
}
trap cleanup EXIT

export DBUS_SYSTEM_BUS_ADDRESS="unix:path=$dir/bus"

# A session bus's policy lets any client own any name.
dbus-daemon --session --nofork --address="$DBUS_SYSTEM_BUS_ADDRESS" &
bus=$!
started+=("$bus")
until [ -S "$dir/bus" ]; do
    kill -0 "$bus" 2>/dev/null || exit 1
    sleep 0.1
done

target/debug/frameguin-fake-daemon "$@" &
fake=$!
started+=("$fake")
# Nothing on a private bus activates the daemon, so it has to own its name
# before the app dials.
until busctl --address="$DBUS_SYSTEM_BUS_ADDRESS" status io.github.valeronm.Frameguin >/dev/null 2>&1; do
    kill -0 "$fake" 2>/dev/null || exit 1
    sleep 0.1
done

target/debug/frameguin
