//! Configuration model, loaded from a TOML file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const APP_DIR_NAME: &str = "GameModeExecutor";

/// Root of the configuration file.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub general: General,
    pub detection: Detection,
    /// What to run when a game is detected.
    pub on_game_start: Event,
    /// What to run once the game is gone.
    pub on_game_stop: Event,
}

/// One set of commands, and how they run relative to each other.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Event {
    pub mode: Mode,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// One after another. An action that is waited for holds up the next.
    #[default]
    Series,
    /// All started at once, then waited for.
    Parallel,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Series => "series",
            Self::Parallel => "parallel",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    /// Run `on_game_stop` when the program itself exits while a game is active.
    pub stop_actions_on_exit: bool,
    /// `error`, `warn`, `info`, `debug` or `trace`.
    pub log_level: String,
    /// Directory holding the log file. Defaults to the roaming profile.
    pub log_dir: Option<PathBuf>,
}

impl Default for General {
    fn default() -> Self {
        Self {
            stop_actions_on_exit: true,
            log_level: "info".to_owned(),
            log_dir: None,
        }
    }
}

/// Detection is Windows' job; these only tune how we watch it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Detection {
    /// How often to look for the presence writer while no game is running.
    /// This is the only polling the program does: once a game starts, the
    /// watcher parks on the writer's process handle until Windows releases it.
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
    /// After the writer exits, how long to wait for it to come back before
    /// declaring the session over. Guards against a game that briefly makes
    /// Windows re-activate it. Zero disables the grace period.
    #[serde(with = "humantime_serde")]
    pub stop_delay: Duration,
    /// How long into a session to wait before asking which of the matched
    /// processes is really the game. Zero skips the question entirely.
    #[serde(with = "humantime_serde")]
    pub identify_after: Duration,
    /// How long to sample the GPU counters for. Utilisation is a rate, so it
    /// needs two readings this far apart.
    #[serde(with = "humantime_serde")]
    pub gpu_sample: Duration,
}

impl Default for Detection {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(2),
            stop_delay: Duration::from_secs(2),
            identify_after: Duration::from_secs(20),
            gpu_sample: Duration::from_secs(1),
        }
    }
}

/// One executable to run when an event fires.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Action {
    pub enabled: bool,
    /// Optional label used in the logs.
    pub name: Option<String>,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    /// Start the process without creating a console window.
    pub no_window: bool,
    /// Wait for the process to exit before running the next action.
    pub wait: bool,
    /// Give up waiting after this delay. Ignored when `wait` is false.
    #[serde(with = "humantime_serde")]
    pub timeout: Option<Duration>,
}

impl Default for Action {
    fn default() -> Self {
        Self {
            enabled: true,
            name: None,
            program: PathBuf::new(),
            args: Vec::new(),
            working_dir: None,
            env: BTreeMap::new(),
            no_window: true,
            wait: false,
            timeout: None,
        }
    }
}

impl Action {
    pub fn label(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.program.display().to_string())
    }
}

/// The configuration file is missing. Carried as error context so the program
/// can exit with a code that says which of the two failures happened.
#[derive(Debug)]
pub struct Missing(pub PathBuf);

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot read config file `{}`", self.0.display())
    }
}

impl std::error::Error for Missing {}

/// The configuration file is present but unusable, whether it failed to parse
/// or failed validation.
#[derive(Debug)]
pub struct Invalid(pub PathBuf);

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "config file `{}` is not usable", self.0.display())
    }
}

impl std::error::Error for Invalid {}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| anyhow::Error::new(error).context(Missing(path.to_path_buf())))?;
        let config: Self = toml::from_str(&text)
            .map_err(|error| anyhow::Error::new(error).context(Invalid(path.to_path_buf())))?;
        config
            .validate()
            .map_err(|error| error.context(Invalid(path.to_path_buf())))?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.detection.poll_interval.is_zero() {
            anyhow::bail!("detection.poll_interval must be greater than zero");
        }
        // No commands at all is a valid configuration -- the one `init`
        // writes. The watcher then detects, names and logs sessions and runs
        // nothing, which is how someone sees it work before deciding what it
        // should run. Decided 2026-09-17, on the first install from the
        // package.
        for action in self
            .on_game_start
            .actions
            .iter()
            .chain(&self.on_game_stop.actions)
        {
            if action.program.as_os_str().is_empty() {
                anyhow::bail!("an action has an empty `program`");
            }
        }
        Ok(())
    }
}

/// Where the configuration file lives when `--config` is not given: next to the
/// executable first (a hand-installed copy), then in the roaming profile.
pub fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        paths.push(dir.join(CONFIG_FILE_NAME));
    }
    if let Some(dir) = roaming_dir() {
        paths.push(dir.join(CONFIG_FILE_NAME));
    }
    paths
}

/// `%APPDATA%\GameModeExecutor`: the roaming profile, for the configuration.
///
/// Roaming is right for it. Windows carries this folder between machines, and
/// the configuration is worth carrying: it names no path of its own -- a
/// program needing elevation is reached through a scheduled task, so the task
/// holds the machine-specific part and the file does not.
pub fn roaming_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join(APP_DIR_NAME))
}

/// `%LOCALAPPDATA%\GameModeExecutor`: the local profile, for the log and the
/// session marker.
///
/// A log describes one machine's sessions, so carrying it to another would be
/// meaningless -- and on a roaming profile it would be copied back and forth at
/// every logon for nothing. The marker is worse than meaningless elsewhere: it
/// would run the stop commands on a machine that never started them. Local is
/// where Windows puts what belongs to the machine rather than the person.
pub fn local_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|local| PathBuf::from(local).join(APP_DIR_NAME))
}

