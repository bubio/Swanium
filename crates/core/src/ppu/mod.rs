//! WonderSwan PPU (Picture Processing Unit) — monochrome, tile-based 2D
//! rendering.
//!
//! The PPU renders a 224×144 screen from tile data, tile maps, and a sprite
//! attribute table that all live in the internal WRAM (0x0000–0x3FFF) shared
//! with the CPU; there is no separate VRAM. Display registers are the I/O
//! ports 0x00–0x3F, read here from the `Bus`'s shadow port array.
//!
//! Phase 4 (see `docs/dev/DevelopmentPlan.md`) drives the PPU one scanline at
//! a time via [`Ppu::render_scanline`]. The framebuffer holds resolved
//! monochrome shade indices (0–15); RGBA expansion happens in the frontend.
//! A future phase may decompose rendering to per-dot timing.

mod palette;

#[cfg(test)]
mod tests;

pub use palette::{ColorPaletteResolver, MonoPaletteResolver, PaletteResolver, Rgb444};

/// Visible screen width in pixels.
pub const SCREEN_WIDTH: usize = 224;
/// Visible screen height in pixels (scanlines).
pub const SCREEN_HEIGHT: usize = 144;

/// Number of framebuffer entries (one shade index per pixel).
const FRAMEBUFFER_LEN: usize = SCREEN_WIDTH * SCREEN_HEIGHT;

/// Display-control register (I/O port 0x00, low byte) layer/window bits.
const DISP_CTRL: usize = 0x00;
const DISP_SCR1_ENABLE: u8 = 1 << 0;
const DISP_SCR2_ENABLE: u8 = 1 << 1;
const DISP_SPR_ENABLE: u8 = 1 << 2;
const DISP_SPR_WINDOW_ENABLE: u8 = 1 << 3;
const DISP_SCR2_WINDOW_OUTSIDE: u8 = 1 << 4; // 0 = inside, 1 = outside
const DISP_SCR2_WINDOW_ENABLE: u8 = 1 << 5;

/// SCR2 window rectangle registers (inclusive bounds).
const SCR2_WINDOW_X1: usize = 0x08;
const SCR2_WINDOW_Y1: usize = 0x09;
const SCR2_WINDOW_X2: usize = 0x0A;
const SCR2_WINDOW_Y2: usize = 0x0B;
/// Sprite window rectangle registers (inclusive bounds).
const SPR_WINDOW_X1: usize = 0x0C;
const SPR_WINDOW_Y1: usize = 0x0D;
const SPR_WINDOW_X2: usize = 0x0E;
const SPR_WINDOW_Y2: usize = 0x0F;

/// Map-base register (I/O port 0x07): low nibble = SCR1 base, high = SCR2.
const MAP_BASE: usize = 0x07;
/// Background scroll registers (X/Y per screen).
const SCR1_SCROLL_X: usize = 0x10;
const SCR1_SCROLL_Y: usize = 0x11;
const SCR2_SCROLL_X: usize = 0x12;
const SCR2_SCROLL_Y: usize = 0x13;

/// 2bpp tile data (monochrome and WonderSwan Color 2bpp) lives at a fixed WRAM
/// offset; each tile is 16 bytes (8 rows × 2 planar bytes), 2 bits per pixel.
const TILE_DATA_BASE: usize = 0x2000;
const TILE_BYTES: usize = 16;
/// WonderSwan Color 4bpp tile data lives at a higher fixed offset; each tile is
/// 32 bytes (8 rows × 4 bytes), 4 bits per pixel.
const TILE_DATA_BASE_4BPP: usize = 0x4000;
const TILE_BYTES_4BPP: usize = 32;
/// Video-mode register (I/O port 0x60, WonderSwan Color only).
const VIDEO_MODE: usize = 0x60;
const VIDEO_MODE_COLOR: u8 = 1 << 7; // color palettes (1) vs mono shade pool (0)
const VIDEO_MODE_4BPP: u8 = 1 << 6; // 16-color 4bpp tiles (1) vs 4-color 2bpp (0)
const VIDEO_MODE_PACKED: u8 = 1 << 5; // packed 4bpp (1) vs planar 4bpp (0)
/// In color mode, tile-map bit 13 selects the second 512-tile bank. This is
/// only a background tile-map attribute; sprite attribute bit 13 is priority.
const TILEMAP_BANK_BIT: u16 = 1 << 13;
const TILE_BANK_SIZE: u16 = 512;
/// A tile map is 32×32 entries of 16 bits each, row-major.
const TILEMAP_COLS: usize = 32;
/// Background planes wrap at 256×256 pixels (32 tiles × 8 px).
const BG_WRAP_MASK: u16 = 0xFF;

