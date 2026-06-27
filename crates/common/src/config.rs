//! Frontend configuration.
//!
//! A plain settings record shared across the frontend. On-disk persistence
//! (TOML load/save) is wired together with the Slint settings UI in the GUI
//! step — see `docs/dev/DevelopmentPlan.md` Phase 7 後続課題; the typed defaults
//! here let the rest of the frontend be built and tested against stable values
//! in the meantime.

/// Integer window scale applied to the 224×144 framebuffer.
pub const DEFAULT_SCALE: u32 = 3;

/// Default master volume (0–100).
pub const DEFAULT_VOLUME: u8 = 100;

/// User-facing frontend settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Integer scale factor for the output window.
    pub scale: u32,
    /// Master volume, 0 (mute) to 100 (full).
    pub volume: u8,
    /// Whether emulation starts paused.
    pub start_paused: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scale: DEFAULT_SCALE,
            volume: DEFAULT_VOLUME,
            start_paused: false,
        }
    }
}

impl Config {
    /// Clamp out-of-range fields to valid values (`scale ≥ 1`, `volume ≤ 100`).
    pub fn sanitised(mut self) -> Self {
        self.scale = self.scale.max(1);
        self.volume = self.volume.min(100);
        self
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
}
