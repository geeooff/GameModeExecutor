//! `purge`: remove every trace of the program, on request, whichever way it
//! was installed.
//!
//! The rule, from `docs/design/08-distribution.md`: it removes what it
//! recognises as its own and leaves the rest, saying what it left. Its own
//! is the logon task, the configuration wherever it was found, the log
//! wherever it was written, the session marker, the updater's folder and
//! what it holds, the two profile folders once they are empty, and last
//! the executables -- through Windows
//! Installer when the installer owns them, through a detached shell that
//! waits for this process to exit when they were unpacked by hand. A task
//! it did not register, a folder holding anything else, a file it does not
//! know: left alone.
//!
//! The uninstall writes to the log once more: Windows Installer runs `stop`
//! and `uninstall-task`, and each says what it did, in the default log
//! folder since the configuration is gone by then. So the shell that runs
//! the uninstall waits for it and sweeps that folder afterwards -- decided
//! 2026-09-25 over a flag telling those commands not to log, which would
//! have changed the package and two setup commands for two lines.
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

use anyhow::{Context, Result};

use crate::config;
use crate::logging;
use crate::marker;
use crate::package;
use crate::service;
use crate::shell::{after_exit, quoted};
use crate::task;
use crate::win;

/// What the executables' removal has to go through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    /// Windows Installer registered this product; `msiexec /x` removes it,
    /// its registration and its folder. `default_log_dir` is where the
    /// uninstall's own commands log, swept once it is over.
    Installed {
        product_code: String,
        default_log_dir: Option<PathBuf>,
    },
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
    /// The log's folder: every log file in it goes, today's and the days
    /// kept, and the single file of earlier versions.
    pub log_dir: Option<PathBuf>,
    /// Where the log goes without a configuration, which is where an
    /// uninstall's commands write theirs.
    pub default_log_dir: Option<PathBuf>,
    /// The updater's folder, ours whole: an update leaves it empty but
    /// there.
    pub updates_dir: Option<PathBuf>,
    pub marker: Option<PathBuf>,
    pub fault_marker: Option<PathBuf>,
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
    /// Folders that are the program's own through and through, removed
    /// with whatever they hold.
    pub trees: Vec<PathBuf>,
    /// Removed only if empty once the files are gone, deepest first.
    pub dirs: Vec<PathBuf>,
    pub program: Option<Program>,
}