/// Sprite attribute table registers and layout.
const SPR_BASE: usize = 0x04; // base = (value & 0x3F) << 9
const SPR_FIRST: usize = 0x05; // index of the first sprite to process
const SPR_COUNT: usize = 0x06; // number of sprites to process (0–128)
/// Sprites use palettes 8–15, so the entry's 3-bit palette is offset by 8.
const SPRITE_PALETTE_OFFSET: u8 = 8;
/// Sprite tiles are 8×8 like background tiles.
const SPRITE_SIZE: usize = 8;
/// The sprite attribute table holds up to 128 four-byte entries.
const SPRITE_TABLE_LEN: usize = 128;
/// Hardware only evaluates the first 32 sprites that overlap a scanline.
const SPRITES_PER_SCANLINE: usize = 32;

/// Decoded view of the display-control register (I/O port 0x00).
///
/// Internal to the `ppu` module (and its tests); not part of the crate's
/// public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DisplayControl {
    /// Screen 1 (background layer 1) enabled.
    pub scr1_enabled: bool,
    /// Screen 2 (background layer 2) enabled.
    pub scr2_enabled: bool,
    /// Sprite layer enabled.
    pub sprites_enabled: bool,
    /// Sprite window enabled (masks sprites whose window attribute is set).
    pub sprite_window_enabled: bool,
    /// SCR2 window enabled.
    pub scr2_window_enabled: bool,
    /// SCR2 window mode: `false` shows SCR2 inside the window, `true` outside.
    pub scr2_window_outside: bool,
}

impl DisplayControl {
    /// Decode the display-control register from the I/O port shadow array.
    ///
    /// `ports` is the 256-entry I/O port shadow (`Bus` port array); only
    /// index [`DISP_CTRL`] is read.
    pub(crate) fn from_ports(ports: &[u8]) -> Self {
        let v = ports[DISP_CTRL];
        Self {
            scr1_enabled: v & DISP_SCR1_ENABLE != 0,
            scr2_enabled: v & DISP_SCR2_ENABLE != 0,
            sprites_enabled: v & DISP_SPR_ENABLE != 0,
            sprite_window_enabled: v & DISP_SPR_WINDOW_ENABLE != 0,
            scr2_window_enabled: v & DISP_SCR2_WINDOW_ENABLE != 0,
            scr2_window_outside: v & DISP_SCR2_WINDOW_OUTSIDE != 0,
        }
    }

    fn all_layers_disabled(self) -> bool {
        !self.scr1_enabled && !self.scr2_enabled && !self.sprites_enabled
    }
}

/// PPU state: the rendered framebuffer plus the current scanline position.
///
/// The framebuffer stores one resolved [`Rgb444`] color per pixel, row-major,
/// `SCREEN_WIDTH * SCREEN_HEIGHT` entries. Monochrome output is a grey RGB444
/// value (see [`palette::grey_rgb444`]); color output is the 12-bit palette RAM
/// color. RGB444 → RGBA8888 expansion happens in `crates/video`.
#[derive(Debug, Clone)]
pub struct Ppu {
    /// Resolved RGB444 colors, row-major (`y * SCREEN_WIDTH + x`).
    framebuffer: Box<[Rgb444]>,
    /// Scanline currently being rendered (0–143 visible; up to 158 total).
    current_line: u8,
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

impl Ppu {
    /// Create a PPU with a cleared framebuffer at scanline 0.
    pub fn new() -> Self {
        Self {
            framebuffer: vec![0u16; FRAMEBUFFER_LEN].into_boxed_slice(),
            current_line: 0,
        }
    }

