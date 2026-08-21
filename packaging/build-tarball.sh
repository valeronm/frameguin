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

app_id=io.github.valeronm.Frameguin
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

# Every data/ member install.sh names. cp -r would copy a data/ that had lost
# one without complaining, and the failure would then land at install time on
# someone else's machine. Comparing the staged tree against data/ cannot catch
# this: a missing member is missing on both sides.
for f in frameguin.1.in frameguin-daemon.service.in "$app_id.service.in" \
         "$app_id.conf" "$app_id.policy" "$app_id.desktop" "$app_id.metainfo.xml" \
         icons/"$app_id.svg" icons/"$app_id-symbolic.svg"; do
    if [ ! -e "data/$f" ]; then
        echo "data/$f is missing; install.sh installs it" >&2
        exit 1
    fi
done

tar -czf "target/dist/$name.tar.gz" -C target/dist "$name"
( cd target/dist && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256" )
# Staging tree removed rather than left beside the tarball: it is under
# target/, which CI caches, and nothing reads it again.
rm -rf "$stage"

echo "target/dist/$name.tar.gz"
