# Releasing

How a version of frameguin is cut. The version is written by hand once, in
the workspace `Cargo.toml`, and the tag has to agree with it.

## Cutting a release

From a clean tree on an up-to-date `main`, taking `0.2.0` as the version
throughout.

**1. Bump the version.** `version` in the workspace `Cargo.toml` — every
crate inherits it.

**2. Refresh the lock.** Any `cargo` command rewrites `Cargo.lock` for the
bumped version; `cargo check --workspace` is the cheapest. The tarball build
passes `--locked`, so a stale lock stops it.

**3. Check the screenshots.** A change that moves the window is supposed to
re-shoot in its own commit, so this is the backstop rather than the step that
does it:

```sh
git diff "$(git describe --tags --abbrev=0)"..HEAD -- app/src/window/ app/src/tray.rs
```

Those are the files that draw what the two screenshots show. A non-empty diff
means checking `screenshot.png` and `screenshot-tray.png` against the running
app and re-shooting by hand — there is no script for it, so an agent stops
here and hands back. The diff also fires on formatting passes and module
splits, which is why this is a look rather than a check.

**4. Build the tarball.**

```sh
./packaging/build-tarball.sh
```

**5. Commit and push.**

```sh
git add Cargo.toml Cargo.lock
git commit -m "Release 0.2.0 with …"   # and the screenshots, if they were re-shot
git push origin main
```

The title names what a reader would notice, as the release notes do.

**6. Tag and push.** Pushing the tag is the whole trigger.

```sh
git tag v0.2.0
git push origin v0.2.0
```

CI builds the tarball and creates a **draft** release with the tarball and its
checksum attached. `gh run watch` follows the build.

**7. Check the draft, write the notes and publish.** The tarball's name
carries the manifest's version, so a tarball named for another version under
this tag means the bump was missed: fix `Cargo.toml` and move the tag, as
below. Otherwise write the notes as [Writing the release
notes](#writing-the-release-notes) describes and publish. Until then `get.sh`
still installs the previous release.

## Writing the release notes

One bullet per change a user can see, never one per commit: most of a range is
refactors, module splits and formatting, and a release names none of them. The
test is whether someone who never reads the repo would notice.

Dropping the invisible commits is not enough. A bullet stands above the range
rather than beside it: the commits that together built one thing a reader
reaches — a page, a section, a reading — are one bullet between them, so a
release of twelve can have four. A page whose bullets can each be matched to a
commit is the log rewritten, however plainly each one reads on its own.

Describe plainly what the app gained or lost, with no clause setting it
against what it did before: a page where each bullet carries an "instead of"
reads as one sentence repeated.

```markdown
- Show the charge limit as a percentage.
- Name the tray icon after the board it found.
- Remove the raw register dump from the battery window.
```

GitHub renders a release body with hard line breaks, so keep each bullet on
one line.

## Moving the tag

When the draft was built from the wrong commit, fix it with an ordinary
commit and move the tag onto it:

```sh
git add -u && git commit -m "…" && git push origin main
git tag -f v0.2.0
git push --force origin v0.2.0
```

Only the tag is force-moved, so this does not cross the rule that pushed
commits are not amended. Re-pushing the tag refreshes the draft's assets and
leaves its notes alone.
