//! `purge`: remove every trace of the program, on request, whichever way it
//! was installed.
//!
//! The rule, from `docs/design/08-distribution.md`: it removes what it
//! recognises as its own and leaves the rest, saying what it left. Its own
//! is the logon task, the configuration wherever it was found, the log
//! wherever it was written, the session marker, the two profile folders
//! once they are empty, and last the executables -- through Windows
//! Installer when the installer owns them, through a detached shell that
//! waits for this process to exit when they were unpacked by hand. A task
//! it did not register, a folder holding anything else, a file it does not
//! know: left alone.
//!
//! It refuses while a game session is open. A purge then would leave the
//! machine on its gaming configuration with nothing left to restore it,
//! which is the one thing this must not do. And the watcher is stopped
//! first, gracefully, the way *Quit* stops it.
//!
//! The plan is computed from what exists and shown before anything is
//! removed; `--yes` skips the question for scripts. Computing it is separate
//! from discovering the machine, so the tests can hand it a scratch layout.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::config;
use crate::logging;
use crate::marker;
use crate::service;
use crate::task;
use crate::win;

/// The same value `scripts/msi.ps1` writes into every package. Fixed for the
/// life of the product; a test checks the two copies agree.
pub const UPGRADE_CODE: &str = "{8C4E0B2D-3F6A-4E7B-9A1C-5D2E8F7B6A30}";

/// What the executables' removal has to go through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    /// Windows Installer registered this product; `msiexec /x` removes it,
    /// its registration and its folder.
    Installed { product_code: String },
    /// Unpacked by hand: these files, the zip's documentation tree when it
    /// is there, then the folder if that leaves it empty.
    Unpacked {
        dir: PathBuf,
        files: Vec<PathBuf>,
        docs: Option<PathBuf>,
    },
}

/// Where the program's traces may be on this machine.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub watcher_running: bool,
    pub task_registered: bool,
    pub config_candidates: Vec<PathBuf>,
    pub log: Option<PathBuf>,
    pub marker: Option<PathBuf>,
    pub local_dir: Option<PathBuf>,
    pub roaming_dir: Option<PathBuf>,
    pub exe_dir: Option<PathBuf>,
    pub product_code: Option<String>,
}

/// What a purge will do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub stop_watcher: bool,
    pub remove_task: bool,
    pub files: Vec<PathBuf>,
    /// Removed only if empty once the files are gone, deepest first.
    pub dirs: Vec<PathBuf>,
    pub program: Option<Program>,
}

/// The files a hand-installed copy is made of, beyond the executables:
/// what the zip unpacks next to them.
const BUNDLE_FILES: [&str; 2] = ["LICENSE", "README.txt"];
const EXECUTABLES: [&str; 2] = ["gamemode-executor.exe", "gamemode-executorw.exe"];

