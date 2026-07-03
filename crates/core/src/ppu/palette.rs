//! Palette resolution: maps a raw tile pixel to a final framebuffer color.
//!
//! The framebuffer stores one **12-bit RGB (RGB444, `0x0RGB`)** value per
//! pixel — the WonderSwan Color's native color depth. Monochrome and color
//! hardware differ essentially only in this resolution step, so the renderer is
//! generic over [`PaletteResolver`]: [`MonoPaletteResolver`] walks the shade
//! pool and returns a grey RGB444 value; Phase 8b adds a color resolver that
//! reads the 12-bit palette RAM at WRAM 0xFE00. RGB444 → RGBA8888 expansion for
//! display happens in `crates/video`.

/// A resolved framebuffer color: 12-bit RGB packed as `0x0RGB` (RGB444).
pub type Rgb444 = u16;

/// Resolves tile pixels and palette state to framebuffer colors (RGB444).
pub trait PaletteResolver {
    /// Resolve tile `pixel` (0–3 for 2bpp) under `palette` (0–15) to an RGB444
    /// color, reading the palette/pool registers from the I/O port shadow
    /// `ports` (and, for color hardware, the palette RAM in `wram`).
    fn resolve(&self, ports: &[u8], palette: u8, pixel: u8) -> Rgb444;

    /// Whether tile `pixel` under `palette` should be skipped as transparent.
    fn transparent(&self, palette: u8, pixel: u8) -> bool;

    /// Resolve the color used when no screen or sprite pixel is opaque.
    fn backdrop(&self, ports: &[u8]) -> Rgb444;
}

/// Expand a 4-bit monochrome shade (`0` = lightest, `15` = darkest) to a grey
/// RGB444 value.
///
/// The framebuffer's RGB444 channels run `0` = dark … `15` = bright (the color
/// hardware's convention), so monochrome brightness is *inverted* here: shade
/// `0` becomes `0x0FFF` (white) and shade `15` becomes `0x0000` (black). This
/// keeps the final on-screen image identical to the pre-Phase-8 monochrome path.
pub(crate) fn grey_rgb444(shade: u8) -> Rgb444 {
    let n = (15 - (shade & 0x0F)) as u16;
    (n << 8) | (n << 4) | n
}

/// Base I/O port of the 16 monochrome palettes (two bytes each, four 3-bit
/// pool indices per palette).
const PALETTE_BASE: usize = 0x20;
/// Base I/O port of the 8-entry shade pool (two 4-bit shades per byte).
const SHADE_POOL_BASE: usize = 0x1C;
/// Display control high byte: monochrome backdrop shade-pool index in bits 0–2.
const DISP_CTRL_HI: usize = 0x01;

/// Monochrome palette resolver.
///
/// Implements the WonderSwan monochrome chain: tile pixel (2-bit) selects a
/// pool index (3-bit) from palette ports 0x20–0x3F, which selects a shade
/// (4-bit) from the shade pool ports 0x1C–0x1F.
#[derive(Debug, Clone, Copy, Default)]
pub struct MonoPaletteResolver;

impl PaletteResolver for MonoPaletteResolver {
    fn resolve(&self, ports: &[u8], palette: u8, pixel: u8) -> Rgb444 {
        let pal_addr = PALETTE_BASE + (palette as usize & 0x0F) * 2;
        // pixels 0,1 are in the low byte; 2,3 in the high byte.
        let byte = ports[pal_addr + (pixel as usize >> 1)];
        // even pixel → low nibble, odd pixel → high nibble; pool index is 3 bits.
        let pool_index = if pixel & 1 == 0 {
            byte & 0x07
        } else {
            (byte >> 4) & 0x07
        };
        grey_rgb444(shade_from_pool(ports, pool_index))
    }

    fn transparent(&self, palette: u8, pixel: u8) -> bool {
        // WSdev Display: in 2bpp mode, palette color zero is opaque for
        // palettes 0–3 and 8–11; transparent for the other palettes.
        pixel == 0 && !matches!(palette & 0x0F, 0..=3 | 8..=11)
    }

    fn backdrop(&self, ports: &[u8]) -> Rgb444 {
        grey_rgb444(shade_from_pool(ports, ports[DISP_CTRL_HI] & 0x07))
    }
}

/// Read a 4-bit shade from the shade pool (I/O ports 0x1C–0x1F, two entries
/// per byte).
fn shade_from_pool(ports: &[u8], index: u8) -> u8 {
    let byte = ports[SHADE_POOL_BASE + (index as usize >> 1)];
    if index & 1 == 0 {
        byte & 0x0F
    } else {
        (byte >> 4) & 0x0F
    }
}

/// Base offset of the WonderSwan Color palette RAM inside the 64 KiB internal
/// RAM: 16 palettes × 16 colors × 2 bytes = 512 bytes at 0xFE00–0xFFFF.
pub(crate) const PALETTE_RAM_BASE: usize = 0xFE00;

/// WonderSwan Color palette resolver.
///
/// Color hardware stores each color as a 12-bit RGB (RGB444) value in two
/// little-endian bytes of palette RAM — low byte `GGGG_BBBB`, high byte
/// `0000_RRRR`, i.e. exactly the [`Rgb444`] `0x0RGB` layout. A tile's palette
/// index (0–15) and color index (the tile pixel: 0–3 in 2bpp, 0–15 in 4bpp)
/// address `PALETTE_RAM_BASE + (palette*16 + color)*2`.
///
/// The resolver borrows the internal RAM (indexing the palette region) so the
/// [`PaletteResolver`] methods keep the same `ports`-only signature the
/// monochrome path uses.
#[derive(Debug, Clone, Copy)]
pub struct ColorPaletteResolver<'a> {
    /// The palette-RAM region (`wram[PALETTE_RAM_BASE..]`).
    palette_ram: &'a [u8],
}

impl<'a> ColorPaletteResolver<'a> {
    /// Create a color resolver over the internal RAM `wram` (which must be at
    /// least [`PALETTE_RAM_BASE`] bytes long — always true for the 64 KiB WSC
    /// RAM).
    pub fn new(wram: &'a [u8]) -> Self {
        Self {
            palette_ram: &wram[PALETTE_RAM_BASE..],
        }
    }

    /// The RGB444 color at palette `palette` (0–15), color index `color`
    /// (0–15).
    fn color(&self, palette: u8, color: u8) -> Rgb444 {
        let entry = (palette as usize & 0x0F) * 16 + (color as usize & 0x0F);
        let a = entry * 2;
        u16::from_le_bytes([self.palette_ram[a], self.palette_ram[a + 1]]) & 0x0FFF
    }
}

impl PaletteResolver for ColorPaletteResolver<'_> {
    fn resolve(&self, _ports: &[u8], palette: u8, pixel: u8) -> Rgb444 {
        self.color(palette, pixel)
    }

    fn transparent(&self, _palette: u8, pixel: u8) -> bool {
        // In color mode, color index 0 of every palette is transparent (the
        // monochrome palettes-0–3/8–11 opacity exception does not apply).
        pixel == 0
    }

    fn backdrop(&self, ports: &[u8]) -> Rgb444 {
        // Back color register (I/O port 0x01) is an 8-bit index into the 256
        // palette-RAM colors: high nibble = palette, low nibble = color.
        // (Unverified against hardware; see DevelopmentPlan "リスクと不確実性".)
        self.color(ports[DISP_CTRL_HI] >> 4, ports[DISP_CTRL_HI] & 0x0F)
    }
}