/// The configuration `init` writes: `config.example.toml` at the root of the
/// repository, compiled in, so the file a user starts from and the one the
/// repository documents are the same bytes.
pub const STARTER: &str = include_str!("../config.example.toml");

/// What writing the starter configuration did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Starter {
    /// There was no file; there is one now.
    Written,
    /// A file was there and was left alone.
    Kept,
    /// A file was there and `force` replaced it.
    Overwritten,
}

/// The decision alone, so it can be tested without a disk.
pub fn starter_outcome(exists: bool, force: bool) -> Starter {
    match (exists, force) {
        (false, _) => Starter::Written,
        (true, false) => Starter::Kept,
        (true, true) => Starter::Overwritten,
    }
}

/// Write the starter configuration to `path`, creating its folder. A file
/// already there is kept unless `force`: the installer runs this on every
/// install and upgrade, and a configuration someone edited must survive
/// both. The outcome is logged at `info`, under `setup`.
pub fn write_starter(path: &Path, force: bool) -> Result<Starter> {
    let outcome = starter_outcome(path.exists(), force);
    if outcome != Starter::Kept {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create `{}`", parent.display()))?;
        }
        std::fs::write(path, STARTER)
            .with_context(|| format!("cannot write `{}`", path.display()))?;
    }
    match outcome {
        Starter::Written => tracing::info!(
            target: crate::logging::target::SETUP,
            path = %path.display(),
            "Starter configuration written"
        ),
        Starter::Kept => tracing::info!(
            target: crate::logging::target::SETUP,
            path = %path.display(),
            "Configuration kept: one is already there (--force replaces it)"
        ),
        Starter::Overwritten => tracing::info!(
            target: crate::logging::target::SETUP,
            path = %path.display(),
            "Configuration replaced by the starter one, as asked"
        ),
    }
    Ok(outcome)
}

/// Where `init` writes when not told otherwise: the roaming profile.
pub fn starter_path() -> Result<PathBuf> {
    Ok(roaming_dir()
        .context("cannot determine %APPDATA%")?
        .join(CONFIG_FILE_NAME))
}

/// First existing candidate, or the first candidate at all so error messages
/// point at a sensible location.
pub fn default_path() -> Result<PathBuf> {
    let candidates = candidate_paths();
    anyhow::ensure!(!candidates.is_empty(), "cannot determine a config location");
    Ok(candidates
        .iter()
        .find(|path| path.is_file())
        .unwrap_or(&candidates[0])
        .clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two are easy to mix up, and mixing them up is invisible: the log
    /// would simply be written somewhere it does not belong, and roam.
    #[test]
    fn the_configuration_roams_and_the_log_does_not() {
        let roaming = roaming_dir().expect("APPDATA is set on Windows");
        let local = local_dir().expect("LOCALAPPDATA is set on Windows");
        assert_ne!(roaming, local);
        assert!(roaming.ends_with(APP_DIR_NAME), "{}", roaming.display());
        assert!(local.ends_with(APP_DIR_NAME), "{}", local.display());
    }

    #[test]
    fn example_config_parses_and_validates() {
        let config: Config = toml::from_str(include_str!("../config.example.toml")).unwrap();
        config.validate().unwrap();
    }

    #[test]
    fn empty_config_falls_back_to_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.detection.poll_interval, Duration::from_secs(2));
        assert_eq!(config.detection.stop_delay, Duration::from_secs(2));
        assert_eq!(config.general.log_level, "info");
        // No actions at all is valid: the watcher observes and runs nothing.
        assert!(config.validate().is_ok());
    }

    #[test]
    fn the_starter_is_written_once_and_replaced_only_on_request() {
        assert_eq!(starter_outcome(false, false), Starter::Written);
        assert_eq!(starter_outcome(false, true), Starter::Written);
        assert_eq!(starter_outcome(true, false), Starter::Kept);
        assert_eq!(starter_outcome(true, true), Starter::Overwritten);
    }

    #[test]
    fn write_starter_keeps_what_is_there_unless_forced() {
        let dir =
            std::env::temp_dir().join(format!("gamemode-executor-starter-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("sub").join(CONFIG_FILE_NAME);
        assert_eq!(write_starter(&path, false).unwrap(), Starter::Written);
        std::fs::write(&path, "# edited\n").unwrap();
        assert_eq!(write_starter(&path, false).unwrap(), Starter::Kept);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# edited\n");
        assert_eq!(write_starter(&path, true).unwrap(), Starter::Overwritten);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), STARTER);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_execution_mode_defaults_to_series() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.on_game_start.mode, Mode::Series);
        assert_eq!(config.on_game_stop.mode, Mode::Series);
    }

    #[test]
    fn parallel_is_spelled_the_way_the_template_spells_it() {
        let config: Config = toml::from_str(
            "[on_game_start]
mode = \"parallel\"

[[on_game_start.actions]]
program = \"cmd.exe\"
",
        )
        .unwrap();
        assert_eq!(config.on_game_start.mode, Mode::Parallel);
        assert_eq!(config.on_game_start.actions.len(), 1);
        config.validate().unwrap();
    }

    #[test]
    fn an_unknown_mode_is_rejected() {
        assert!(
            toml::from_str::<Config>(
                "[on_game_start]
mode = \"concurrent\"
"
            )
            .is_err()
        );
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(
            toml::from_str::<Config>(
                "[general]
poll_intervall = \"2s\"
"
            )
            .is_err()
        );
    }
}
