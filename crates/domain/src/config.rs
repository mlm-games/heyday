use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub enable_repo: bool,
    pub enable_aur: bool,
    pub enable_flatpak: bool,
    pub enable_appimage: bool,
    pub upgrade_repo: bool,
    pub upgrade_aur: bool,
    pub upgrade_flatpak: bool,
    pub upgrade_appimage: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enable_repo: true,
            enable_aur: true,
            enable_flatpak: true,
            enable_appimage: true,
            upgrade_repo: true,
            upgrade_aur: false,
            upgrade_flatpak: true,
            upgrade_appimage: true,
        }
    }
}

impl Settings {
    fn path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(".config/soredowe/settings.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, &s);
        }
    }
}
