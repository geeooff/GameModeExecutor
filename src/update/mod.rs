//! Updating from the latest GitHub release, on the user's click and at no
//! other time.
//!
//! **The UI reflects an object.** Every rule lives here: which menu entries
//! exist in which phase, which actions are legal, when a verdict expires.
//! The tray asks [`view`] for a list of items and calls [`perform`] for the
//! one chosen; it holds no rule of its own. The object is driven by
//! events, so it is tested whole without a network -- the network is
//! behind [`feed::Feed`], the same seam the engine has for the OS.
//!
//! **The installer is the updater.** An installed copy downloads the next
//! package, verifies it against the release's checksum file and runs
//! `msiexec /qn` detached; the package stops this watcher with a handover
//! and starts the new one, which resumes the game session if there is
//! one. An unpacked copy does the same through a hidden shell that waits
//! for this process to exit, expands the archive over the folder and runs
//! `install-task`. `docs/design/13-updating.md` has the whole shape and
//! the measurements behind it.
//!
//! **Never a silent poll.** `perform(Action::Check)` is the only thing that
//! opens a connection, and only the menu and the `update` command call it.

pub mod feed;
pub mod hash;
mod install;
pub mod winhttp;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;

pub use feed::{Kind, Release, Verdict, Version};

use crate::logging::target;
use crate::win::StopSignal;

/// How long a verdict about *now* -- up to date, or a failure -- stays in
/// the menu. A release found does not expire: it does not un-release.
pub const VERDICT_TTL: Duration = Duration::from_secs(60 * 60);

/// What went wrong, in one line for the menu and a code for the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// WinHTTP could not reach the server: DNS, connection, timeout, TLS.
    NoConnection { code: u32 },
    /// The server answered, with a status that is not the one expected.
    Http { status: u32 },
    /// An answer that is not what a release looks like: a captive portal,
    /// a moved repository, a tag that is not a version.
    Unexpected(String),
    /// The downloaded file does not hash to what the release says.
    Verification,
    /// The download could not be written where it goes.
    Write { path: PathBuf, detail: String },
    /// Windows Installer refused before it stopped the watcher.
    Installer { code: i32 },
    /// The previous update did not take; the text is what the shell wrote.
    Setup(String),
}

impl Fault {
    /// A file that could not be written, said by its folder: the folder is
    /// what the user can do something about.
    fn write(path: &Path, error: &std::io::Error) -> Self {
        Fault::Write {
            path: path.parent().unwrap_or(path).to_path_buf(),
            detail: error.to_string(),
        }
    }

    fn write_dir(dir: &Path, error: &std::io::Error) -> Self {
        Fault::Write {
            path: dir.to_path_buf(),
            detail: error.to_string(),
        }
    }

    /// The disabled menu line, ending in a pointer to the log because the
    /// line is all the menu can carry.
    pub fn menu_line(&self, during: &str) -> String {
        match self {
            Fault::NoConnection { .. } => format!("Could not {during}: no connection (see log)"),
            Fault::Http { status } => {
                format!("Could not {during}: GitHub answered {status} (see log)")
            }
            Fault::Unexpected(_) => format!("Could not {during}: unexpected answer (see log)"),
            Fault::Verification => "Download failed: the file did not verify (see log)".to_owned(),
            Fault::Write { path, .. } => {
                format!(
                    "Download failed: cannot write to {} (see log)",
                    path.display()
                )
            }
            Fault::Installer { code } => {
                format!("Update failed: Windows Installer {code} (see log)")
            }
            Fault::Setup(text) => format!("{text} (see log)"),
        }
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::NoConnection { code } => write!(f, "no connection (WinHTTP error {code})"),
            Fault::Http { status } => write!(f, "GitHub answered {status}"),
            Fault::Unexpected(text) => write!(f, "unexpected answer: {text}"),
            Fault::Verification => write!(f, "the file did not verify"),
            Fault::Write { path, detail } => {
                write!(f, "cannot write to {}: {detail}", path.display())
            }
            Fault::Installer { code } => write!(f, "Windows Installer exited with {code}"),
            Fault::Setup(text) => write!(f, "{text}"),
        }
    }
}

/// What the updater is doing, and what it has to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Checking,
    UpToDate {
        version: Version,
        at: Instant,
    },
    Available {
        release: Release,
    },
    Downloading {
        release: Release,
        size: Option<u64>,
    },
    Installing {
        release: Release,
    },
    /// `at` is `None` for a failure found at start, which stays until the
    /// next check rather than expiring.
    Failed {
        fault: Fault,
        during: &'static str,
        at: Option<Instant>,
    },
}

