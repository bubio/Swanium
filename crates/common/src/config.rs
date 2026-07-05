//! Frontend configuration.
//!
//! A plain settings record shared across the frontend, with TOML persistence.
//! [`Config::load`] reads the user's config file (creating nothing if it is
//! missing — first run falls back to typed defaults), and [`Config::save`]
//! writes it back to the platform config directory. The path-taking
//! [`Config::load_from`] / [`Config::save_to`] variants are the testable core
//! and let callers point at an arbitrary file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::SwaniumError;

/// Integer window scale applied to the 224×144 framebuffer.
pub const DEFAULT_SCALE: u32 = 3;

/// Default master volume (0–100).
pub const DEFAULT_VOLUME: u8 = 100;

/// Maximum number of entries kept in the recent-ROM history.
pub const RECENT_LIMIT: usize = 10;

/// Application name used to namespace the on-disk config directory.
const APP_NAME: &str = "swanium";

/// File name of the config file inside the platform config directory.
const CONFIG_FILE: &str = "config.toml";

fn default_scale() -> u32 {
    DEFAULT_SCALE
}

fn default_volume() -> u8 {
    DEFAULT_VOLUME
}

/// Texture-sampling filter used when scaling the framebuffer to the window.
///
/// The frontend maps these onto Slint's `image-rendering` (`Nearest` →
/// `pixelated`, `Linear` → `smooth`); those are the only filters Slint's
/// software / femtovg image path exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RendererKind {
    /// Nearest-neighbour sampling — crisp, blocky pixels (the default).
    #[default]
    Nearest,
    /// Bilinear sampling — smoothed, blurrier upscaling.
    Linear,
}

/// Screen rotation for vertical-orientation games.
///
/// `Right`/`Left` rotate the 224×144 output 90° clockwise / counter-clockwise
/// into a 144×224 portrait image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RotationKind {
    /// No rotation — landscape (the default).
    #[default]
    None,
    /// 90° clockwise.
    Right,
    /// 90° counter-clockwise.
    Left,
}

impl RotationKind {
    /// Whether the output is rotated into portrait (width/height swapped).
    pub fn is_rotated(self) -> bool {
        self != RotationKind::None
    }
}

/// User-facing frontend settings.
///
/// Each field carries a serde default so that a partial or older config file
/// loads cleanly, filling any missing keys from the typed defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Integer scale factor for the output window.
    pub scale: u32,
    /// Master volume, 0 (mute) to 100 (full).
    pub volume: u8,
    /// Whether emulation starts paused.
    pub start_paused: bool,
    /// Whether the window opens in fullscreen.
    pub fullscreen: bool,
    /// Screen rotation for vertical-orientation games.
    pub rotation: RotationKind,
    /// Texture-sampling filter used when upscaling the framebuffer.
    pub renderer: RendererKind,
    /// Recently opened ROM paths, most-recent first (capped at [`RECENT_LIMIT`]).
    pub recent_files: Vec<String>,
    /// Keyboard bindings as `WS-button-name → key text` (empty = built-in default).
    ///
    /// Kept as plain strings because `common` cannot depend on the `input`
    /// crate (that would form a `common → input → core → common` cycle); the
    /// frontend resolves these names against its `Button` enum.
    pub keyboard_bindings: BTreeMap<String, String>,
    /// Gamepad bindings as `WS-button-name → gilrs-button-name` (empty = default).
    pub gamepad_bindings: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scale: default_scale(),
            volume: default_volume(),
            start_paused: false,
            fullscreen: false,
            rotation: RotationKind::default(),
            renderer: RendererKind::default(),
            recent_files: Vec::new(),
            keyboard_bindings: BTreeMap::new(),
            gamepad_bindings: BTreeMap::new(),
        }
    }
}

impl Config {
    /// Clamp out-of-range fields to valid values (`scale ≥ 1`, `volume ≤ 100`,
    /// recent-file history capped at [`RECENT_LIMIT`]).
    pub fn sanitised(mut self) -> Self {
        self.scale = self.scale.max(1);
        self.volume = self.volume.min(100);
        self.recent_files.truncate(RECENT_LIMIT);
        self
    }