    /// Reset the PPU to its power-on state (cleared framebuffer, line 0).
    pub fn reset(&mut self) {
        self.framebuffer.fill(0);
        self.current_line = 0;
    }

    /// The rendered framebuffer: `SCREEN_WIDTH * SCREEN_HEIGHT` [`Rgb444`]
    /// colors, row-major. Stable read API for the frontend and future
    /// RetroAchievements integration.
    pub fn framebuffer(&self) -> &[Rgb444] {
        &self.framebuffer
    }

    /// The scanline the PPU is currently positioned at.
    pub fn current_line(&self) -> u8 {
        self.current_line
    }

    /// Render one visible scanline into the framebuffer.
    ///
    /// `line` is the scanline (0–143; lines ≥ [`SCREEN_HEIGHT`] are ignored).
    /// `wram` is the internal work RAM (tile data, tile maps); `ports` is the
    /// I/O port shadow array holding the display registers; `resolver` maps
    /// raw tile pixels to shade indices (see [`PaletteResolver`]).
    ///
    /// Compositing order, back to front: SCR1, sprites with priority 0
    /// (behind SCR2), SCR2, then sprites with priority 1 (in front of SCR2).
    /// Pixel transparency and the backdrop shade are defined by `resolver`;
    /// sprite pixels use palettes 8–15.
    pub fn render_scanline<R: PaletteResolver>(
        &mut self,
        line: u8,
        wram: &[u8],
        ports: &[u8],
        resolver: &R,
    ) {
        let y = line as usize;
        if y >= SCREEN_HEIGHT {
            return;
        }
        let dc = DisplayControl::from_ports(ports);
        let mode = TileMode::from_ports(ports);
        let row = y * SCREEN_WIDTH;
        let backdrop = resolver.backdrop(ports);

        if dc.all_layers_disabled() {
            self.framebuffer[row..row + SCREEN_WIDTH].fill(backdrop);
            self.current_line = line;
            return;
        }

        // Decode the sprite attribute table once per scanline and keep only the
        // entries that cover this line (preserving table order, i.e. priority),
        // up to the hardware's 32-sprites-per-scanline limit.
        // The per-pixel sprite sampler then walks this short list instead of
        // re-decoding all 128 entries for every pixel — the dominant PPU cost
        // (see docs/dev/Profiling.md). Zero-allocation: a stack-resident array
        // sized to the full table, filled up to `sprite_count`.
        let mut line_sprites = [SpriteEntry::default(); SPRITE_TABLE_LEN];
        let sprite_count = if dc.sprites_enabled {
            collect_line_sprites(wram, ports, line, &mut line_sprites)
        } else {
            0
        };
        let line_sprites = &line_sprites[..sprite_count];

        // Resolve each enabled background layer for the whole line up front,
        // decoding tile-map entries and tile rows once per 8-pixel span instead
        // of per pixel (see fill_background_line).
        let mut scr1_line = [BgSample::default(); SCREEN_WIDTH];
        if dc.scr1_enabled {
            fill_background_line(wram, ports, BgLayer::Scr1, line, mode, &mut scr1_line);
        }
        let mut scr2_line = [BgSample::default(); SCREEN_WIDTH];
        if dc.scr2_enabled {
            fill_background_line(wram, ports, BgLayer::Scr2, line, mode, &mut scr2_line);
        }
        let mut sprite_back_line = [None; SCREEN_WIDTH];
        let mut sprite_front_line = [None; SCREEN_WIDTH];
        if dc.sprites_enabled {
            fill_sprite_line(
                wram,
                ports,
                &dc,
                line_sprites,
                mode,
                line,
                false,
                resolver,
                &mut sprite_back_line,
            );
            fill_sprite_line(
                wram,
                ports,
                &dc,
                line_sprites,
                mode,
                line,
                true,
                resolver,
                &mut sprite_front_line,
            );
        }

        for x in 0..SCREEN_WIDTH {
            let mut color = backdrop;
            if dc.scr1_enabled {
                let s = scr1_line[x];
                if !resolver.transparent(s.palette, s.pixel) {
                    color = resolver.resolve(ports, s.palette, s.pixel);
                }
            }
            if let Some(px) = sprite_back_line[x] {
                color = px;
            }
            if dc.scr2_enabled && scr2_visible_at(&dc, ports, x, line) {
                let s = scr2_line[x];
                if !resolver.transparent(s.palette, s.pixel) {
                    color = resolver.resolve(ports, s.palette, s.pixel);
                }
            }
            if let Some(px) = sprite_front_line[x] {
                color = px;
            }
            self.framebuffer[row + x] = color;
        }
        self.current_line = line;
    }
}

/// Which background screen layer is being sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BgLayer {
    Scr1,
    Scr2,
}