/// What happened, from the menu or from the worker.
#[derive(Clone, Debug)]
pub enum Event {
    CheckAsked,
    CheckDone(Result<Verdict, Fault>),
    InstallAsked,
    DownloadStarted {
        size: Option<u64>,
    },
    DownloadDone(Result<(), Fault>),
    InstallFailed(Fault),
    /// The previous update did not take; read at start.
    FoundAtStart(Fault),
    /// The previous update took, and this is its version running for the
    /// first time; read at start.
    UpdatedAtStart(Version),
}

/// What the worker has to go and do once an event was applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Check,
    Download(Release),
    Install(Release),
}

/// What a menu entry does when chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Check,
    Install,
    OpenReleasePage(String),
}

/// One menu entry, as the tray draws it: a label, what choosing it means,
/// and whether it can be chosen at all -- a disabled entry is how the menu
/// carries a sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub action: Option<Action>,
    pub enabled: bool,
}

impl Item {
    fn says(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: None,
            enabled: false,
        }
    }

    fn offers(label: impl Into<String>, action: Action) -> Self {
        Self {
            label: label.into(),
            action: Some(action),
            enabled: true,
        }
    }

    fn withheld(label: impl Into<String>, action: Action) -> Self {
        Self {
            label: label.into(),
            action: Some(action),
            enabled: false,
        }
    }
}

const CHECK: &str = "Check for updates";

/// What to tell the user once, when the outcome of something they asked
/// for arrives: a title and a sentence for a notification. The menu closes
/// on a click, as every Windows menu does, so the answer has to reach them
/// somewhere else -- and it stays in the menu too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub text: String,
}

/// The state machine. Pure: it applies events and renders items, and
/// tells the caller what to go and do.
#[derive(Debug)]
pub struct Machine {
    phase: Phase,
    running: Version,
    /// Set by an outcome, taken by whoever shows it.
    notice: Option<Notice>,
}

