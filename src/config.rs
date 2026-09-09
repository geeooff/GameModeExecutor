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
    /// Actions run once, when a game has been detected long enough.
    pub on_game_start: Vec<Action>,
    /// Actions run once, when no game has been detected for long enough.
    pub on_game_stop: Vec<Action>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    /// How often the detectors are polled.
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
    /// A game must be detected for this long before `on_game_start` runs.
    #[serde(with = "humantime_serde")]
    pub start_delay: Duration,
    /// No game must be detected for this long before `on_game_stop` runs.
    #[serde(with = "humantime_serde")]
    pub stop_delay: Duration,
    /// Run `on_game_stop` when the program itself exits while a game is active.
    pub stop_actions_on_exit: bool,
    /// `error`, `warn`, `info`, `debug` or `trace`.
    pub log_level: String,
    /// Directory for rolling daily log files. Disabled when empty.
    pub log_dir: Option<PathBuf>,
    /// Number of daily log files kept.
    pub log_keep_days: usize,
}

impl Default for General {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(2),
            start_delay: Duration::from_secs(5),
            stop_delay: Duration::from_secs(15),
            stop_actions_on_exit: true,
            log_level: "info".to_owned(),
            log_dir: None,
            log_keep_days: 7,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    /// A game is running as soon as one enabled detector says so.
    Any,
    /// A game is running only when every enabled detector says so.
    All,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Detection {
    pub match_mode: MatchMode,
    /// Executable names to watch, with or without the `.exe` suffix.
    /// The detector is disabled when the list is empty.
    pub processes: Vec<String>,
    /// Executable names that never count as a game, whatever the detector.
    pub ignore_processes: Vec<String>,
    pub fullscreen: Fullscreen,
}

impl Default for Detection {
    fn default() -> Self {
        Self {
            match_mode: MatchMode::Any,
            processes: Vec::new(),
            ignore_processes: Vec::new(),
            fullscreen: Fullscreen::default(),
        }
    }
}

/// Detection based on the shell notification state, which reports whether a
/// full-screen application owns the desktop.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Fullscreen {
    pub enabled: bool,
    /// Which shell states count as "a game is running".
    pub states: Vec<FullscreenState>,
}

impl Default for Fullscreen {
    fn default() -> Self {
        Self {
            enabled: false,
            states: vec![FullscreenState::D3dExclusive, FullscreenState::Busy],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FullscreenState {
    /// `QUNS_RUNNING_D3D_FULL_SCREEN`: exclusive full-screen Direct3D.
    D3dExclusive,
    /// `QUNS_BUSY`: a full-screen (typically borderless) application is running.
    Busy,
    /// `QUNS_APP`: a Store app is running full-screen.
    StoreApp,
    /// `QUNS_PRESENTATION_MODE`: presentation settings are applied.
    Presentation,
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

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read config file `{}`", path.display()))?;
        let config: Self = toml::from_str(&text)
            .with_context(|| format!("cannot parse config file `{}`", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.general.poll_interval.is_zero() {
            anyhow::bail!("general.poll_interval must be greater than zero");
        }
        if self.detection.processes.is_empty() && !self.detection.fullscreen.enabled {
            anyhow::bail!(
                "no detector is enabled: set `detection.processes` and/or `detection.fullscreen.enabled`"
            );
        }
        for action in self.on_game_start.iter().chain(&self.on_game_stop) {
            if action.program.as_os_str().is_empty() {
                anyhow::bail!("an action has an empty `program`");
            }
        }
        Ok(())
    }
}

/// Where the configuration file lives when `--config` is not given: next to the
/// executable first (portable install), then in the roaming profile.
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

pub fn roaming_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join(APP_DIR_NAME))
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

    #[test]
    fn example_config_parses_and_validates() {
        let config: Config = toml::from_str(include_str!("../config.example.toml")).unwrap();
        config.validate().unwrap();
    }

    #[test]
    fn empty_config_falls_back_to_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.general.poll_interval, Duration::from_secs(2));
        assert_eq!(config.general.log_level, "info");
        // No detector is enabled by default, which validation rejects.
        assert!(config.validate().is_err());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(toml::from_str::<Config>("[general]
poll_intervall = \"2s\"
").is_err());
    }
}
