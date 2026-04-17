use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallSource {
    Flatpak,
    AppImage,
    HomebrewCask,
    HomebrewFormula,
    Msi,
    Winget,
    DmgDirect,
    CargoInstall,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateHint {
    OpenUrl(String),
    ShowCommand(String),
    OpenReleasesPage,
}

const RELEASES_URL: &str = "https://github.com/marko-koljancic/solarxy/releases";
const FLATHUB_URL: &str = "https://flathub.org/apps/dev.koljam.solarxy";

static CACHED: OnceLock<InstallSource> = OnceLock::new();

pub fn detect() -> InstallSource {
    *CACHED.get_or_init(detect_uncached)
}

fn detect_uncached() -> InstallSource {
    if std::env::var_os("FLATPAK_ID").is_some() {
        return InstallSource::Flatpak;
    }
    if std::env::var_os("APPIMAGE").is_some() {
        return InstallSource::AppImage;
    }

    if let Some(marker) = read_marker_file() {
        match marker.as_str() {
            "msi" => return InstallSource::Msi,
            "winget" => return InstallSource::Winget,
            "homebrew-cask" => return InstallSource::HomebrewCask,
            "homebrew-formula" => return InstallSource::HomebrewFormula,
            _ => {}
        }
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(src) = classify_exe_path(&exe)
    {
        return src;
    }

    InstallSource::Unknown
}

fn read_marker_file() -> Option<String> {
    let path = marker_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    Some(raw.trim().to_lowercase())
}

fn marker_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let program_data = std::env::var_os("ProgramData")?;
        // GUI MSI writes %ProgramData%\Solarxy\install-source; CLI MSI
        // writes %ProgramData%\Solarxy-cli\install-source. Pick the
        // folder matching whichever binary is currently running so each
        // binary reads its own channel marker.
        let is_cli = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .is_some_and(|n| n.eq_ignore_ascii_case("solarxy-cli"));
        let folder = if is_cli { "Solarxy-cli" } else { "Solarxy" };
        Some(
            PathBuf::from(program_data)
                .join(folder)
                .join("install-source"),
        )
    }
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir()?;
        Some(home.join("Library/Application Support/Solarxy/install-source"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let data = dirs::data_dir()?;
        Some(data.join("solarxy/install-source"))
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        None
    }
}

fn classify_exe_path(exe: &Path) -> Option<InstallSource> {
    let s = exe.to_string_lossy();

    if cfg!(target_os = "macos") && s.starts_with("/Applications/Solarxy.app/") {
        return Some(InstallSource::DmgDirect);
    }

    if s.starts_with("/opt/homebrew/")
        || s.starts_with("/usr/local/Cellar/")
        || s.starts_with("/home/linuxbrew/.linuxbrew/")
    {
        return Some(InstallSource::HomebrewFormula);
    }

    if cfg!(target_os = "windows")
        && (s.contains("\\Program Files\\Solarxy\\")
            || s.contains("\\Program Files\\solarxy-cli\\"))
    {
        return Some(InstallSource::Msi);
    }

    if let Some(cargo_home) = std::env::var_os("CARGO_HOME") {
        let cargo_bin = PathBuf::from(cargo_home).join("bin");
        if exe.starts_with(&cargo_bin) {
            return Some(InstallSource::CargoInstall);
        }
    }
    if let Some(home) = dirs::home_dir() {
        let default_cargo_bin = home.join(".cargo/bin");
        if exe.starts_with(&default_cargo_bin) {
            return Some(InstallSource::CargoInstall);
        }
    }

    None
}

pub fn update_hint(src: InstallSource) -> UpdateHint {
    match src {
        InstallSource::Flatpak => UpdateHint::OpenUrl(FLATHUB_URL.to_string()),
        InstallSource::AppImage | InstallSource::DmgDirect | InstallSource::Unknown => {
            UpdateHint::OpenReleasesPage
        }
        InstallSource::HomebrewCask => {
            UpdateHint::ShowCommand("brew upgrade --cask koljam/solarxy/solarxy".to_string())
        }
        InstallSource::HomebrewFormula => {
            UpdateHint::ShowCommand("brew upgrade solarxy-cli".to_string())
        }
        InstallSource::Msi | InstallSource::Winget => {
            UpdateHint::ShowCommand("winget upgrade Koljam.Solarxy".to_string())
        }
        InstallSource::CargoInstall => {
            UpdateHint::ShowCommand("cargo install solarxy-cli --force".to_string())
        }
    }
}

pub fn releases_url() -> &'static str {
    RELEASES_URL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_falls_back_to_releases_page() {
        assert_eq!(
            update_hint(InstallSource::Unknown),
            UpdateHint::OpenReleasesPage
        );
    }

    #[test]
    fn flatpak_opens_flathub() {
        assert_eq!(
            update_hint(InstallSource::Flatpak),
            UpdateHint::OpenUrl(FLATHUB_URL.into())
        );
    }

    #[test]
    fn msi_shows_winget_command() {
        match update_hint(InstallSource::Msi) {
            UpdateHint::ShowCommand(c) => assert!(c.contains("winget upgrade")),
            other => panic!("expected ShowCommand, got {other:?}"),
        }
    }

    #[test]
    fn cask_shows_brew_cask_command() {
        match update_hint(InstallSource::HomebrewCask) {
            UpdateHint::ShowCommand(c) => assert!(c.contains("--cask")),
            other => panic!("expected ShowCommand, got {other:?}"),
        }
    }
}
