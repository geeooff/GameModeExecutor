//! What Windows Installer knows about this program.
//!
//! The package `scripts/msi.ps1` builds carries one upgrade code for the
//! life of the product; every version is a new product code under it.
//! Asking Windows Installer for that upgrade code says whether the program
//! was installed from the package and which product code to remove. Both
//! `purge` and `update` need the answer, and neither is the natural home
//! of a Windows Installer query, so it lives here.

use std::path::PathBuf;

/// The same value `scripts/msi.ps1` writes into every package. Fixed for the
/// life of the product; a test checks the two copies agree.
pub const UPGRADE_CODE: &str = "{8C4E0B2D-3F6A-4E7B-9A1C-5D2E8F7B6A30}";

/// Where the package installs, `%LOCALAPPDATA%\Programs\GameModeExecutor`,
/// which `scripts/msi.ps1` fixes through `ProgramFilesFolder`.
pub fn install_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|local| {
        PathBuf::from(local)
            .join("Programs")
            .join("GameModeExecutor")
    })
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

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
    fn the_install_folder_is_the_one_the_package_builder_names() {
        let script = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts")
                .join("msi.ps1"),
        )
        .expect("scripts/msi.ps1 is in the repository");
        // ProgramFilesFolder redirects to %LOCALAPPDATA%\Programs for a
        // per-user package, and the directory row names the last segment.
        assert!(script.contains("'ProgramFilesFolder', 'TARGETDIR'"));
        assert!(
            script.contains("'INSTALLDIR', 'ProgramFilesFolder', 'GAMEMO~1|GameModeExecutor'"),
            "the folder is named after the product"
        );
        let dir = install_dir().expect("LOCALAPPDATA is set on Windows");
        assert!(
            dir.ends_with(r"Programs\GameModeExecutor"),
            "{}",
            dir.display()
        );
    }
}
