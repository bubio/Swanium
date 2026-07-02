//! Palette resolution: maps a raw tile pixel to a final framebuffer shade.
//!
//! Monochrome and (future) color hardware differ essentially only in this
//! step, so the renderer is generic over [`PaletteResolver`]. Phase 8 adds a
//! color resolver that reads the palette RAM at WRAM 0xFE00 instead of the
//! monochrome shade pool.

/// Resolves tile pixels and palette state to framebuffer shade indices.
pub trait PaletteResolver {
    /// Resolve tile `pixel` (0–3) under `palette` (0–15) to a shade index,
    /// reading the palette/pool registers from the I/O port shadow `ports`.
    fn resolve(&self, ports: &[u8], palette: u8, pixel: u8) -> u8;

    /// Whether tile `pixel` under `palette` should be skipped as transparent.
    fn transparent(&self, palette: u8, pixel: u8) -> bool;

    /// Resolve the shade used when no screen or sprite pixel is opaque.
    fn backdrop(&self, ports: &[u8]) -> u8;
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
    fn resolve(&self, ports: &[u8], palette: u8, pixel: u8) -> u8 {
        let pal_addr = PALETTE_BASE + (palette as usize & 0x0F) * 2;
        // pixels 0,1 are in the low byte; 2,3 in the high byte.
        let byte = ports[pal_addr + (pixel as usize >> 1)];
        // even pixel → low nibble, odd pixel → high nibble; pool index is 3 bits.
        let pool_index = if pixel & 1 == 0 {
            byte & 0x07
        } else {
            (byte >> 4) & 0x07
        };
        shade_from_pool(ports, pool_index)
    }

    fn transparent(&self, palette: u8, pixel: u8) -> bool {
        // WSdev Display: in 2bpp mode, palette color zero is opaque for
        // palettes 0–3 and 8–11; transparent for the other palettes.
        pixel == 0 && !matches!(palette & 0x0F, 0..=3 | 8..=11)
    }

    fn backdrop(&self, ports: &[u8]) -> u8 {
        shade_from_pool(ports, ports[DISP_CTRL_HI] & 0x07)
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
