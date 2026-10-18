#!/bin/bash
# Builds this checkout stamped with its commit and installs it system-wide:
# the build as the user, so target/ stays theirs, and the install under
# sudo. Arguments go to cargo.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

dev/build.sh "$@"
sudo ./install.sh
