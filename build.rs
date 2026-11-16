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
    // Re-run when HEAD moves, and when the sources change so the dirty marker
    // is not left behind by an edit.
    watch_head();
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

    embed_resources(&short, !dirty.is_empty());
}

/// The icon Explorer, the task bar and Alt-Tab show for the executables, and
/// the version block the Properties dialog and Windows Installer read.
///
/// Done with `rc.exe` from the Windows SDK and nothing else. Both have to be
/// PE resources -- there is no way to set them from code -- and the SDK's
/// resource compiler is the Microsoft tool for producing them. Anyone who can
/// build this already has it: it ships with the Build Tools that provide the
/// MSVC linker.
///
/// One resource file per binary, because the version block names the file it
/// is in: `OriginalFilename` and `FileDescription` differ between the console
/// executable, the windowless one and the probe. `FileVersion` carries the
/// commit, `ProductVersion` the plain version, and a tree with uncommitted
/// changes is flagged as a private build, which is what Windows calls one.
///
/// Like the commit stamp above, a miss is a warning rather than an error. An
/// executable with no icon works perfectly; a build that refuses to run does
/// not.
fn embed_resources(commit_short: &str, dirty: bool) {
    let manifest =
        std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets this"));
    // The "active" artwork, in its light-background variant: an executable icon
    // cannot follow the theme, and the darker green keeps its definition on the
    // white Explorer background Windows ships with.
    let icon = manifest.join("assets/icons/gamemode-active-light.ico");
    let license = manifest.join("LICENSE");
    println!("cargo:rerun-if-changed={}", icon.display());
    println!("cargo:rerun-if-changed={}", license.display());
    if !icon.exists() {
        println!(
            "cargo:warning=no icon at {}, building without resources",
            icon.display()
        );
        return;
    }
    let Some(rc) = find_resource_compiler() else {
        println!("cargo:warning=rc.exe not found, building without resources");
        return;
    };

    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets this");
    let numeric: Vec<&str> = version.split('.').collect();
    let (major, minor, patch) = (
        numeric.first().copied().unwrap_or("0"),
        numeric.get(1).copied().unwrap_or("0"),
        numeric.get(2).copied().unwrap_or("0"),
    );
    let author = std::env::var("CARGO_PKG_AUTHORS").unwrap_or_default();
    let repository = std::env::var("CARGO_PKG_REPOSITORY").unwrap_or_default();
    // The LICENSE file is the one place the copyright line is written; the
    // resource repeats it rather than keeping a second copy.
    let copyright = std::fs::read_to_string(&license)
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("Copyright"))
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let flags = if dirty { "0x8" } else { "0x0" }; // VS_FF_PRIVATEBUILD
    let private = if dirty {
        concat!(
            r#"      VALUE "PrivateBuild", "Built from a tree with uncommitted changes\0""#,
            "\n"
        )
    } else {
        ""
    };

    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets this"));
    let binaries = [
        ("gamemode-executor", "GameModeExecutor command line"),
        ("gamemode-executorw", "GameModeExecutor watcher"),
        (
            "presence-probe",
            "GameModeExecutor presence writer probe (development tool)",
        ),
    ];
    for (name, description) in binaries {
        let script = out.join(format!("{name}.rc"));
        let compiled = out.join(format!("{name}.res"));
        // Resource id 1: Windows shows the lowest-numbered icon group as the
        // application icon, and 1 is the convention for it. The numeric
        // constants are rc.exe's own, spelled out so no header is needed:
        // FILEOS 0x40004 is VOS_NT_WINDOWS32, FILETYPE 1 is VFT_APP, and the
        // string block is en-US in Unicode. The script is rc.exe's language,
        // where `\` begins an escape inside a quoted string: every value put
        // in one goes through `rc_string`, and each `\0` written here is the
        // terminator rc.exe expects.
        let contents = format!(
            r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3f
FILEFLAGS {flags}
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "{author}\0"
      VALUE "FileDescription", "{description}\0"
      VALUE "FileVersion", "{version} ({commit})\0"
      VALUE "InternalName", "{name}\0"
      VALUE "LegalCopyright", "{copyright}. MIT License.\0"
      VALUE "OriginalFilename", "{name}.exe\0"
      VALUE "ProductName", "GameModeExecutor\0"
      VALUE "ProductVersion", "{version}\0"
      VALUE "Comments", "{repository}\0"
{private}    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
            icon = rc_string(&icon.display().to_string()),
            major = major,
            minor = minor,
            patch = patch,
            flags = flags,
            author = rc_string(&author),
            description = rc_string(description),
            version = rc_string(&version),
            commit = rc_string(commit_short),
            name = rc_string(name),
            copyright = rc_string(&copyright),
            repository = rc_string(&repository),
            private = private,
        );
        if std::fs::write(&script, contents).is_err() {
            println!(
                "cargo:warning=cannot write the resource script for {name}, building it without resources"
            );
            continue;
        }
        let status = Command::new(&rc)
            .args(["/nologo", "/fo"])
            .arg(&compiled)
            .arg(&script)
            .status();
        match status {
            Ok(status) if status.success() => {
                // This binary only: the library has no resources to carry and
                // each executable describes itself.
                println!("cargo:rustc-link-arg-bin={name}={}", compiled.display());
            }
            _ => println!("cargo:warning=rc.exe failed for {name}, building it without resources"),
        }
    }
}

/// A value for a quoted string in rc.exe's script, where `\` begins an
/// escape as in C and a quote is written twice. The icon's path is the one
/// value with backslashes today; the others go through it too, so a quote
/// in a description or a backslash in a copyright line cannot break the
/// script.
fn rc_string(value: &str) -> String {
    value.replace('\\', r"\\").replace('"', r#""""#)
}

/// Finds the SDK's resource compiler: the developer prompt's own variable
/// first, then the newest version under the installed Windows Kit.
fn find_resource_compiler() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("WindowsSdkVerBinPath") {
        for arch in ["x64", "x86"] {
            let candidate = std::path::Path::new(&dir).join(arch).join("rc.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    let program_files =
        std::env::var_os("ProgramFiles(x86)").or_else(|| std::env::var_os("ProgramFiles"))?;
    let bin = std::path::Path::new(&program_files)
        .join("Windows Kits")
        .join("10")
        .join("bin");

    // Version directories sort lexicographically in the order we want -- they
    // are zero-padded 10.0.NNNNN.0 -- so the last one is the newest.
    let mut versions: Vec<_> = std::fs::read_dir(&bin)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    versions.sort();

    versions.iter().rev().find_map(|version| {
        ["x64", "x86"]
            .iter()
            .map(|arch| version.join(arch).join("rc.exe"))
            .find(|candidate| candidate.exists())
    })
}

/// Ask cargo to re-run this script whenever the checked-out commit changes.
///
/// Watching `.git/HEAD` alone is the obvious version and it is wrong: on a
/// branch that file holds `ref: refs/heads/<name>` and does not change when you
/// commit -- the file it points at does. A release once shipped carrying the
/// commit before it, and a `-dirty` marker from a tree that was clean by then,
/// because of exactly this.
///
/// Both are watched: the ref for the normal case, `HEAD` itself for a detached
/// checkout, where it holds the commit directly.
fn watch_head() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    let Ok(head) = std::fs::read_to_string(".git/HEAD") else {
        return;
    };
    if let Some(reference) = head.trim().strip_prefix("ref: ") {
        println!("cargo:rerun-if-changed=.git/{reference}");
        // Once packed, the loose ref file stops existing and this is where the
        // value lives instead.
        println!("cargo:rerun-if-changed=.git/packed-refs");
    }
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