impl Plan {
    /// Everything in `layout` that exists, in the order it goes.
    pub fn compute(layout: &Layout) -> Self {
        let mut files = Vec::new();
        let mut push = |path: &Path| {
            if path.is_file() && !files.iter().any(|known: &PathBuf| known == path) {
                files.push(path.to_path_buf());
            }
        };
        for candidate in &layout.config_candidates {
            push(candidate);
        }
        if let Some(log) = &layout.log {
            push(log);
        }
        if let Some(marker) = &layout.marker {
            push(marker);
        }

        // Deepest first, so a parent is judged after its children are gone.
        // The log's folder counts as ours only inside the program's own
        // places; a log sent elsewhere leaves its folder behind.
        let mut dirs = Vec::new();
        let ours = [&layout.local_dir, &layout.exe_dir];
        if let Some(log) = &layout.log
            && let Some(parent) = log.parent()
            && ours
                .into_iter()
                .flatten()
                .any(|place| parent.starts_with(place) && parent != place)
            && parent.is_dir()
        {
            dirs.push(parent.to_path_buf());
        }
        for dir in [&layout.local_dir, &layout.roaming_dir]
            .into_iter()
            .flatten()
        {
            if dir.is_dir() && !dirs.contains(dir) {
                dirs.push(dir.clone());
            }
        }

        let program = match (&layout.product_code, &layout.exe_dir) {
            (Some(code), _) => Some(Program::Installed {
                product_code: code.clone(),
            }),
            (None, Some(dir)) => {
                let files: Vec<PathBuf> = EXECUTABLES
                    .iter()
                    .chain(BUNDLE_FILES.iter())
                    .map(|name| dir.join(name))
                    .filter(|path| path.is_file())
                    .collect();
                // The zip's documentation tree, recognised by its first page.
                let docs = dir.join("docs");
                let docs = docs.join("getting-started.md").is_file().then_some(docs);
                (!files.is_empty()).then(|| Program::Unpacked {
                    dir: dir.clone(),
                    files,
                    docs,
                })
            }
            (None, None) => None,
        };

        Self {
            stop_watcher: layout.watcher_running,
            remove_task: layout.task_registered,
            files,
            dirs,
            program,
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.stop_watcher
            && !self.remove_task
            && self.files.is_empty()
            && self.dirs.is_empty()
            && self.program.is_none()
    }

    /// One line per thing, for the question.
    pub fn describe(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.stop_watcher {
            lines.push("stop the running watcher".to_owned());
        }
        if self.remove_task {
            lines.push(format!("remove the logon task `{}`", task::TASK_NAME));
        }
        for file in &self.files {
            lines.push(format!("delete {}", file.display()));
        }
        for dir in &self.dirs {
            lines.push(format!("remove {} if it is then empty", dir.display()));
        }
        match &self.program {
            Some(Program::Installed { product_code }) => lines.push(format!(
                "uninstall the program through Windows Installer (product {product_code})"
            )),
            Some(Program::Unpacked { dir, files, docs }) => {
                for file in files {
                    lines.push(format!(
                        "delete {}, once this command has exited",
                        file.display()
                    ));
                }
                if let Some(docs) = docs {
                    lines.push(format!("delete the documentation tree {}", docs.display()));
                }
                lines.push(format!("remove {} if it is then empty", dir.display()));
            }
            None => {}
        }
        lines
    }
}

/// What this machine holds, read once. `config_path` is the file the command
/// line resolved to, which may not be either default candidate.
pub fn discover(config: Option<&config::Config>, config_path: &Path) -> Layout {
    let mut candidates = config::candidate_paths();
    if !candidates.iter().any(|candidate| candidate == config_path) {
        candidates.insert(0, config_path.to_path_buf());
    }
    let local_dir = config::local_dir();
    let log_dir = config
        .and_then(|config| config.general.log_dir.clone())
        .or_else(|| local_dir.as_ref().map(|dir| dir.join("logs")));
    Layout {
        watcher_running: win::SingleInstance::is_held(service::INSTANCE),
        task_registered: task::exists(),
        config_candidates: candidates,
        log: log_dir.map(|dir| dir.join(logging::LOG_FILE_NAME)),
        marker: local_dir.as_ref().map(|dir| dir.join(marker::FILE_NAME)),
        local_dir,
        roaming_dir: config::roaming_dir(),
        exe_dir: std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf)),
        product_code: installed_product(),
    }
}

/// Carry the plan out. The executables go last and outlive this process:
/// the caller prints nothing after this returns.
pub fn execute(plan: &Plan) -> Result<()> {
    if plan.stop_watcher {
        service::stop(win::StopReason::Restore)?;
        println!("Watcher stopped.");
    }
    if plan.remove_task {
        task::uninstall()?;
    }
    for file in &plan.files {
        std::fs::remove_file(file).with_context(|| format!("cannot delete {}", file.display()))?;
        println!("Deleted {}.", file.display());
    }
    for dir in &plan.dirs {
        match std::fs::remove_dir(dir) {
            Ok(()) => println!("Removed {}.", dir.display()),
            // Not empty, or already gone: either way it is not ours to force.
            Err(_) => println!("Left {} alone: it holds something else.", dir.display()),
        }
    }
    match &plan.program {
        Some(Program::Installed { product_code }) => {
            // Once this process is gone: it lives in the folder the installer
            // is about to empty, and Windows Installer would find it in use.
            after_exit(&[format!(
                "Start-Process msiexec.exe -ArgumentList '/x {product_code} /passive'"
            )])?;
            println!("Windows Installer will now remove the program.");
        }
        Some(Program::Unpacked { dir, files, docs }) => {
            let mut steps: Vec<String> = files
                .iter()
                .map(|file| {
                    format!(
                        "Remove-Item -LiteralPath {} -Force -ErrorAction SilentlyContinue",
                        quoted(file)
                    )
                })
                .collect();
            if let Some(docs) = docs {
                steps.push(format!(
                    "Remove-Item -LiteralPath {} -Recurse -Force -ErrorAction SilentlyContinue",
                    quoted(docs)
                ));
            }
            // The folder itself: only if that left it empty, which is what
            // Remove-Item without -Recurse does.
            steps.push(format!(
                "Remove-Item -LiteralPath {} -ErrorAction SilentlyContinue",
                quoted(dir)
            ));
            after_exit(&steps)?;
            println!("The executables will be deleted once this command has exited.");
            // A folder cannot go while a shell sits in it, and the shell this
            // was typed into usually does. Seen in the field on 2026-09-17.
            if std::env::current_dir().is_ok_and(|here| here.starts_with(dir)) {
                println!(
                    "This window is inside {}; the folder itself stays until you leave it.",
                    dir.display()
                );
            }
        }
        None => {}
    }
    Ok(())
}

