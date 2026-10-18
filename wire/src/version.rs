//! The version this binary was built as: the manifest's, with the name of
//! the build appended where one was made on top of a release.

// This crate's manifest version is the workspace's.
const RELEASE: &str = env!("CARGO_PKG_VERSION");

// A release is compiled without `FRAMEGUIN_BUILD`; `dev/build.sh` sets it
// to the commit a checkout build was made from.
const BUILD: Option<&str> = match option_env!("FRAMEGUIN_BUILD") {
    Some(name) if !name.is_empty() => Some(name),
    _ => None,
};

/// The version this binary was built as: `X.Y.Z` for a release, and
/// `X.Y.Z+<build>` for a build made on top of one.
#[must_use]
pub fn version() -> String {
    match BUILD {
        Some(build) => format!("{RELEASE}+{build}"),
        None => RELEASE.to_owned(),
    }
}