/// Tile pixel format in effect, derived from the video-mode register (port
/// 0x60). Determines how [`sample_background`]/[`sample_sprite`] decode tile
/// pixels; the palette (mono shade pool vs color RAM) is handled separately by
/// the [`PaletteResolver`].
///
/// Layouts (WSdev "Display"; 4bpp byte order and Color background/sprite
/// attribute meanings cross-checked against ares `ares/ws/ppu/{memory,screen,
/// sprite}.cpp` and Mednafen `src/wswan/{gfx,tcache}.cpp`):
/// - 2bpp planar (mono and color 2bpp): 16 bytes/tile, per row two planes.
/// - 4bpp planar: 32 bytes/tile, per row four plane bytes.
/// - 4bpp packed: 32 bytes/tile, per row four bytes of two 4-bit pixels
///   (high nibble = left pixel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TileMode {
    /// 4bpp (16-color) tiles instead of 2bpp (4-color).
    bpp4: bool,
    /// Packed 4bpp byte layout instead of planar (only meaningful when `bpp4`).
    packed: bool,
    /// Color mode: background tile-map bit 13 selects the second 512-tile bank.
    banked: bool,
}

impl TileMode {
    /// Decode the tile format from the I/O port shadow (port 0x60). Monochrome
    /// hardware (or a Color console with the color bit clear) always reports
    /// 2bpp planar with no banking, matching the pre-Color behaviour.
    pub(crate) fn from_ports(ports: &[u8]) -> Self {
        let m = ports[VIDEO_MODE];
        let color = m & VIDEO_MODE_COLOR != 0;
        Self {
            bpp4: color && (m & VIDEO_MODE_4BPP != 0),
            packed: m & VIDEO_MODE_PACKED != 0,
            banked: color,
        }
    }

    /// The effective tile index for a background entry, applying the color
    /// second-bank bit when banking is active.
    fn bg_tile(self, entry: &TileMapEntry) -> u16 {
        if self.banked && entry.bank {
            entry.tile_idx + TILE_BANK_SIZE
        } else {
            entry.tile_idx
        }
    }

    /// Read a tile pixel in this format from WRAM.
    fn pixel(self, wram: &[u8], tile: u16, tx: usize, ty: usize) -> u8 {
        if self.bpp4 {
            tile_pixel_4bpp(wram, self.packed, tile, tx, ty)
        } else {
            tile_pixel(wram, tile, tx, ty)
        }
    }

    /// Read the bytes of one tile row so a whole 8-pixel span can be extracted
    /// without re-reading WRAM per pixel (see [`fill_background_line`]). Only
    /// the first two bytes are meaningful in 2bpp; all four in 4bpp.
    fn read_row(self, wram: &[u8], tile: u16, ty: usize) -> [u8; 4] {
        if self.bpp4 {
            let b = TILE_DATA_BASE_4BPP + tile as usize * TILE_BYTES_4BPP + ty * 4;
            [wram[b], wram[b + 1], wram[b + 2], wram[b + 3]]
        } else {
            let b = TILE_DATA_BASE + tile as usize * TILE_BYTES + ty * 2;
            [wram[b], wram[b + 1], 0, 0]
        }
    }

