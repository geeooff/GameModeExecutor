//! Running the update once the file is verified, and reading afterwards
//! whether it took.
//!
//! An installed copy hands the package to `msiexec /qn` through a hidden
//! shell that waits for it: the package itself stops this watcher with a
//! handover and starts the new one, so on success nothing here is left to
//! do; on failure the shell writes why and runs the logon task, so the
//! previous watcher comes back and reports it. An unpacked copy's shell
//! waits for this process to exit, expands the archive over the folder
//! with the old executables kept as `.old`, and runs `install-task`.
//!
//! `pending.txt` is written before anything is launched and read by
//! whichever watcher starts next: the same version, the update took;
//! another, it did not, and `result.txt` says why when the shell got as
//! far as writing it.

use std::path::Path;
use std::process::Child;

use super::{Context, Fault, Kind, Release, Version};
use crate::logging::target;
use crate::shell;

const PENDING: &str = "pending.txt";
const RESULT: &str = "result.txt";
const INSTALL_LOG: &str = "install.log";

pub enum Launched {
    /// The shell running `msiexec`; waiting on it says whether the
    /// installer refused before stopping this process.
    Installer(Child),
    /// The shell waiting for this process to exit; nothing to wait on.
    Shell,
}

/// What the last update left behind, read at start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Settled {
    /// No update was pending.
    Nothing,
    /// The update took: this is the version it installed, running for the
    /// first time -- the moment to say so, since the install itself went by
    /// in a second on a fast machine (the maintainer's remark, 2026-09-18).
    Updated(Version),
    /// The update did not take, and this is why.
    Failed(Fault),
}

/// Read what the last update left behind, tidy the folder, and say what
/// the menu and a notification should carry.
pub fn settle(context: &Context) -> Settled {
    let dir = &context.updates_dir;
    let pending = std::fs::read_to_string(dir.join(PENDING)).ok();
    let result = std::fs::read_to_string(dir.join(RESULT)).ok();
    let running = Version::running();
    let verdict = match pending.as_deref().map(str::trim).and_then(Version::parse) {
        None => None,
        Some(version) if version == running => {
            tracing::info!(target: target::UPDATE, "Updated to {running}");
            tidy(context, true);
            return Settled::Updated(running);
        }
        Some(version) => {
            let note = result
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned);
            // The shell's note first; Windows Installer's own verdict next,
            // read from the log it wrote -- which can say the update took
            // even though this is another version: measured on 2026-09-18,
            // when the version installed was one that did not know this
            // file, and a build reinstalled by hand read it afterwards.
            let why = match (note, installer_status(&dir.join(INSTALL_LOG))) {
                (Some(note), _) => note,
                (None, Some(0)) => {
                    tracing::info!(
                        target: target::UPDATE,
                        "The update to {version} was installed, and this is {running} by other means"
                    );
                    tidy(context, true);
                    return Settled::Nothing;
                }
                (None, Some(code)) => format!("Windows Installer {code}"),
                (None, None) => "the update did not take".to_owned(),
            };
            tracing::warn!(
                target: target::UPDATE,
                log = %dir.join(INSTALL_LOG).display(),
                "The update to {version} failed and this is still {running}: {why}"
            );
            Some(Fault::Setup(format!("Update to {version} failed: {why}")))
        }
    };
    tidy(context, verdict.is_none());
    verdict.map_or(Settled::Nothing, Settled::Failed)
}