/// The product code Windows Installer registered for this upgrade code, if
/// the program was installed from the package.
pub fn installed_product() -> Option<String> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::ApplicationInstallationAndServicing::MsiEnumRelatedProductsW;
    use windows::core::{HSTRING, PWSTR};

    let upgrade = HSTRING::from(UPGRADE_CODE);
    // A product code is 38 characters plus the terminator.
    let mut buffer = [0u16; 39];
    // SAFETY: `upgrade` outlives the call, and `buffer` is exactly the size
    // the function documents for a product code, written in place.
    let result = unsafe { MsiEnumRelatedProductsW(&upgrade, None, 0, PWSTR(buffer.as_mut_ptr())) };
    if result != ERROR_SUCCESS.0 {
        return None;
    }
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

/// A path as a PowerShell single-quoted literal, which only a quote can
/// end -- doubled inside, and nothing else expands.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

/// Run PowerShell statements once this process has exited, in a window
/// nobody sees.
///
/// Windows PowerShell rather than `cmd.exe`, because it can wait for
/// exactly this process -- `Wait-Process` on our own id -- where a batch
/// line could only guess with a delay. Each step says for itself what it
/// does when its target is already gone. `CREATE_NO_WINDOW` gives it a hidden console of its own;
/// outliving this process needs no flag, Windows does not end children with
/// their parent. Only single quotes reach the command line, so std's
/// quoting for `CommandLineToArgvW` carries it through intact.
fn after_exit(steps: &[String]) -> Result<()> {
    after_process(std::process::id(), steps)
}