    /// Extract the pixel at in-row column `tx` (0–7) from a row previously read
    /// by [`read_row`](Self::read_row). Bit-for-bit equivalent to
    /// [`pixel`](Self::pixel) for the same tile row.
    fn pixel_in_row(self, row: &[u8; 4], tx: usize) -> u8 {
        if self.bpp4 {
            if self.packed {
                let byte = row[tx >> 1];
                if tx & 1 == 0 {
                    byte >> 4
                } else {
                    byte & 0x0F
                }
            } else {
                let bit = 7 - tx;
                let mut px = 0u8;
                for (plane, &b) in row.iter().enumerate() {
                    px |= ((b >> bit) & 1) << plane;
                }
                px
            }
        } else {
            let bit = 7 - tx;
            let lo = (row[0] >> bit) & 1;
            let hi = (row[1] >> bit) & 1;
            (hi << 1) | lo
        }
    }
}

impl Ppu {
    /// Debug helper: the raw `(pixel, palette)` a background layer samples at
    /// screen coordinate `(x, y)`. `scr2 = true` selects SCR2, else SCR1.
    /// Transparency depends on the sampled palette and raw pixel.
    pub fn debug_bg_sample(
        &self,
        wram: &[u8],
        ports: &[u8],
        scr2: bool,
        x: usize,
        y: u8,
    ) -> (u8, u8) {
        let layer = if scr2 { BgLayer::Scr2 } else { BgLayer::Scr1 };
        let s = sample_background(wram, ports, layer, x, y);
        (s.pixel, s.palette)
    }
}

/// A decoded 16-bit tile-map entry (internal to the `ppu` module).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TileMapEntry {
    /// Tile number (9 bits, 0–511); combined with [`bank`](Self::bank) in color
    /// mode to address up to 1024 tiles.
    pub tile_idx: u16,
    /// Palette index (4 bits, 0–15); consumed by the palette resolver.
    pub palette: u8,
    /// Second tile-bank select (bit 13); honoured only in color mode.
    pub bank: bool,
    /// Horizontal flip.
    pub hflip: bool,
    /// Vertical flip.
    pub vflip: bool,
}

impl TileMapEntry {
    /// Decode a tile-map entry from its little-endian 16-bit word.
    pub(crate) fn decode(word: u16) -> Self {
        Self {
            tile_idx: word & 0x01FF,
            palette: ((word >> 9) & 0x0F) as u8,
            bank: word & TILEMAP_BANK_BIT != 0,
            hflip: word & (1 << 14) != 0,
            vflip: word & (1 << 15) != 0,
        }
    }
}

/// A sampled background pixel: the raw 2-bit value plus its palette index.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct BgSample {
    pixel: u8,
    palette: u8,
}

/// Read a 2-bit pixel from planar tile data in WRAM.
///
/// `tile_idx` selects the 16-byte tile at [`TILE_DATA_BASE`]; `(tx, ty)` are
/// the in-tile pixel coordinates (0–7). Each row is two bytes: plane 0 (low
/// bit) then plane 1 (high bit), MSB = leftmost pixel.
pub(crate) fn tile_pixel(wram: &[u8], tile_idx: u16, tx: usize, ty: usize) -> u8 {
    let addr = TILE_DATA_BASE + tile_idx as usize * TILE_BYTES + ty * 2;
    let plane0 = wram[addr];
    let plane1 = wram[addr + 1];
    let bit = 7 - tx;
    let lo = (plane0 >> bit) & 1;
    let hi = (plane1 >> bit) & 1;
    (hi << 1) | lo
}