    /// Record `path` as the most recently opened ROM.
    ///
    /// Moves it to the front, removing any earlier occurrence so the list stays
    /// duplicate-free, and drops the oldest entries past [`RECENT_LIMIT`].
    pub fn push_recent(&mut self, path: impl Into<String>) {
        let path = path.into();
        self.recent_files.retain(|p| p != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(RECENT_LIMIT);
    }

    /// Empty the recent-ROM history.
    pub fn clear_recent(&mut self) {
        self.recent_files.clear();
    }

    /// Path to the config file in the platform config directory
    /// (e.g. `~/.config/swanium/config.toml` on Linux).
    ///
    /// Returns [`SwaniumError::NoConfigDir`] if no such directory can be found
    /// (rare; e.g. a headless environment with no `HOME`).
    pub fn config_path() -> Result<PathBuf, SwaniumError> {
        let dirs =
            directories::ProjectDirs::from("", "", APP_NAME).ok_or(SwaniumError::NoConfigDir)?;
        Ok(dirs.config_dir().join(CONFIG_FILE))
    }

    /// Parse a [`Config`] from a TOML file. Out-of-range fields are clamped.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, SwaniumError> {
        let text = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&text)?;
        Ok(config.sanitised())
    }

    /// Serialise this [`Config`] to a TOML file, creating parent directories.
    pub fn save_to(&self, path: impl AsRef<Path>) -> Result<(), SwaniumError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Load the user's config from the platform config path.
    ///
    /// A missing file is the normal first-run case and yields [`Config::default`];
    /// a malformed file logs a warning and also falls back to defaults so the
    /// app still starts. Both results are sanitised.
    pub fn load() -> Self {
        let Ok(path) = Self::config_path() else {
            return Config::default();
        };
        match Self::load_from(&path) {
            Ok(config) => config,
            Err(SwaniumError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                Config::default()
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), "ignoring invalid config: {e}");
                Config::default()
            }
        }
    }

    /// Save this config to the platform config path.
    pub fn save(&self) -> Result<(), SwaniumError> {
        let path = Self::config_path()?;
        self.save_to(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scale_is_positive() {
        assert!(Config::default().scale >= 1);
    }

    #[test]
    fn default_volume_is_full() {
        assert_eq!(Config::default().volume, 100);
    }

    #[test]
    fn sanitise_raises_zero_scale_to_one() {
        let cfg = Config {
            scale: 0,
            ..Config::default()
        };
        assert_eq!(cfg.sanitised().scale, 1);
    }

    #[test]
    fn sanitise_clamps_overloud_volume() {
        let cfg = Config {
            volume: 250,
            ..Config::default()
        };
        assert_eq!(cfg.sanitised().volume, 100);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("swanium-cfg-{}", std::process::id()));
        let path = dir.join("config.toml");
        let mut cfg = Config {
            scale: 5,
            volume: 42,
            start_paused: true,
            fullscreen: true,
            rotation: RotationKind::Left,
            renderer: RendererKind::Linear,
            ..Config::default()
        };
        cfg.push_recent("/roms/a.ws");
        cfg.keyboard_bindings.insert("A".into(), "x".into());
        cfg.gamepad_bindings.insert("A".into(), "East".into());
        cfg.save_to(&path).expect("save");
        let loaded = Config::load_from(&path).expect("load");
        assert_eq!(loaded, cfg);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn push_recent_moves_to_front_without_duplicates() {
        let mut cfg = Config::default();
        cfg.push_recent("/roms/a.ws");
        cfg.push_recent("/roms/b.ws");
        cfg.push_recent("/roms/a.ws");
        assert_eq!(cfg.recent_files, vec!["/roms/a.ws", "/roms/b.ws"]);
    }

    #[test]
    fn push_recent_caps_at_limit() {
        let mut cfg = Config::default();
        for i in 0..(RECENT_LIMIT + 5) {
            cfg.push_recent(format!("/roms/{i}.ws"));
        }
        assert_eq!(cfg.recent_files.len(), RECENT_LIMIT);
        // The most recently pushed entry is first.
        assert_eq!(
            cfg.recent_files[0],
            format!("/roms/{}.ws", RECENT_LIMIT + 4)
        );
    }

    #[test]
    fn clear_recent_empties_history() {
        let mut cfg = Config::default();
        cfg.push_recent("/roms/a.ws");
        cfg.clear_recent();
        assert!(cfg.recent_files.is_empty());
    }

    #[test]
    fn sanitise_truncates_overlong_recent_history() {
        let cfg = Config {
            recent_files: (0..RECENT_LIMIT + 3).map(|i| format!("{i}.ws")).collect(),
            ..Config::default()
        };
        assert_eq!(cfg.sanitised().recent_files.len(), RECENT_LIMIT);
    }

    #[test]
    fn default_renderer_is_nearest() {
        assert_eq!(Config::default().renderer, RendererKind::Nearest);
    }

    #[test]
    fn load_fills_missing_fields_from_defaults() {
        let dir = std::env::temp_dir().join(format!("swanium-partial-{}", std::process::id()));
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(&path, "scale = 4\n").expect("write");
        let loaded = Config::load_from(&path).expect("load");
        assert_eq!(loaded.scale, 4);
        assert_eq!(loaded.volume, DEFAULT_VOLUME);
        assert!(!loaded.start_paused);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_clamps_out_of_range_values() {
        let dir = std::env::temp_dir().join(format!("swanium-clamp-{}", std::process::id()));
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(&path, "scale = 0\nvolume = 200\n").expect("write");
        let loaded = Config::load_from(&path).expect("load");
        assert_eq!(loaded.scale, 1);
        assert_eq!(loaded.volume, 100);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_missing_file_is_io_error() {
        let path = std::env::temp_dir().join("swanium-does-not-exist-zzz/config.toml");
        let err = Config::load_from(&path).expect_err("should fail");
        assert!(matches!(err, SwaniumError::Io(_)));
    }

    #[test]
    fn load_rejects_malformed_toml() {
        let dir = std::env::temp_dir().join(format!("swanium-bad-{}", std::process::id()));
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(&path, "scale = = =\n").expect("write");
        let err = Config::load_from(&path).expect_err("should fail");
        assert!(matches!(err, SwaniumError::ConfigParse(_)));
        std::fs::remove_dir_all(&dir).ok();
    }
}