/// The same, once the process `pid` has exited -- which is how the tests
/// run the steps without exiting themselves.
fn after_process(pid: u32, steps: &[String]) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut script = vec![format!(
        "Wait-Process -Id {pid} -ErrorAction SilentlyContinue"
    )];
    script.extend(steps.iter().cloned());
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
        ])
        .arg(script.join("; "))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("cannot start the shell that finishes the removal")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn scratch() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "gamemode-executor-purge-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn the_plan_lists_only_what_exists() {
        let root = scratch();
        let local = root.join("local");
        let roaming = root.join("roaming");
        let exe_dir = root.join("program");
        touch(&roaming.join("config.toml"));
        touch(&local.join("logs").join("gamemode-executor.log"));
        touch(&exe_dir.join("gamemode-executor.exe"));
        touch(&exe_dir.join("LICENSE"));
        let layout = Layout {
            watcher_running: false,
            task_registered: true,
            config_candidates: vec![exe_dir.join("config.toml"), roaming.join("config.toml")],
            log: Some(local.join("logs").join("gamemode-executor.log")),
            marker: Some(local.join(marker::FILE_NAME)),
            local_dir: Some(local.clone()),
            roaming_dir: Some(roaming.clone()),
            exe_dir: Some(exe_dir.clone()),
            product_code: None,
        };

        let plan = Plan::compute(&layout);

        assert!(!plan.stop_watcher);
        assert!(plan.remove_task);
        // The config next to the executable and the marker do not exist.
        assert_eq!(
            plan.files,
            vec![
                roaming.join("config.toml"),
                local.join("logs").join("gamemode-executor.log")
            ]
        );
        assert_eq!(
            plan.dirs,
            vec![local.join("logs"), local.clone(), roaming.clone()]
        );
        assert_eq!(
            plan.program,
            Some(Program::Unpacked {
                dir: exe_dir.clone(),
                files: vec![
                    exe_dir.join("gamemode-executor.exe"),
                    exe_dir.join("LICENSE")
                ],
                docs: None,
            })
        );
    }

    #[test]
    fn an_installed_product_goes_through_the_installer() {
        let layout = Layout {
            product_code: Some("{00000000-0000-0000-0000-000000000000}".to_owned()),
            exe_dir: Some(PathBuf::from(r"C:\nowhere")),
            ..Layout::default()
        };
        let plan = Plan::compute(&layout);
        assert_eq!(
            plan.program,
            Some(Program::Installed {
                product_code: "{00000000-0000-0000-0000-000000000000}".to_owned()
            })
        );
        assert!(plan.files.is_empty() && plan.dirs.is_empty());
    }

    #[test]
    fn nothing_there_is_an_empty_plan() {
        let plan = Plan::compute(&Layout::default());
        assert!(plan.is_empty());
        assert!(plan.describe().is_empty());
    }

    #[test]
    fn a_log_sent_elsewhere_is_deleted_but_its_folder_is_not_ours() {
        let root = scratch();
        let elsewhere = root.join("elsewhere");
        touch(&elsewhere.join("gamemode-executor.log"));
        let layout = Layout {
            log: Some(elsewhere.join("gamemode-executor.log")),
            local_dir: Some(root.join("local")),
            ..Layout::default()
        };
        let plan = Plan::compute(&layout);
        assert_eq!(plan.files, vec![elsewhere.join("gamemode-executor.log")]);
        assert!(plan.dirs.is_empty(), "{:?}", plan.dirs);
    }

    /// The package builder and this module must agree on the upgrade code,
    /// or `purge` on an installed copy would fall back to deleting files
    /// under Windows Installer's feet.
    #[test]
    fn the_upgrade_code_matches_the_package_builder() {
        let script = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts")
                .join("msi.ps1"),
        )
        .expect("scripts/msi.ps1 is in the repository");
        assert!(
            script.contains(&format!("$UpgradeCode = '{UPGRADE_CODE}'")),
            "scripts/msi.ps1 does not carry {UPGRADE_CODE}"
        );
    }

    #[test]
    fn executing_an_unpacked_plan_removes_files_and_empty_folders() {
        let root = scratch();
        let local = root.join("local");
        let kept = root.join("kept");
        touch(&local.join("logs").join("gamemode-executor.log"));
        touch(&kept.join("config.toml"));
        touch(&kept.join("something-else.txt"));
        let plan = Plan {
            stop_watcher: false,
            remove_task: false,
            files: vec![
                local.join("logs").join("gamemode-executor.log"),
                kept.join("config.toml"),
            ],
            dirs: vec![local.join("logs"), local.clone(), kept.clone()],
            program: None,
        };

        execute(&plan).unwrap();

        assert!(!local.exists(), "empty folders go");
        assert!(
            kept.join("something-else.txt").exists(),
            "a folder holding something else stays"
        );
    }

    /// The hand-installed case hands the executables to a shell that waits
    /// for this process to exit. The test cannot exit, so the shell is told
    /// to wait for a process that already has.
    #[test]
    fn an_unpacked_program_is_deleted_by_the_shell_once_the_process_is_gone() {
        let dir = scratch().join("program");
        let exe = dir.join("gamemode-executor.exe");
        let license = dir.join("LICENSE");
        let page = dir.join("docs").join("getting-started.md");
        touch(&exe);
        touch(&license);
        touch(&page);
        let gone = Command::new("cmd.exe")
            .args(["/c", "exit"])
            .spawn()
            .unwrap();
        let pid = gone.id();
        gone.wait_with_output().unwrap();

        after_process(
            pid,
            &[
                format!(
                    "Remove-Item -LiteralPath {} -Force -ErrorAction SilentlyContinue",
                    quoted(&exe)
                ),
                format!(
                    "Remove-Item -LiteralPath {} -Force -ErrorAction SilentlyContinue",
                    quoted(&license)
                ),
                format!(
                    "Remove-Item -LiteralPath {} -Recurse -Force -ErrorAction SilentlyContinue",
                    quoted(&dir.join("docs"))
                ),
                format!(
                    "Remove-Item -LiteralPath {} -ErrorAction SilentlyContinue",
                    quoted(&dir)
                ),
            ],
        )
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(15);
        while dir.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(!exe.exists(), "the executable was deleted");
        assert!(!page.exists(), "the documentation tree was deleted");
        assert!(!dir.exists(), "the folder went once empty");
    }
}
