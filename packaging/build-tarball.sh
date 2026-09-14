#!/bin/bash
# Builds target/dist/frameguin-<version>-<arch>-linux.tar.xz, the release
# package. The tarball carries install.sh and the unrendered data/ templates
# rather than a prepared tree, so a tarball install and a checkout install run
# the same code and land in the same places.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="$(sed -n '/^\[workspace\.package\]/,/^\[/ s/^version = "\(.*\)"/\1/p' Cargo.toml)"
if [ -z "$version" ]; then
    echo "no version in [workspace.package] of Cargo.toml" >&2
    exit 1
fi

# --locked: a committed lock that disagrees with the manifest is a release
# defect, and cargo would otherwise rewrite it here without saying so.
cargo build --release --workspace --locked

name="frameguin-$version-$(uname -m)-linux"
stage="target/dist/$name"
rm -rf "$stage"
mkdir -p "$stage"

install -m755 target/release/frameguin target/release/frameguin-daemon install.sh "$stage/"
cp -r data "$stage/data"
cp README.md LICENSE "$stage/"

tar -cJf "target/dist/$name.tar.xz" -C target/dist "$name"
( cd target/dist && sha256sum "$name.tar.xz" >"$name.tar.xz.sha256" )
# Staging tree removed rather than left beside the tarball: it is under
# target/, which CI caches, and nothing reads it again.
rm -rf "$stage"

echo "target/dist/$name.tar.xz"