/// Read a 4-bit pixel (0–15) from a WonderSwan Color 4bpp tile in WRAM.
///
/// `tile_idx` selects the 32-byte tile at [`TILE_DATA_BASE_4BPP`]; each row is
/// four bytes. In `packed` layout a row byte holds two pixels (high nibble =
/// left); in planar layout the four bytes are bit-planes 0–3 (MSB = leftmost
/// pixel).
pub(crate) fn tile_pixel_4bpp(
    wram: &[u8],
    packed: bool,
    tile_idx: u16,
    tx: usize,
    ty: usize,
) -> u8 {
    let row = TILE_DATA_BASE_4BPP + tile_idx as usize * TILE_BYTES_4BPP + ty * 4;
    if packed {
        let byte = wram[row + (tx >> 1)];
        if tx & 1 == 0 {
            byte >> 4
        } else {
            byte & 0x0F
        }
    } else {
        let bit = 7 - tx;
        let mut px = 0u8;
        for plane in 0..4 {
            px |= ((wram[row + plane] >> bit) & 1) << plane;
        }
        px
    }
}

/// Read a tile-map entry at tile coordinates `(col, row)` from a map at
/// `base` in WRAM.
fn tilemap_entry(wram: &[u8], base: usize, col: usize, row: usize) -> TileMapEntry {
    let addr = base + (row * TILEMAP_COLS + col) * 2;
    let word = u16::from_le_bytes([wram[addr], wram[addr + 1]]);
    TileMapEntry::decode(word)
}

/// Tile-map base offset in WRAM for a background layer.
///
/// Monochrome hardware uses the low 3 bits of each nibble, limiting maps to the
/// 16 KiB mono WRAM window. Color mode keeps the full nibble so screen maps can
/// live in the upper Color WRAM range.
fn map_base(ports: &[u8], layer: BgLayer) -> usize {
    let nibble = match layer {
        BgLayer::Scr1 => ports[MAP_BASE] & 0x0F,
        BgLayer::Scr2 => (ports[MAP_BASE] >> 4) & 0x0F,
    };
    let mask = if ports[VIDEO_MODE] & VIDEO_MODE_COLOR != 0 {
        0x0F
    } else {
        0x07
    };
    ((nibble & mask) as usize) << 11
}

/// Sample a background layer at visible screen coordinate `(screen_x, line)`,
/// applying the layer's scroll offset and tile flips.
fn sample_background(
    wram: &[u8],
    ports: &[u8],
    layer: BgLayer,
    screen_x: usize,
    line: u8,
) -> BgSample {
    let mode = TileMode::from_ports(ports);
    let (scroll_x, scroll_y) = match layer {
        BgLayer::Scr1 => (ports[SCR1_SCROLL_X], ports[SCR1_SCROLL_Y]),
        BgLayer::Scr2 => (ports[SCR2_SCROLL_X], ports[SCR2_SCROLL_Y]),
    };
    let bg_x = (screen_x as u16 + scroll_x as u16) & BG_WRAP_MASK;
    let bg_y = (line as u16 + scroll_y as u16) & BG_WRAP_MASK;

    let base = map_base(ports, layer);
    let entry = tilemap_entry(wram, base, (bg_x >> 3) as usize, (bg_y >> 3) as usize);

    let mut tx = (bg_x & 7) as usize;
    let mut ty = (bg_y & 7) as usize;
    if entry.hflip {
        tx = 7 - tx;
    }
    if entry.vflip {
        ty = 7 - ty;
    }
    BgSample {
        pixel: mode.pixel(wram, mode.bg_tile(&entry), tx, ty),
        palette: entry.palette,
    }
}

