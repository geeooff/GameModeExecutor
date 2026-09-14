//! Stamps the commit the binaries were built from into the binaries.
//!
//! The point is a documentation link that cannot rot: the program can name the
//! exact revision it came from, so `--version` on a machine you are debugging
//! tells you what is actually running rather than roughly which release it was.
//!
//! Nothing here fails a build. Source unpacked from an archive has no `.git`,
//! and a machine without git is a perfectly good machine to compile on; both
//! produce an honest "unknown" rather than an error.

use std::process::Command;

fn main() {
    // Re-run when HEAD moves -- committing changes no tracked file, so cargo
    // would otherwise keep the previous stamp -- and when the sources change,
    // so the dirty marker is not left behind by an edit.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
    let short = commit.get(..8).unwrap_or("unknown").to_owned();

    // `--porcelain` prints one line per change and nothing at all when the tree
    // is clean. Untracked files count: a source file nobody added yet is still
    // a difference between this build and that commit.
    let dirty = match git(&["status", "--porcelain"]) {
        Some(output) if !output.is_empty() => "-dirty",
        _ => "",
    };

    // Every branch is taken here rather than in the program, so the program
    // side is nothing but constants -- which is also what clap needs, since it
    // wants a `&'static str` and will not take a String built at runtime.
    let known = commit != "unknown";
    let doc_ref = if known { commit.as_str() } else { "main" };
    let display = if known {
        format!("{commit}{dirty}")
    } else {
        "unknown (built outside a git checkout)".to_owned()
    };

    println!("cargo:rustc-env=GIT_COMMIT={commit}");
    println!("cargo:rustc-env=GIT_COMMIT_SHORT={short}");
    println!("cargo:rustc-env=GIT_DIRTY={dirty}");
    println!("cargo:rustc-env=GIT_DOC_REF={doc_ref}");
    println!("cargo:rustc-env=GIT_COMMIT_DISPLAY={display}");
}

/// Runs git and returns its trimmed output, or `None` for any reason at all --
/// git missing, not a repository, a failing command.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_owned())
}
