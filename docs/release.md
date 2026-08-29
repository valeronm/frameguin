# Releasing

How a version of frameguin is cut. The version is spelled in several places
that derive from nothing, and only some of them are checked.

## Cutting a release

From a clean tree on an up-to-date `main`, taking `0.2.0` as the version
throughout.

**1. Bump the version copies.**

- `version` in the workspace `Cargo.toml` — both crates inherit it.
- `packaging/changelog` — a new stanza at the top, its bullets written as
  [Writing the changelog bullets](#writing-the-changelog-bullets) describes.
  Copy the newest stanza and edit it rather than writing the header and
  trailer by hand: the gate parses the file with `dpkg-parsechangelog`, which
  wants the exact Debian shape. The distribution is `unstable`, and the
  version carries a Debian revision suffix (`0.2.0-1`) that the check strips
  before comparing.
- That stanza's trailer is **whoever is cutting this release** — `git config
  user.name` and `user.email`, not the previous stanza's, which is the one
  part copying gets wrong. Its date is `date -R`.
- `data/*.metainfo.xml` — a `<release version="0.2.0" date="…"/>`, the date
  being today in `date -I` form, as the **first** child of `<releases>`. The
  check reads the first entry, so one appended below passes while the newest
  release stays wrong.

**2. Refresh the lock.** Any `cargo` command rewrites `Cargo.lock` for the
bumped version; `cargo check --workspace` is the cheapest. Do this before the
gate: the builds pass `--locked`, so a stale lock stops them with an error the
version check never reaches.

**3. Check the screenshots.** A change that moves the window is supposed to
re-shoot in its own commit, so this is the backstop rather than the step that
does it:

```sh
git diff "$(git describe --tags --abbrev=0)"..HEAD -- app/src/window/ app/src/tray.rs
```

Those are the files that draw what the two screenshots show. A non-empty diff
means checking `screenshot.png` and `screenshot-tray.png` against the running
app and re-shooting by hand — there is no script for it, so an agent stops
here and hands back.

**4. Run the gate.**

```sh
./packaging/build-deb.sh --expect 0.2.0
./packaging/build-tarball.sh --expect 0.2.0
```

**5. Commit, and push the branch before the tag.**

```sh
git add Cargo.toml Cargo.lock packaging/changelog data/*.metainfo.xml
git commit -m "Release 0.2.0 with …"   # and the screenshots, if they were re-shot
git push origin main
```

The title names what a reader would notice, as the changelog bullets do. The
branch goes first because the metainfo resolves its URLs against `main`, so
everything it references has to be there before the tag — today that is the
two screenshots.

**6. Tag and push.** Pushing the tag is the whole trigger.

```sh
git tag v0.2.0
git push origin v0.2.0
```

CI builds both packages and creates the release, with the `.deb`, the tarball
and the tarball's checksum attached, and the newest changelog stanza as the
body. `gh run watch` follows the build; if it fails, see [When a version copy
disagrees](#when-a-version-copy-disagrees).

## Writing the changelog bullets

One bullet per change a user can see, never one per commit: most of a range is
refactors, module splits and formatting, and a release names none of them. The
test is whether someone who never reads the repo would notice.

Say what the app now does, and name what it did before wherever the change is
a correction. These bullets are the release page as well as `apt changelog`,
so they are read by someone deciding whether to update, who needs to know
whether the thing that annoyed them is the thing that was fixed.

```
frameguin (0.9.0-1) unstable; urgency=medium

  * Show the charge limit as a percentage, instead of the raw value the EC
    reports.
  * Name the tray icon after the board it found, rather than leaving it
    "Frameguin" on every machine.

 -- Your Name <you@example.org>  Mon, 02 Feb 2026 09:15:00 +0000
```

The package's `Maintainer` is a separate, stable field in `app/Cargo.toml`,
and a release does not touch it. Nothing compares the two: a trailer that
differs from `Maintainer` is the ordinary Debian case, and lintian says
nothing about it.

## Why the gate runs before the tag

`packaging/check-version.sh` is the definition of "releasable as X"; its own
header says what it refuses.

Skipping the gate moves the failure to after a public tag exists, which is
survivable but costs a moved tag. `check-version.sh` runs before any
compilation, so a mismatch fails locally in about a second rather than after a
CI run.

It is a weaker check than CI's: here the expected version is typed by hand,
where CI derives it from the tag, so the tag is the one copy a local run cannot
vouch for. Without `--expect`, only the files are cross-checked.

## When a version copy disagrees

The package build fails, and the release step never runs: nothing is published.
The tag is already public and points at a commit that cannot produce a release.

`./packaging/check-version.sh 0.2.0` prints which copy is wrong. Fix it, re-run
the gate, then move the tag onto the new commit:

```sh
git add -u && git commit -m "…" && git push origin main
./packaging/build-deb.sh --expect 0.2.0
git tag -f v0.2.0
git push --force origin v0.2.0
```

Only the tag is force-moved — the fix is an ordinary commit, so this does not
cross the rule that pushed commits are not amended. Re-pushing the tag rebuilds
the release in place.

## Background: what is deliberately not checked

A check earns its place when something breaks if it is wrong, not when a
disagreement is merely possible. Two things fail that test:

- **The metainfo `<release date=…>`.** Nothing reads it against anything:
  packages order by version and never by date, and the date's one consumer is a
  software centre's version history. Both dates are hand-typed at different
  moments, so a bump either side of midnight would block a green release on a
  cosmetic disagreement.
- **Screenshot staleness.** No script can tell a current screenshot from a
  stale one, and the available heuristic — the UI changed but the image did not
  — fires on formatting passes and module splits. The screenshot check above
  is that heuristic, run by hand where a false positive costs a look rather
  than a red build.