/// Windows Installer's own verdict on the log it wrote: the number after
/// `Installation success or error status:` on its last line. The log is
/// UTF-16 with a byte-order mark, as `msiexec /l*v` writes it.
fn installer_status(log: &Path) -> Option<i32> {
    let bytes = std::fs::read(log).ok()?;
    let text = if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    const MARK: &str = "Installation success or error status: ";
    let after = &text[text.rfind(MARK)? + MARK.len()..];
    after
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// Empty the updates folder of everything but the installer's log, and
/// drop the `.old` executables an unpacked copy keeps until its new
/// version has started -- which it has, if this runs.
fn tidy(context: &Context, all: bool) {
    if let Ok(entries) = std::fs::read_dir(&context.updates_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_log = path.extension().is_some_and(|ext| ext == "log");
            if is_log && !all {
                continue;
            }
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    if context.kind() == Kind::Zip {
        for name in ["gamemode-executor.exe.old", "gamemode-executorw.exe.old"] {
            let _ = std::fs::remove_file(context.install_dir.join(name));
        }
    }
}

/// The shell line for an installed copy: `msiexec /qn` waited for, and on
/// failure a note and the logon task, so the previous watcher comes back.
fn installer_script(file: &Path, log: &Path, result: &Path) -> String {
    [
        format!(
            "$p = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', {}, '/qn', '/l*v', {}) -Wait -PassThru",
            shell::quoted(file),
            shell::quoted(log)
        ),
        format!(
            "if ($p.ExitCode -ne 0) {{ Set-Content -LiteralPath {} -Value ('Windows Installer ' + $p.ExitCode); schtasks /Run /TN '{}' | Out-Null }}",
            shell::quoted(result),
            crate::task::TASK_NAME
        ),
    ]
    .join("; ")
}

/// The shell line for an unpacked copy: wait for the process `pid`, expand
/// the archive, keep the old executables as `.old`, copy the new files
/// over, `install-task`. On any failure: a note, the old executables back,
/// `install-task` all the same, so a watcher runs either way.
fn zip_script(
    pid: u32,
    file: &Path,
    unpacked: &Path,
    source: &Path,
    install_dir: &Path,
    result: &Path,
) -> String {
    let twin = shell::quoted(&install_dir.join("gamemode-executorw.exe"));
    let dir = shell::quoted(install_dir);
    let names = "'gamemode-executor.exe', 'gamemode-executorw.exe'";
    [
        format!("Wait-Process -Id {pid} -ErrorAction SilentlyContinue"),
        "try {".to_owned(),
        format!(
            "Expand-Archive -LiteralPath {} -DestinationPath {} -Force -ErrorAction Stop",
            shell::quoted(file),
            shell::quoted(unpacked)
        ),
        format!(
            "foreach ($n in {names}) {{ Move-Item -LiteralPath (Join-Path {dir} $n) -Destination (Join-Path {dir} ($n + '.old')) -Force -ErrorAction Stop }}"
        ),
        format!(
            "Copy-Item -Path (Join-Path {} '*') -Destination {dir} -Recurse -Force -ErrorAction Stop",
            shell::quoted(source)
        ),
        format!("& {twin} install-task"),
        "} catch {".to_owned(),
        format!(
            "Set-Content -LiteralPath {} -Value ('zip: ' + $_.Exception.Message)",
            shell::quoted(result)
        ),
        format!(
            "foreach ($n in {names}) {{ $old = Join-Path {dir} ($n + '.old'); if (Test-Path -LiteralPath $old) {{ Move-Item -LiteralPath $old -Destination (Join-Path {dir} $n) -Force }} }}"
        ),
        format!("& {twin} install-task"),
        "}".to_owned(),
    ]
    .join("; ")
}

/// Launch the update. The file has been verified by the caller.
pub fn launch(context: &Context, release: &Release, file: &Path) -> Result<Launched, Fault> {
    let dir = &context.updates_dir;
    std::fs::write(dir.join(PENDING), release.version.to_string())
        .map_err(|error| Fault::write(&dir.join(PENDING), &error))?;
    let _ = std::fs::remove_file(dir.join(RESULT));
    let result = dir.join(RESULT);
    match context.kind() {
        Kind::Installer => {
            let script = installer_script(file, &dir.join(INSTALL_LOG), &result);
            tracing::info!(
                target: target::UPDATE,
                package = %file.display(),
                "Installing {}; the watcher stops now and comes back on the new version",
                release.version
            );
            let child = shell::hidden(&script).map_err(|error| {
                Fault::Unexpected(format!("cannot start the installer: {error:#}"))
            })?;
            Ok(Launched::Installer(child))
        }
        Kind::Zip => {
            let unpacked = dir.join("unpacked");
            let source = unpacked.join(format!("GameModeExecutor-{}", release.version));
            let script = zip_script(
                std::process::id(),
                file,
                &unpacked,
                &source,
                &context.install_dir,
                &result,
            );
            tracing::info!(
                target: target::UPDATE,
                archive = %file.display(),
                folder = %context.install_dir.display(),
                "Installing {}; the watcher stops now and comes back on the new version",
                release.version
            );
            shell::hidden(&script)
                .map_err(|error| Fault::Unexpected(format!("cannot start the shell: {error:#}")))?;
            Ok(Launched::Shell)
        }
    }
}

/// Wait for the installer's shell and read the note it leaves on failure.
/// `None` is no note: the installer returned success. From a watcher that
/// is still running afterwards, that is its own kind of failure -- the
/// package should have stopped it -- and the caller says so.
pub fn wait(context: &Context, mut child: Child) -> Option<Fault> {
    let _ = child.wait();
    let note = std::fs::read_to_string(context.updates_dir.join(RESULT))
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())?;
    let _ = std::fs::remove_file(context.updates_dir.join(PENDING));
    let _ = std::fs::remove_file(context.updates_dir.join(RESULT));
    Some(
        match note
            .strip_prefix("Windows Installer ")
            .and_then(|code| code.trim().parse::<i32>().ok())
        {
            Some(code) => Fault::Installer { code },
            None => Fault::Setup(note),
        },
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn scratch() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("gme-install-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn context(kind: Kind, dir: &Path) -> Context {
        Context {
            repository: "https://example.invalid".to_owned(),
            kind: Some(kind),
            updates_dir: dir.join("updates"),
            install_dir: dir.join("program"),
            stop: None,
            wake: None,
        }
    }

    #[test]
    fn nothing_pending_means_nothing_to_say_and_a_tidy_folder() {
        let dir = scratch();
        let context = context(Kind::Installer, &dir);
        std::fs::create_dir_all(&context.updates_dir).unwrap();
        std::fs::write(context.updates_dir.join("GameModeExecutor-0.2.0.msi"), b"x").unwrap();
        std::fs::create_dir_all(context.updates_dir.join("unpacked")).unwrap();
        assert_eq!(settle(&context), Settled::Nothing);
        assert!(
            std::fs::read_dir(&context.updates_dir)
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn the_running_version_pending_means_the_update_took() {
        let dir = scratch();
        let context = context(Kind::Zip, &dir);
        std::fs::create_dir_all(&context.updates_dir).unwrap();
        std::fs::create_dir_all(&context.install_dir).unwrap();
        std::fs::write(
            context.updates_dir.join(PENDING),
            Version::running().to_string(),
        )
        .unwrap();
        std::fs::write(context.updates_dir.join(INSTALL_LOG), b"log").unwrap();
        let old = context.install_dir.join("gamemode-executorw.exe.old");
        std::fs::write(&old, b"old").unwrap();
        assert_eq!(settle(&context), Settled::Updated(Version::running()));
        assert!(!old.exists(), "the previous executable is dropped");
        assert!(
            !context.updates_dir.join(INSTALL_LOG).exists(),
            "everything goes on success"
        );
    }

    #[test]
    fn another_version_pending_means_it_did_not_take_and_the_note_says_why() {
        let dir = scratch();
        let context = context(Kind::Installer, &dir);
        std::fs::create_dir_all(&context.updates_dir).unwrap();
        std::fs::write(context.updates_dir.join(PENDING), "99.0.0").unwrap();
        std::fs::write(context.updates_dir.join(RESULT), "Windows Installer 1603\n").unwrap();
        std::fs::write(context.updates_dir.join(INSTALL_LOG), b"log").unwrap();
        assert_eq!(
            settle(&context),
            Settled::Failed(Fault::Setup(
                "Update to 99.0.0 failed: Windows Installer 1603".to_owned()
            ))
        );
        assert!(
            context.updates_dir.join(INSTALL_LOG).exists(),
            "the log stays for reading"
        );
        assert!(!context.updates_dir.join(PENDING).exists(), "said once");
        assert_eq!(settle(&context), Settled::Nothing, "and not again");
    }

    /// A log the way `msiexec /l*v` writes one: UTF-16, a byte-order mark,
    /// and the verdict on the last line.
    fn installer_log(dir: &Path, status: i32) {
        let text = format!(
            "MSI (s) (64:B8) [13:33:31:127]: Product: GameModeExecutor -- Installation completed.\r\n\
             MSI (s) (64:B8) [13:33:31:127]: Windows Installer installed the product. Product Version: 0.1.0. \
             Installation success or error status: {status}.\r\n"
        );
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(dir.join(INSTALL_LOG), bytes).unwrap();
    }

    #[test]
    fn the_installer_log_is_read_for_its_verdict() {
        let dir = scratch();
        installer_log(&dir, 0);
        assert_eq!(installer_status(&dir.join(INSTALL_LOG)), Some(0));
        installer_log(&dir, 1603);
        assert_eq!(installer_status(&dir.join(INSTALL_LOG)), Some(1603));
        std::fs::write(dir.join(INSTALL_LOG), "no verdict here").unwrap();
        assert_eq!(installer_status(&dir.join(INSTALL_LOG)), None);
        assert_eq!(installer_status(&dir.join("absent.log")), None);
    }

    #[test]
    fn a_pending_version_the_installer_reports_installed_is_not_a_failure() {
        // The field case of 2026-09-18: the version installed did not know
        // pending.txt, and a build reinstalled by hand read it afterwards.
        let dir = scratch();
        let context = context(Kind::Installer, &dir);
        std::fs::create_dir_all(&context.updates_dir).unwrap();
        std::fs::write(context.updates_dir.join(PENDING), "99.0.0").unwrap();
        installer_log(&context.updates_dir, 0);
        assert_eq!(settle(&context), Settled::Nothing);
        assert!(!context.updates_dir.join(PENDING).exists());
    }

    #[test]
    fn a_pending_version_with_an_installer_error_names_its_code() {
        let dir = scratch();
        let context = context(Kind::Installer, &dir);
        std::fs::create_dir_all(&context.updates_dir).unwrap();
        std::fs::write(context.updates_dir.join(PENDING), "99.0.0").unwrap();
        installer_log(&context.updates_dir, 1603);
        assert_eq!(
            settle(&context),
            Settled::Failed(Fault::Setup(
                "Update to 99.0.0 failed: Windows Installer 1603".to_owned()
            ))
        );
    }

    #[test]
    fn a_pending_version_with_no_note_is_still_a_failure() {
        let dir = scratch();
        let context = context(Kind::Installer, &dir);
        std::fs::create_dir_all(&context.updates_dir).unwrap();
        std::fs::write(context.updates_dir.join(PENDING), "99.0.0").unwrap();
        assert_eq!(
            settle(&context),
            Settled::Failed(Fault::Setup(
                "Update to 99.0.0 failed: the update did not take".to_owned()
            ))
        );
    }

    #[test]
    fn the_installer_script_runs_msiexec_quietly_and_notes_a_failure() {
        let script = installer_script(
            Path::new(r"C:\u\it's\GameModeExecutor-0.2.0.msi"),
            Path::new(r"C:\u\install.log"),
            Path::new(r"C:\u\result.txt"),
        );
        assert!(script.contains("'msiexec.exe'"));
        assert!(script.contains("'/qn'"), "no window");
        assert!(
            script.contains(r"'C:\u\it''s\GameModeExecutor-0.2.0.msi'"),
            "quotes doubled: {script}"
        );
        assert!(script.contains("-Wait -PassThru"));
        assert!(script.contains("'Windows Installer ' + $p.ExitCode"));
        assert!(script.contains(r"schtasks /Run /TN 'GameModeExecutor\Watcher'"));
    }

    #[test]
    fn the_zip_script_waits_keeps_the_old_files_and_restarts_either_way() {
        let script = zip_script(
            4242,
            Path::new(r"C:\u\GameModeExecutor-0.2.0.zip"),
            Path::new(r"C:\u\unpacked"),
            Path::new(r"C:\u\unpacked\GameModeExecutor-0.2.0"),
            Path::new(r"C:\Tools\GME"),
            Path::new(r"C:\u\result.txt"),
        );
        assert!(script.starts_with("Wait-Process -Id 4242"));
        assert!(script.contains(r"Expand-Archive -LiteralPath 'C:\u\GameModeExecutor-0.2.0.zip'"));
        assert!(
            script.contains("($n + '.old')"),
            "the old executables are kept"
        );
        assert!(
            script.contains(
                r"Copy-Item -Path (Join-Path 'C:\u\unpacked\GameModeExecutor-0.2.0' '*')"
            )
        );
        assert_eq!(
            script.matches("install-task").count(),
            2,
            "restarted on success and on failure"
        );
        assert!(script.contains("} catch {"));
        assert!(script.contains("'zip: ' + $_.Exception.Message"));
    }

    #[test]
    fn waiting_reads_the_note_and_forgets_it() {
        let dir = scratch();
        let context = context(Kind::Installer, &dir);
        std::fs::create_dir_all(&context.updates_dir).unwrap();
        let done = || {
            std::process::Command::new("cmd.exe")
                .args(["/c", "exit 0"])
                .spawn()
                .unwrap()
        };

        std::fs::write(context.updates_dir.join(PENDING), "0.2.0").unwrap();
        std::fs::write(context.updates_dir.join(RESULT), "Windows Installer 1618").unwrap();
        assert_eq!(
            wait(&context, done()),
            Some(Fault::Installer { code: 1618 })
        );
        assert!(!context.updates_dir.join(RESULT).exists());
        assert!(!context.updates_dir.join(PENDING).exists());

        std::fs::write(context.updates_dir.join(RESULT), "zip: no such folder").unwrap();
        assert_eq!(
            wait(&context, done()),
            Some(Fault::Setup("zip: no such folder".to_owned()))
        );

        assert_eq!(wait(&context, done()), None, "no note, no fault");
    }
}
