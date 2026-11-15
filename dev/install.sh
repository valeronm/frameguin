#!/bin/bash
# Builds this checkout stamped with its commit and installs it system-wide:
# the build as the user, so target/ stays theirs, and the install under
# sudo. Arguments go to cargo.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

# A build outlasting sudo's timeout would otherwise prompt again at the end.
sudo -v
while sleep 60; do sudo -n -v; done >/dev/null 2>&1 &
keepalive=$!
trap 'kill "$keepalive"' EXIT

dev/build.sh "$@"
sudo ./install.sh
