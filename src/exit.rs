//! Exit codes.
//!
//! Everything used to leave with `1`, whatever went wrong, which tells a script
//! nothing. `clap` already owns `2` for command-line misuse, so that value is
//! spoken for.

use crate::config;
use crate::win;

pub const SUCCESS: u8 = 0;
pub const FAILURE: u8 = 1;
// 2 is clap's, for command-line misuse; it never comes from here.
pub const CONFIG_MISSING: u8 = 3;
pub const CONFIG_INVALID: u8 = 4;
pub const ALREADY_RUNNING: u8 = 5;

/// Pick the code that describes a failure, by looking for the markers the
/// relevant errors carry as context.
pub fn code_for(error: &anyhow::Error) -> u8 {
    if error.downcast_ref::<config::Missing>().is_some() {
        CONFIG_MISSING
    } else if error.downcast_ref::<config::Invalid>().is_some() {
        CONFIG_INVALID
    } else if error.downcast_ref::<win::AlreadyRunning>().is_some() {
        ALREADY_RUNNING
    } else {
        FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_missing_file_is_told_apart_from_an_unusable_one() {
        let missing = anyhow::Error::new(std::io::Error::other("no such file"))
            .context(config::Missing(PathBuf::from("a.toml")));
        let invalid =
            anyhow::anyhow!("bad value").context(config::Invalid(PathBuf::from("a.toml")));
        assert_eq!(code_for(&missing), CONFIG_MISSING);
        assert_eq!(code_for(&invalid), CONFIG_INVALID);
    }

    #[test]
    fn a_second_instance_has_its_own_code() {
        let error = anyhow::anyhow!("mutex held").context(win::AlreadyRunning);
        assert_eq!(code_for(&error), ALREADY_RUNNING);
    }

    #[test]
    fn anything_else_is_a_plain_failure() {
        assert_eq!(code_for(&anyhow::anyhow!("something else")), FAILURE);
    }

    #[test]
    fn clap_keeps_its_own_code() {
        // 2 is what clap returns for command-line misuse; none of ours may
        // collide with it, or a script could not tell the two apart.
        let ours = [
            SUCCESS,
            FAILURE,
            CONFIG_MISSING,
            CONFIG_INVALID,
            ALREADY_RUNNING,
        ];
        assert!(!ours.contains(&2));
    }
}
