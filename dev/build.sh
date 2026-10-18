#!/bin/bash
# Builds the release binaries named after the commit they were built from, so
# an install from a checkout is told apart from the release it was made on.
# Arguments go to cargo; `sudo ./install.sh` installs the result.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

build="$(git rev-parse --short HEAD)"
# An untracked file leaves the tree clean here.
git diff --quiet HEAD || build="$build.dirty"

FRAMEGUIN_BUILD="$build" cargo build --release "$@"