/// Fill `out` with a whole background scanline for `layer` in one pass.
///
/// Equivalent to calling [`sample_background`] for every `screen_x`, but the
/// tile-map entry and the tile's row bytes are decoded once per 8-pixel tile
/// span (they only change at tile boundaries) rather than per pixel — the
/// dominant remaining PPU cost after the sprite fix (see `docs/dev/Profiling.md`).
/// `mode` is hoisted from the caller so it is computed once per line, not per
/// pixel.
fn fill_background_line(
    wram: &[u8],
    ports: &[u8],
    layer: BgLayer,
    line: u8,
    mode: TileMode,
    out: &mut [BgSample; SCREEN_WIDTH],
) {
    let (scroll_x, scroll_y) = match layer {
        BgLayer::Scr1 => (ports[SCR1_SCROLL_X], ports[SCR1_SCROLL_Y]),
        BgLayer::Scr2 => (ports[SCR2_SCROLL_X], ports[SCR2_SCROLL_Y]),
    };
    let bg_y = (line as u16 + scroll_y as u16) & BG_WRAP_MASK;
    let map_row = (bg_y >> 3) as usize;
    let base = map_base(ports, layer);

    // Cached state for the current tile column; recomputed only when the span
    // changes. `cur_col = usize::MAX` forces a load on the first pixel.
    let mut cur_col = usize::MAX;
    let mut palette = 0u8;
    let mut hflip = false;
    let mut row_bytes = [0u8; 4];

    for (x, slot) in out.iter_mut().enumerate() {
        let bg_x = (x as u16 + scroll_x as u16) & BG_WRAP_MASK;
        let col = (bg_x >> 3) as usize;
        if col != cur_col {
            cur_col = col;
            let entry = tilemap_entry(wram, base, col, map_row);
            palette = entry.palette;
            hflip = entry.hflip;
            let ty = if entry.vflip {
                7 - (bg_y & 7) as usize
            } else {
                (bg_y & 7) as usize
            };
            row_bytes = mode.read_row(wram, mode.bg_tile(&entry), ty);
        }
        let mut tx = (bg_x & 7) as usize;
        if hflip {
            tx = 7 - tx;
        }
        *slot = BgSample {
            pixel: mode.pixel_in_row(&row_bytes, tx),
            palette,
        };
    }
}

/// A decoded 4-byte sprite attribute-table entry (internal to the `ppu`
/// module).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SpriteEntry {
    /// Tile number (9 bits, 0–511).
    pub tile_idx: u16,
    /// Palette index (3 bits, 0–7); offset by 8 to select palettes 8–15.
    pub palette: u8,
    /// Priority: `true` draws in front of SCR2, `false` behind it.
    pub priority: bool,
    /// Window attribute (bit 12): when the sprite window is enabled, such a
    /// sprite is clipped out inside the sprite window and remains visible
    /// outside it.
    pub window: bool,
    /// Horizontal flip.
    pub hflip: bool,
    /// Vertical flip.
    pub vflip: bool,
    /// Top-left X coordinate.
    pub x: u8,
    /// Top-left Y coordinate.
    pub y: u8,
}

impl SpriteEntry {
    /// Decode the 4-byte sprite entry at `addr` in WRAM. The first two bytes
    /// are the little-endian attribute word; byte 2 is Y, byte 3 is X.
    pub(crate) fn decode(wram: &[u8], addr: usize) -> Self {
        let word = u16::from_le_bytes([wram[addr], wram[addr + 1]]);
        Self {
            tile_idx: word & 0x01FF,
            palette: ((word >> 9) & 0x07) as u8,
            window: word & (1 << 12) != 0,
            priority: word & (1 << 13) != 0,
            hflip: word & (1 << 14) != 0,
            vflip: word & (1 << 15) != 0,
            y: wram[addr + 2],
            x: wram[addr + 3],
        }
    }
}

/// True if pixel `(x, y)` lies within the inclusive rectangle whose corner
/// registers start at `rect_base` (X1, Y1, X2, Y2 in consecutive ports).
fn in_window(ports: &[u8], x: usize, y: u8, x1: usize, y1: usize, x2: usize, y2: usize) -> bool {
    let xb = x as u8;
    xb >= ports[x1] && xb <= ports[x2] && y >= ports[y1] && y <= ports[y2]
}

/// Whether SCR2 is visible at `(x, line)` given its window configuration.
fn scr2_visible_at(dc: &DisplayControl, ports: &[u8], x: usize, line: u8) -> bool {
    if !dc.scr2_window_enabled {
        return true;
    }
    let inside = in_window(
        ports,
        x,
        line,
        SCR2_WINDOW_X1,
        SCR2_WINDOW_Y1,
        SCR2_WINDOW_X2,
        SCR2_WINDOW_Y2,
    );
    if dc.scr2_window_outside {
        !inside
    } else {
        inside
    }
}

