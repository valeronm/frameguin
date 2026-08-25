#!/bin/bash
# Prints the newest changelog stanza as Markdown, for the body of a GitHub
# release. Without it the curated bullets reach only `apt changelog` on an
# already-installed package, while the release page — where the README's
# "Latest release" link sends everyone — carries whatever GitHub generates
# from the commit titles.
#
# usage: release-notes.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

# dpkg-parsechangelog rather than a regex, and the same call check-version.sh
# makes, so the notes and the version gate cannot disagree about which stanza
# is the newest one.
dpkg-parsechangelog -l packaging/changelog -SChanges | awk '
    function flush() { if (item != "") { print item; item = "" } }
    # The field opens with the source/version line the release page already
    # carries in its title.
    !body { if ($0 ~ /^[^ ]/) body = 1; next }
    # deb822 spells a blank line as a lone dot.
    $0 == "." { flush(); if (started) pending = 1; next }
    { sub(/^  /, "") }
    # A bullet wrapped across source lines is joined back into one: GitHub
    # renders release bodies with hard line breaks on, so every newline the
    # changelog wraps at would reach the page as a <br>.
    /^ +/ { sub(/^ +/, ""); item = item " " $0; next }
    { flush(); if (pending) { print ""; pending = 0 }; started = 1; item = $0 }
    END { flush() }
'
