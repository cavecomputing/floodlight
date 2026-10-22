//! settings.toml: hand-editable, in the OS's config folder. Unknown or missing keys fall back to
//! defaults instead of failing, and every change is saved at once.

use crate::brand;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub fn other(self) -> Theme {
        match self {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
}

/// `~/.config/<app>/settings.toml` on Linux, `~/Library/Application Support/<app>/` on macOS,
/// `%APPDATA%\<app>\config\` on Windows.
pub fn path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", brand::CONFIG_DIR)
        .map(|dirs| dirs.config_dir().join("settings.toml"))
}

pub fn load() -> Settings {
    path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

/// Writes to a temp file beside the target, then renames it over, so a crash mid-write never
/// leaves half a file.
pub fn save(settings: &Settings) -> io::Result<()> {
    let path = path().ok_or_else(|| io::Error::other("no config folder on this OS"))?;
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::other("settings path has no folder"))?;
    fs::create_dir_all(folder)?;
    let text = toml::to_string(settings).map_err(io::Error::other)?;
    let temp = path.with_extension("toml.tmp");
    fs::write(&temp, text)?;
    fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_and_missing_keys_fall_back() {
        let settings: Settings = toml::from_str("colour = \"mauve\"\n").unwrap();
        assert_eq!(settings.theme, Theme::Dark);
        let settings: Settings = toml::from_str("theme = \"light\"\n").unwrap();
        assert_eq!(settings.theme, Theme::Light);
    }

    #[test]
    fn writes_the_theme_in_lowercase() {
        assert_eq!(
            toml::to_string(&Settings {
                theme: Theme::Light
            })
            .unwrap(),
            "theme = \"light\"\n"
        );
    }
}
