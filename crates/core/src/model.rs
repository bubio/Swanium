//! Hardware model selection (WonderSwan / WonderSwan Color / SwanCrystal).
//!
//! The model is a plain, `Copy` enum threaded through the [`Bus`](crate::bus)
//! so that model-dependent behaviour — the palette resolver, tile formats, the
//! internal-RAM window, RTC presence — can branch on it without global state
//! (see `docs/dev/DevelopmentPlan.md` §6 and the RetroAchievements/FFI notes in
//! §7). Phase 8 realises the Color-specific paths; monochrome remains the
//! default so every earlier phase keeps behaving identically.

/// Which WonderSwan hardware variant the core emulates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HardwareModel {
    /// Original monochrome WonderSwan (`WS`).
    #[default]
    Mono,
    /// WonderSwan Color (`WSC`).
    Color,
    /// SwanCrystal — Color-compatible hardware with a different LCD panel.
    Crystal,
}

impl HardwareModel {
    /// Whether this model has the WonderSwan Color feature set: the 12-bit RGB
    /// palette RAM, 4bpp/packed tiles, the second tile bank, and the full 64 KiB
    /// internal RAM window. True for [`Color`](Self::Color) and
    /// [`Crystal`](Self::Crystal), false for [`Mono`](Self::Mono).
    pub fn is_color(self) -> bool {
        matches!(self, Self::Color | Self::Crystal)
    }

    /// Pick a default model for a cartridge from its header's Color-required
    /// flag: a Color-only cartridge implies [`Color`](Self::Color), otherwise
    /// [`Mono`](Self::Mono). The frontend may override this (e.g. to run a
    /// Color-capable cartridge on [`Crystal`](Self::Crystal)).
    pub fn from_color_flag(color_required: bool) -> Self {
        if color_required {
            Self::Color
        } else {
            Self::Mono
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HardwareModel;

    #[test]
    fn default_model_is_mono() {
        assert_eq!(HardwareModel::default(), HardwareModel::Mono);
    }

    #[test]
    fn mono_is_not_color() {
        assert!(!HardwareModel::Mono.is_color());
    }

    #[test]
    fn color_is_color() {
        assert!(HardwareModel::Color.is_color());
    }

    #[test]
    fn crystal_is_color() {
        assert!(HardwareModel::Crystal.is_color());
    }

    #[test]
    fn color_flag_set_selects_color() {
        assert_eq!(HardwareModel::from_color_flag(true), HardwareModel::Color);
    }

    #[test]
    fn color_flag_clear_selects_mono() {
        assert_eq!(HardwareModel::from_color_flag(false), HardwareModel::Mono);
    }
}