/// The files a hand-installed copy is made of, beyond the executables:
/// what the zip unpacks next to them.
/// `LICENSE` without an extension is what zips before 2026-09-18 carried.
const BUNDLE_FILES: [&str; 3] = ["LICENSE", "LICENSE.txt", "README.txt"];
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
        if let Some(dir) = &layout.log_dir {
            let mut logs = logging::log_files(dir);
            logs.sort();
            for log in &logs {
                push(log);
            }
        }
        if let Some(marker) = &layout.marker {
            push(marker);
        }
        if let Some(marker) = &layout.fault_marker {
            push(marker);
        }

        let trees: Vec<PathBuf> = layout
            .updates_dir
            .iter()
            .filter(|dir| dir.is_dir())
            .cloned()
            .collect();

        // Deepest first, so a parent is judged after its children are gone.
        // The log's folder counts as ours only inside the program's own
        // places; a log sent elsewhere leaves its folder behind.
        let mut dirs = Vec::new();
        let ours = [&layout.local_dir, &layout.exe_dir];
        if let Some(logs) = &layout.log_dir
            && ours
                .into_iter()
                .flatten()
                .any(|place| logs.starts_with(place) && logs != place)
            && logs.is_dir()
        {
            dirs.push(logs.clone());
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
                default_log_dir: layout.default_log_dir.clone(),
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
            trees,
            dirs,
            program,
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.stop_watcher
            && !self.remove_task
            && self.files.is_empty()
            && self.trees.is_empty()
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
        for tree in &self.trees {
            lines.push(format!("delete {} and what it holds", tree.display()));
        }
        for dir in &self.dirs {
            lines.push(format!("remove {} if it is then empty", dir.display()));
        }
        match &self.program {
            Some(Program::Installed { product_code, .. }) => lines.push(format!(
                "uninstall the program through Windows Installer (product {product_code}), \
                 then delete the log it writes on the way"
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
    let log_dir = config.map_or_else(config::default_log_dir, |config| config.general.log_dir());
    Layout {
        watcher_running: win::SingleInstance::is_held(service::INSTANCE),
        task_registered: task::exists(),
        config_candidates: candidates,
        log_dir,
        default_log_dir: config::default_log_dir(),
        updates_dir: crate::update::updates_dir(),
        marker: local_dir.as_ref().map(|dir| dir.join(marker::FILE_NAME)),
        fault_marker: local_dir
            .as_ref()
            .map(|dir| dir.join(marker::FAULT_FILE_NAME)),
        local_dir,
        roaming_dir: config::roaming_dir(),
        exe_dir: std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf)),
        product_code: package::installed_product(),
    }
}

/// A shell step removing `dir` if it holds nothing, and leaving it alone
/// otherwise. Asked outright: `Remove-Item` without `-Recurse` on a folder
/// that is not empty wants to prompt, and a non-interactive shell turns
/// that into an error and an exit code of 1 -- the folder stays, but by
/// accident, measured on 2026-09-25.
fn remove_if_empty(dir: &Path) -> String {
    let dir = quoted(dir);
    format!(
        "if (-not (Get-ChildItem -LiteralPath {dir} -Force -ErrorAction SilentlyContinue)) \
         {{ Remove-Item -LiteralPath {dir} -ErrorAction SilentlyContinue }}"
    )
}

/// The shell's steps for an installed copy: the uninstall, waited for, then
/// the log its `stop` and `uninstall-task` wrote and the folders that leaves
/// empty.
fn uninstall_steps(product_code: &str, default_log_dir: Option<&Path>) -> Vec<String> {
    let mut steps = vec![format!(
        "Start-Process msiexec.exe -ArgumentList '/x {product_code} /passive' -Wait"
    )];
    if let Some(logs) = default_log_dir {
        steps.push(format!(
            "Get-ChildItem -LiteralPath {} -File -Filter '{}' -ErrorAction SilentlyContinue \
             | Remove-Item -Force -ErrorAction SilentlyContinue",
            quoted(logs),
            logging::log_file_pattern()
        ));
        for dir in [Some(logs), logs.parent()].into_iter().flatten() {
            steps.push(remove_if_empty(dir));
        }
    }
    steps
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
    for tree in &plan.trees {
        std::fs::remove_dir_all(tree)
            .with_context(|| format!("cannot delete {}", tree.display()))?;
        println!("Deleted {}.", tree.display());
    }
    for dir in &plan.dirs {
        match std::fs::remove_dir(dir) {
            Ok(()) => println!("Removed {}.", dir.display()),
            // Not empty, or already gone: either way it is not ours to force.
            Err(_) => println!("Left {} alone: it holds something else.", dir.display()),
        }
    }
    match &plan.program {
        Some(Program::Installed {
            product_code,
            default_log_dir,
        }) => {
            // Once this process is gone: it lives in the folder the installer
            // is about to empty, and Windows Installer would find it in use.
            after_exit(&uninstall_steps(product_code, default_log_dir.as_deref()))?;
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
            // The folder itself: only if that left it empty.
            steps.push(remove_if_empty(dir));
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use std::process::Command;

    use super::*;
    use crate::shell::after_process;

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
        touch(&local.join("logs").join("gamemode-executor.2026-09-25.log"));
        touch(&local.join("logs").join("notes.txt"));
        touch(&local.join("updates").join("GameModeExecutor-0.4.0.msi"));
        touch(&exe_dir.join("gamemode-executor.exe"));
        touch(&exe_dir.join("LICENSE"));
        let layout = Layout {
            watcher_running: false,
            task_registered: true,
            config_candidates: vec![exe_dir.join("config.toml"), roaming.join("config.toml")],
            log_dir: Some(local.join("logs")),
            default_log_dir: Some(local.join("logs")),
            updates_dir: Some(local.join("updates")),
            marker: Some(local.join(marker::FILE_NAME)),
            fault_marker: Some(local.join(marker::FAULT_FILE_NAME)),
            local_dir: Some(local.clone()),
            roaming_dir: Some(roaming.clone()),
            exe_dir: Some(exe_dir.clone()),
            product_code: None,
        };

        let plan = Plan::compute(&layout);

        assert!(!plan.stop_watcher);
        assert!(plan.remove_task);
        // The config next to the executable and the marker do not exist;
        // every log file goes, dated or from before the rotation, and a
        // file of someone else's in the folder stays.
        assert_eq!(
            plan.files,
            vec![
                roaming.join("config.toml"),
                local.join("logs").join("gamemode-executor.2026-09-25.log"),
                local.join("logs").join("gamemode-executor.log"),
            ]
        );
        assert_eq!(
            plan.trees,
            vec![local.join("updates")],
            "the updater's folder, taken whole"
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
            default_log_dir: Some(PathBuf::from(r"C:\local\GameModeExecutor\logs")),
            ..Layout::default()
        };
        let plan = Plan::compute(&layout);
        assert_eq!(
            plan.program,
            Some(Program::Installed {
                product_code: "{00000000-0000-0000-0000-000000000000}".to_owned(),
                default_log_dir: Some(PathBuf::from(r"C:\local\GameModeExecutor\logs")),
            })
        );
        assert!(plan.files.is_empty() && plan.dirs.is_empty());
    }

    /// The uninstall's `stop` and `uninstall-task` log what they did, after
    /// the purge has deleted the log: the shell waits for the installer,
    /// then deletes the log files alone, then the two folders only if that
    /// left them empty.
    #[test]
    fn an_uninstall_is_waited_for_and_the_log_it_writes_swept() {
        let logs = Path::new(r"C:\local\GameModeExecutor\logs");
        let steps = uninstall_steps("{CODE}", Some(logs));
        assert_eq!(
            steps[0],
            "Start-Process msiexec.exe -ArgumentList '/x {CODE} /passive' -Wait"
        );
        assert!(
            steps[1].starts_with(r"Get-ChildItem -LiteralPath 'C:\local\GameModeExecutor\logs' -File -Filter 'gamemode-executor*log'"),
            "{}",
            steps[1]
        );
        assert_eq!(
            steps[2..],
            [
                remove_if_empty(logs),
                remove_if_empty(Path::new(r"C:\local\GameModeExecutor")),
            ]
        );
        assert!(
            steps.iter().all(|step| !step.contains("-Recurse")),
            "{steps:?}"
        );
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
            log_dir: Some(elsewhere.clone()),
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
    fn executing_an_unpacked_plan_removes_files_and_empty_folders() {
        let root = scratch();
        let local = root.join("local");
        let kept = root.join("kept");
        touch(&local.join("logs").join("gamemode-executor.log"));
        touch(&local.join("updates").join("unpacked").join("README.txt"));
        touch(&kept.join("config.toml"));
        touch(&kept.join("something-else.txt"));
        let plan = Plan {
            stop_watcher: false,
            remove_task: false,
            files: vec![
                local.join("logs").join("gamemode-executor.log"),
                kept.join("config.toml"),
            ],
            trees: vec![local.join("updates")],
            dirs: vec![local.join("logs"), local.clone(), kept.clone()],
            program: None,
        };

        execute(&plan).unwrap();

        assert!(
            !local.exists(),
            "the updater's folder goes whole, then the empty folders"
        );
        assert!(
            kept.join("something-else.txt").exists(),
            "a folder holding something else stays"
        );
    }

    #[test]
    fn the_sweep_after_an_uninstall_takes_the_log_and_only_empty_folders() {
        // The steps after the uninstall, run by the real hidden shell: in
        // one profile the log is all there is, in the other something else
        // shares both folders and stays, with them.
        let alone = scratch().join("GameModeExecutor");
        let shared = scratch().join("GameModeExecutor");
        for local in [&alone, &shared] {
            touch(&local.join("logs").join("gamemode-executor.2026-09-26.log"));
            touch(&local.join("logs").join("gamemode-executor.log"));
        }
        touch(&shared.join("logs").join("notes.txt"));
        touch(&shared.join("updates").join("pending.txt"));

        for local in [&alone, &shared] {
            let steps = uninstall_steps("{CODE}", Some(&local.join("logs")));
            let status = crate::shell::hidden(&steps[1..].join("; "))
                .unwrap()
                .wait()
                .unwrap();
            assert!(status.success(), "{status:?}");
        }

        assert!(!alone.exists(), "the log was all there was");
        assert!(logging::log_files(&shared.join("logs")).is_empty());
        assert!(shared.join("logs").join("notes.txt").exists());
        assert!(shared.join("updates").join("pending.txt").exists());
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