/// Decode the sprite attribute table into `out`, keeping (in table order,
/// i.e. priority order) only the sprites whose 8-pixel-tall box covers `line`.
/// Collection stops after 32 matching sprites, matching the hardware's
/// per-scanline overflow behavior: later OAM entries on that line are ignored
/// even if earlier entries are transparent at a given pixel.
/// Returns how many entries were written.
///
/// Called once per scanline by [`Ppu::render_scanline`] so the per-pixel
/// sprite sampler works from a short pre-filtered list rather than re-decoding
/// all 128 entries for every pixel.
fn collect_line_sprites(
    wram: &[u8],
    ports: &[u8],
    line: u8,
    out: &mut [SpriteEntry; SPRITE_TABLE_LEN],
) -> usize {
    let oam_base = ((ports[SPR_BASE] as usize) & 0x3F) << 9;
    let first = ports[SPR_FIRST] as usize;
    let count = (ports[SPR_COUNT] as usize).min(SPRITE_TABLE_LEN);

    let mut n = 0;
    for i in 0..count {
        let idx = (first + i) & (SPRITE_TABLE_LEN - 1);
        let sprite = SpriteEntry::decode(wram, oam_base + idx * 4);
        if sprite_axis_delta(line, sprite.y) < SPRITE_SIZE {
            out[n] = sprite;
            n += 1;
            if n == SPRITES_PER_SCANLINE {
                break;
            }
        }
    }
    n
}

fn sprite_axis_delta(screen: u8, origin: u8) -> usize {
    screen.wrapping_sub(origin) as usize
}

#[allow(clippy::too_many_arguments)]
fn fill_sprite_line<R: PaletteResolver>(
    wram: &[u8],
    ports: &[u8],
    dc: &DisplayControl,
    line_sprites: &[SpriteEntry],
    mode: TileMode,
    line: u8,
    want_priority: bool,
    resolver: &R,
    out: &mut [Option<Rgb444>; SCREEN_WIDTH],
) {
    for sprite in line_sprites {
        if sprite.priority != want_priority {
            continue;
        }
        // The line ∈ [y, y+8) test was already applied by `collect_line_sprites`.
        let mut ty = sprite_axis_delta(line, sprite.y);
        if sprite.vflip {
            ty = SPRITE_SIZE - 1 - ty;
        }

        for dx in 0..SPRITE_SIZE {
            let screen_x = sprite.x.wrapping_add(dx as u8) as usize;
            if screen_x >= SCREEN_WIDTH || out[screen_x].is_some() {
                continue;
            }
            // A window-attributed sprite is hidden inside the sprite window and
            // shown outside it. Golden Axe parks the sprite window off-screen
            // while using bit 12 on character sprites, so the opposite
            // interpretation removes the actors while leaving backgrounds/HUD.
            if dc.sprite_window_enabled
                && sprite.window
                && in_window(
                    ports,
                    screen_x,
                    line,
                    SPR_WINDOW_X1,
                    SPR_WINDOW_Y1,
                    SPR_WINDOW_X2,
                    SPR_WINDOW_Y2,
                )
            {
                continue;
            }
            let tx = if sprite.hflip {
                SPRITE_SIZE - 1 - dx
            } else {
                dx
            };
            // Sprites carry no bank bit (attribute bit 13 is priority), so their
            // tiles are limited to 0–511, but they follow the active 2bpp/4bpp
            // format like backgrounds.
            let pixel = mode.pixel(wram, sprite.tile_idx, tx, ty);
            let palette = sprite.palette + SPRITE_PALETTE_OFFSET;
            if !resolver.transparent(palette, pixel) {
                out[screen_x] = Some(resolver.resolve(ports, palette, pixel));
            }
        }
    }
}
