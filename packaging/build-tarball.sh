#!/bin/bash
# Builds target/dist/frameguin-<version>-<arch>-linux.tar.gz, for distributions
# the .deb cannot serve. The tarball carries install.sh and the unrendered
# data/ templates rather than a prepared tree, so a tarball install and a
# checkout install run the same code and land in the same places.
#
# usage: build-tarball.sh [--expect <version>]
set -euo pipefail

expected=""
case "${1:-}" in
    --expect) expected="${2:?--expect needs a version}" ;;
    "") ;;
    *) echo "usage: $(basename "$0") [--expect <version>]" >&2; exit 2 ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="$(./packaging/check-version.sh "$expected")"

# --locked: a committed lock that disagrees with the manifest is a release
# defect, and cargo would otherwise rewrite it here without saying so.
cargo build --release --workspace --locked

name="frameguin-$version-$(uname -m)-linux"
stage="target/dist/$name"
rm -rf "$stage"
mkdir -p "$stage/packaging"

install -m755 target/release/frameguin target/release/frameguin-daemon install.sh "$stage/"
install -m755 packaging/render-data.sh "$stage/packaging/"
cp -r data "$stage/data"
cp README.md LICENSE "$stage/"

tar -czf "target/dist/$name.tar.gz" -C target/dist "$name"
( cd target/dist && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256" )
# Staging tree removed rather than left beside the tarball: it is under
# target/, which CI caches, and nothing reads it again.
rm -rf "$stage"

echo "target/dist/$name.tar.gz"
