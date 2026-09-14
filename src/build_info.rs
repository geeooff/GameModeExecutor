//! What this binary is, and where its documentation lives.
//!
//! The commit is stamped in at build time by `build.rs`. It lives here rather
//! than in the configuration file on purpose: `config.toml` belongs to the
//! user, who keeps it across upgrades, so a build-time constant sitting in it
//! would be wrong -- not merely old -- the first time the executables are
//! replaced and the file is not. Compiled in, it cannot drift from the code it
//! describes.
//!
//! Everything here is a constant. `build.rs` takes the decisions -- known
//! commit or not, which reference the documentation link should use -- so that
//! nothing has to be assembled at runtime, which is also what clap requires of
//! a version string.

/// Full commit, or `unknown` when built outside a git checkout.
pub const COMMIT: &str = env!("GIT_COMMIT");

/// Empty, or `-dirty` when the tree had uncommitted changes.
pub const DIRTY: &str = env!("GIT_DIRTY");

/// The commit as a person should read it, excuse included when there is none.
pub const COMMIT_DISPLAY: &str = env!("GIT_COMMIT_DISPLAY");

pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// One line: `0.1.0 (de538e3f-dirty)`. What `-V` prints.
pub const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("GIT_COMMIT_SHORT"),
    env!("GIT_DIRTY"),
    ")"
);

/// The documentation for *this* build.
///
/// A link to a branch would show whatever that branch says today, which may
/// describe a version the reader is not running. A link to the commit cannot
/// rot. When the commit is unknown there is nothing better than the branch, and
/// the version output says so rather than pretending.
pub const DOCS_URL: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/blob/",
    env!("GIT_DOC_REF"),
    "/docs/getting-started.md"
);

/// What `--version` prints: everything worth knowing when someone hands you a
/// binary and asks what it is.
pub const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("GIT_COMMIT_SHORT"),
    env!("GIT_DIRTY"),
    ")\ncommit:        ",
    env!("GIT_COMMIT_DISPLAY"),
    "\nrepository:    ",
    env!("CARGO_PKG_REPOSITORY"),
    "\ndocumentation: ",
    env!("CARGO_PKG_REPOSITORY"),
    "/blob/",
    env!("GIT_DOC_REF"),
    "/docs/getting-started.md"
);

/// True when the commit is unknown, which makes the documentation link
/// approximate rather than exact.
pub fn is_unstamped() -> bool {
    COMMIT == "unknown"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documentation_link_names_this_build() {
        assert!(DOCS_URL.starts_with(REPOSITORY), "{DOCS_URL}");
        assert!(DOCS_URL.ends_with("/docs/getting-started.md"), "{DOCS_URL}");
        if !is_unstamped() {
            assert!(DOCS_URL.contains(COMMIT), "{DOCS_URL}");
            // A branch name here would be the whole bug: the link has to point
            // at the revision this binary was built from, not at whatever the
            // branch says by the time someone clicks it.
            assert!(!DOCS_URL.contains("/blob/main/"), "{DOCS_URL}");
        }
    }

    #[test]
    fn the_version_lines_carry_the_commit() {
        assert!(VERSION.starts_with(env!("CARGO_PKG_VERSION")), "{VERSION}");
        assert!(LONG_VERSION.contains("documentation:"), "{LONG_VERSION}");
        assert!(LONG_VERSION.contains(COMMIT_DISPLAY), "{LONG_VERSION}");
    }
}