impl Machine {
    pub fn new(running: Version) -> Self {
        Self {
            phase: Phase::Idle,
            running,
            notice: None,
        }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// The notice the last outcome left, once.
    pub fn take_notice(&mut self) -> Option<Notice> {
        self.notice.take()
    }

    fn say(&mut self, title: impl Into<String>, text: impl Into<String>) {
        self.notice = Some(Notice {
            title: title.into(),
            text: text.into(),
        });
    }

    /// Whether the phase is busy: a check, a download or an install in
    /// flight, during which no new check makes sense.
    fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Checking | Phase::Downloading { .. } | Phase::Installing { .. }
        )
    }

    pub fn apply(&mut self, event: Event, now: Instant) -> Option<Effect> {
        match (event, &self.phase) {
            (Event::CheckAsked, _) if !self.busy() => {
                self.phase = Phase::Checking;
                Some(Effect::Check)
            }
            (Event::CheckDone(result), Phase::Checking) => {
                self.phase = match result {
                    Ok(Verdict::UpToDate) => {
                        self.say(
                            "Up to date",
                            format!("{} is the latest version.", self.running),
                        );
                        Phase::UpToDate {
                            version: self.running,
                            at: now,
                        }
                    }
                    Ok(Verdict::Available(release)) => {
                        self.say(
                            "Update available",
                            format!(
                                "{} is available. Right-click the icon to download and install it.",
                                release.version
                            ),
                        );
                        Phase::Available { release }
                    }
                    Err(fault) => {
                        self.say(
                            "Could not check for updates",
                            format!("{fault}. See the log."),
                        );
                        Phase::Failed {
                            fault,
                            during: "check",
                            at: Some(now),
                        }
                    }
                };
                None
            }
            (Event::InstallAsked, Phase::Available { release }) => {
                let release = release.clone();
                self.phase = Phase::Downloading {
                    release: release.clone(),
                    size: None,
                };
                Some(Effect::Download(release))
            }
            (Event::DownloadStarted { size }, Phase::Downloading { release, .. }) => {
                self.phase = Phase::Downloading {
                    release: release.clone(),
                    size,
                };
                None
            }
            (Event::DownloadDone(Ok(())), Phase::Downloading { release, .. }) => {
                let release = release.clone();
                self.say(
                    format!("Installing {}", release.version),
                    "The icon disappears for a moment and comes back on the new version.",
                );
                self.phase = Phase::Installing {
                    release: release.clone(),
                };
                Some(Effect::Install(release))
            }
            (Event::DownloadDone(Err(fault)), Phase::Downloading { .. }) => {
                self.say("Update failed", format!("{fault}. See the log."));
                self.phase = Phase::Failed {
                    fault,
                    during: "download",
                    at: Some(now),
                };
                None
            }
            (Event::InstallFailed(fault), Phase::Installing { .. }) => {
                self.say("Update failed", format!("{fault}. See the log."));
                self.phase = Phase::Failed {
                    fault,
                    during: "install",
                    at: Some(now),
                };
                None
            }
            (Event::FoundAtStart(fault), Phase::Idle) => {
                self.say("The last update failed", format!("{fault}. See the log."));
                self.phase = Phase::Failed {
                    fault,
                    during: "install",
                    at: None,
                };
                None
            }
            // The install went by in a second; this is the moment the new
            // version can be seen. Nothing to offer, so the phase stays.
            (Event::UpdatedAtStart(version), Phase::Idle) => {
                self.say(
                    format!("Updated to {version}"),
                    "GameModeExecutor is running the new version.",
                );
                None
            }
            // A stale answer, or a click the phase does not take: nothing.
            _ => None,
        }
    }

    /// The menu section, top to bottom. Expiry is decided here, from `now`,
    /// so there is no timer anywhere.
    pub fn view(&self, now: Instant) -> Vec<Item> {
        let expired = |at: Instant| now.duration_since(at) >= VERDICT_TTL;
        match &self.phase {
            Phase::Idle => vec![Item::offers(CHECK, Action::Check)],
            Phase::Checking => vec![Item::says("Checking for updates\u{2026}")],
            Phase::UpToDate { at, .. } if expired(*at) => vec![Item::offers(CHECK, Action::Check)],
            Phase::UpToDate { version, .. } => vec![
                Item::offers(CHECK, Action::Check),
                Item::says(format!("{version} is the latest version")),
            ],
            Phase::Available { release } => vec![
                Item::offers(CHECK, Action::Check),
                Item::offers(
                    format!("Download and install {}", release.version),
                    Action::Install,
                ),
                Item::offers(
                    format!("What changed in {}", release.version),
                    Action::OpenReleasePage(release.page.clone()),
                ),
            ],
            Phase::Downloading { release, size } => vec![
                Item::withheld(CHECK, Action::Check),
                Item::says(match size {
                    Some(size) => format!(
                        "Downloading {} ({})\u{2026}",
                        release.version,
                        megabytes(*size)
                    ),
                    None => format!("Downloading {}\u{2026}", release.version),
                }),
                Item::offers(
                    format!("What changed in {}", release.version),
                    Action::OpenReleasePage(release.page.clone()),
                ),
            ],
            Phase::Installing { release } => vec![
                Item::withheld(CHECK, Action::Check),
                Item::says(format!("Installing {}\u{2026}", release.version)),
            ],
            Phase::Failed { at: Some(at), .. } if expired(*at) => {
                vec![Item::offers(CHECK, Action::Check)]
            }
            Phase::Failed { fault, during, .. } => vec![
                Item::offers(CHECK, Action::Check),
                Item::says(fault.menu_line(during)),
            ],
        }
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

// ------------------------------------------------------------ the process --

/// What the worker needs to know about this copy of the program.
pub struct Context {
    /// `https://github.com/{owner}/{repo}`, from the build.
    pub repository: String,
    /// The kind of copy this is, when pinned -- the tests pin it. `None`
    /// means it is decided when asked, from what Windows Installer says at
    /// that moment, never at start: the package starts the watcher from
    /// `RegisterTask`, which runs *before* `RegisterProduct`, so a watcher
    /// that decided at start saw no product and took itself for an
    /// unpacked copy. Seen on 2026-09-18 17:44 -- it expanded the zip over
    /// the package's folder.
    pub kind: Option<Kind>,
    /// Where downloads go: `%LOCALAPPDATA%\GameModeExecutor\updates`.
    pub updates_dir: PathBuf,
    /// Where the executables live, for the zip path.
    pub install_dir: PathBuf,
    /// The running watcher's stop, when there is one: the zip path stops
    /// this process itself, with a handover, once its shell is started.
    pub stop: Option<Arc<StopSignal>>,
    /// Called after an outcome changed the phase, from the worker thread:
    /// the tray's way to learn there is a notice to show. A callback rather
    /// than a window handle, for the reason the engine reports sessions
    /// through one -- this module has no business knowing what is drawn.
    pub wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Context {
    /// The context of this process. Which kind of copy it is waits for the
    /// question -- see `kind`.
    pub fn of_this_process(
        stop: Option<Arc<StopSignal>>,
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<Self> {
        let exe = std::env::current_exe()?;
        let install_dir = exe
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let updates_dir = crate::config::local_dir()
            .ok_or_else(|| anyhow::anyhow!("no local profile folder"))?
            .join("updates");
        Ok(Self {
            repository: crate::build_info::REPOSITORY.to_owned(),
            kind: None,
            updates_dir,
            install_dir,
            stop,
            wake,
        })
    }

    /// Installed or unpacked, decided now.
    ///
    /// Installed means two things at once: Windows Installer knows the
    /// upgrade code, *and* this executable runs from the folder the package
    /// installs to. A copy unpacked somewhere else on a machine that also
    /// has the package must update its own files, not the package's --
    /// otherwise its `msiexec` would upgrade the other copy and leave
    /// itself as it was.
    pub fn kind(&self) -> Kind {
        self.kind.unwrap_or_else(|| {
            kind_of(
                &self.install_dir,
                crate::package::installed_product().is_some(),
                crate::package::install_dir().as_deref(),
            )
        })
    }
}

/// The decision alone, so it can be tested without a package on the
/// machine. Paths are compared as spelled, case-insensitively, which is
/// what Windows does with them.
fn kind_of(install_dir: &Path, product_installed: bool, package_dir: Option<&Path>) -> Kind {
    let same = |a: &Path, b: &Path| {
        a.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches(['\\', '/']))
    };
    match package_dir {
        Some(package_dir) if product_installed && same(install_dir, package_dir) => Kind::Installer,
        _ => Kind::Zip,
    }
}

struct State {
    machine: Machine,
    context: Arc<Context>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> Option<T> {
    let mut held = STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.as_mut().map(f)
}

/// Start the updater for this process: read what the previous update left
/// behind -- a version that took, or one that did not -- say so, and give
/// the menu its section. Nothing connects until someone clicks.
pub fn start(context: Context) {
    let mut machine = Machine::new(Version::running());
    match install::settle(&context) {
        install::Settled::Nothing => {}
        install::Settled::Updated(version) => {
            machine.apply(Event::UpdatedAtStart(version), Instant::now());
        }
        install::Settled::Failed(fault) => {
            machine.apply(Event::FoundAtStart(fault), Instant::now());
        }
    }
    // An outcome found at start is told the way any other is: the tray
    // reads the notice once its message loop runs.
    let wake = machine
        .notice
        .is_some()
        .then(|| context.wake.clone())
        .flatten();
    let mut held = STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *held = Some(State {
        machine,
        context: Arc::new(context),
    });
    drop(held);
    if let Some(wake) = wake {
        wake();
    }
}

/// The menu section as it should read right now. Empty when the updater
/// was never set up, which is what the tests of everything else see.
pub fn view() -> Vec<Item> {
    with_state(|state| state.machine.view(Instant::now())).unwrap_or_default()
}

/// The notice the last outcome left, once; the tray shows it.
pub fn take_notice() -> Option<Notice> {
    with_state(|state| state.machine.take_notice()).flatten()
}

/// Apply an event, run what it asks for on a worker thread, and wake the
/// tray if the outcome left a notice.
fn apply(event: Event) {
    let (effect, wake) = with_state(|state| {
        let effect = state.machine.apply(event, Instant::now());
        let context = Arc::clone(&state.context);
        let wake = state
            .machine
            .notice
            .is_some()
            .then(|| context.wake.clone())
            .flatten();
        (effect.map(|effect| (effect, context)), wake)
    })
    .unwrap_or((None, None));
    if let Some(wake) = wake {
        wake();
    }
    if let Some((effect, context)) = effect {
        std::thread::spawn(move || run(effect, &context));
    }
}

/// Act on a menu entry. Anything the phase does not take is ignored, which
/// is what a click on a stale menu deserves. `OpenReleasePage` carries its
/// URL for the caller to open -- the shell is the tray's business -- and
/// changes nothing here.
pub fn perform(action: Action) {
    match action {
        Action::Check => apply(Event::CheckAsked),
        Action::Install => apply(Event::InstallAsked),
        Action::OpenReleasePage(_) => {}
    }
}

/// The check, over the real network, said in the log: what the menu's
/// *Check for updates* and the console's `update --check` both run.
pub fn check_now(context: &Context) -> Result<Verdict, Fault> {
    tracing::info!(target: target::UPDATE, "Checking for updates");
    let feed = winhttp::WinHttp::new();
    let kind = context.kind();
    tracing::debug!(target: target::UPDATE, kind = ?kind, "This copy updates as");
    let verdict = feed::check(&feed, &context.repository, Version::running(), kind);
    match &verdict {
        Ok(Verdict::UpToDate) => tracing::info!(
            target: target::UPDATE,
            "{} is the latest version",
            Version::running()
        ),
        Ok(Verdict::Available(release)) => tracing::info!(
            target: target::UPDATE,
            tag = release.tag,
            asset = release.asset,
            sha256 = release.sha256,
            "Update available: {}",
            release.version
        ),
        Err(fault) => tracing::warn!(
            target: target::UPDATE,
            error = %fault,
            "Could not check for updates"
        ),
    }
    verdict
}

/// Fetch, verify and launch `release`, said in the log. What comes back
/// is what the caller has to wait for, if anything.
pub fn install_now(context: &Context, release: &Release) -> Result<Launched, Fault> {
    let feed = winhttp::WinHttp::new();
    download(&feed, context, release).map_err(|fault| {
        tracing::warn!(
            target: target::UPDATE,
            error = %fault,
            "Could not download {}",
            release.version
        );
        fault
    })?;
    let file = context.updates_dir.join(&release.asset);
    install::launch(context, release, &file).map_err(|fault| {
        tracing::warn!(
            target: target::UPDATE,
            error = %fault,
            "The update to {} could not be started",
            release.version
        );
        fault
    })
}

/// What was launched, and so what is left to wait for.
pub use install::Launched;

/// Wait for a launched installer and read its note; `None` is success.
pub fn wait_for(context: &Context, child: std::process::Child) -> Option<Fault> {
    install::wait(context, child)
}

/// The worker: the network, the disk and the launch, never on the window's
/// thread.
fn run(effect: Effect, context: &Context) {
    let feed = winhttp::WinHttp::new();
    match effect {
        Effect::Check => apply(Event::CheckDone(check_now(context))),
        Effect::Download(release) => {
            let outcome = download(&feed, context, &release);
            if let Err(fault) = &outcome {
                tracing::warn!(
                    target: target::UPDATE,
                    error = %fault,
                    "Could not download {}",
                    release.version
                );
            }
            apply(Event::DownloadDone(outcome));
        }
        Effect::Install(release) => {
            let file = context.updates_dir.join(&release.asset);
            match install::launch(context, &release, &file) {
                Ok(install::Launched::Installer(child)) => {
                    // The package stops this process on its way. Still
                    // being here when the shell returns means the install
                    // failed -- before the stop, with the shell's note
                    // saying how, or in a way that left this watcher
                    // running, which is a failure of its own.
                    let fault = install::wait(context, child).unwrap_or_else(|| {
                        Fault::Setup("the installer ended without replacing the program".to_owned())
                    });
                    tracing::warn!(
                        target: target::UPDATE,
                        error = %fault,
                        "The update to {} failed",
                        release.version
                    );
                    apply(Event::InstallFailed(fault));
                }
                Ok(install::Launched::Shell) => {
                    // The shell waits for this process; leave, handing the
                    // session over.
                    if let Some(stop) = &context.stop {
                        stop.signal_handover();
                    }
                }
                Err(fault) => {
                    tracing::warn!(
                        target: target::UPDATE,
                        error = %fault,
                        "The update to {} could not be started",
                        release.version
                    );
                    apply(Event::InstallFailed(fault));
                }
            }
        }
    }
}

/// Fetch the asset by tag and verify it against the release's hash. A file
/// that does not verify is deleted before anything can run it.
fn download(feed: &dyn feed::Feed, context: &Context, release: &Release) -> Result<(), Fault> {
    std::fs::create_dir_all(&context.updates_dir)
        .map_err(|error| Fault::write_dir(&context.updates_dir, &error))?;
    let file = context.updates_dir.join(&release.asset);
    let url = release.download_url(&context.repository);
    let mut announced = None;
    let written = feed.download(&url, &file, &mut |size| {
        announced = size;
        if let Some(size) = size {
            tracing::info!(
                target: target::UPDATE,
                url,
                "Downloading {} ({})",
                release.version,
                megabytes(size)
            );
        } else {
            tracing::info!(target: target::UPDATE, url, "Downloading {}", release.version);
        }
        apply(Event::DownloadStarted { size });
    })?;
    let actual = hash::sha256_of(&file).map_err(|error| Fault::Unexpected(error.to_string()))?;
    if actual != release.sha256 {
        tracing::warn!(
            target: target::UPDATE,
            expected = release.sha256,
            actual,
            bytes = written,
            "The downloaded file does not hash to what the release says; deleted"
        );
        let _ = std::fs::remove_file(&file);
        return Err(Fault::Verification);
    }
    tracing::info!(
        target: target::UPDATE,
        path = %file.display(),
        bytes = written,
        "Downloaded and verified {}",
        release.version
    );
    Ok(())
}

#[cfg(test)]
mod tests;
