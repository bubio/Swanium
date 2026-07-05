//! Conversion of the emulator core's framebuffer into an RGBA8 image.
//!
//! The core produces a `224 × 144` buffer of 12-bit RGB colors (RGB444,
//! packed `0x0RGB`; see [`swanium_core::system::System::framebuffer`]) — the
//! WonderSwan Color's native depth. Monochrome output uses the same buffer with
//! grey values. This crate expands each RGB444 color into a packed `RGBA8888`
//! pixel the GPU can upload as a texture. The actual wgpu surface, swap chain,
//! and scaling pipeline are wired in a later step (see
//! `docs/dev/DevelopmentPlan.md` Phase 7 後続課題); keeping the pixel conversion
//! here as a pure, testable function lets the rest of the frontend and CI
//! exercise the data path without a GPU.

/// Visible screen width in pixels.
pub const SCREEN_WIDTH: usize = 224;

/// Visible screen height in pixels.
pub const SCREEN_HEIGHT: usize = 144;

/// Bytes per RGBA8 pixel.
pub const BYTES_PER_PIXEL: usize = 4;

/// Expand a 12-bit RGB444 color (`0x0RGB`) into an opaque RGBA8 pixel.
///
/// Each 4-bit channel is scaled to 8 bits by `n × 17` (so `0x0` → `0x00` and
/// `0xF` → `0xFF`); bits above the low 12 are ignored. The alpha byte is always
/// `0xFF`.
pub fn rgb444_to_rgba(color: u16) -> [u8; 4] {
    let r = ((color >> 8) & 0x0F) as u8 * 17;
    let g = ((color >> 4) & 0x0F) as u8 * 17;
    let b = (color & 0x0F) as u8 * 17;
    [r, g, b, 0xFF]
}

/// Convert a framebuffer of RGB444 colors into a freshly allocated RGBA8 buffer.
///
/// The returned vector has `framebuffer.len() * 4` bytes. Prefer
/// [`write_rgba`] in a hot loop to reuse an existing allocation.
pub fn framebuffer_to_rgba(framebuffer: &[u16]) -> Vec<u8> {
    let mut out = vec![0u8; framebuffer.len() * BYTES_PER_PIXEL];
    write_rgba(framebuffer, &mut out);
    out
}

/// Convert a framebuffer of RGB444 colors into `out`, reusing its allocation.
///
/// # Panics
///
/// Panics if `out` is shorter than `framebuffer.len() * 4`.
pub fn write_rgba(framebuffer: &[u16], out: &mut [u8]) {
    assert!(
        out.len() >= framebuffer.len() * BYTES_PER_PIXEL,
        "output buffer too small: {} < {}",
        out.len(),
        framebuffer.len() * BYTES_PER_PIXEL
    );
    for (color, pixel) in framebuffer
        .iter()
        .zip(out.chunks_exact_mut(BYTES_PER_PIXEL))
    {
        pixel.copy_from_slice(&rgb444_to_rgba(*color));
    }
}

/// Convert the framebuffer into `out`, rotated 90° clockwise.
///
/// Used for vertical-orientation games: the core always renders a
/// [`SCREEN_WIDTH`]×[`SCREEN_HEIGHT`] (224×144) buffer, and this produces the
/// 144×224 image the player sees with the console turned on its side. The
/// destination is written row-major at the rotated dimensions (width
/// [`SCREEN_HEIGHT`], height [`SCREEN_WIDTH`]).
///
/// A source pixel `(sx, sy)` maps to destination `(SCREEN_HEIGHT-1-sy, sx)`.
///
/// # Panics
///
/// Panics if `framebuffer` is not exactly `SCREEN_WIDTH * SCREEN_HEIGHT` long,
/// or if `out` is shorter than `framebuffer.len() * 4`.
pub fn write_rgba_rotated_cw(framebuffer: &[u16], out: &mut [u8]) {
    assert_eq!(
        framebuffer.len(),
        SCREEN_WIDTH * SCREEN_HEIGHT,
        "rotation expects a full {SCREEN_WIDTH}×{SCREEN_HEIGHT} framebuffer"
    );
    assert!(
        out.len() >= framebuffer.len() * BYTES_PER_PIXEL,
        "output buffer too small: {} < {}",
        out.len(),
        framebuffer.len() * BYTES_PER_PIXEL
    );
    // Destination is SCREEN_HEIGHT wide by SCREEN_WIDTH tall. Walk it row-major
    // so writes to `out` stay sequential; gather each pixel from the source.
    for (dy, sx) in (0..SCREEN_WIDTH).enumerate() {
        for (dx, inv_sy) in (0..SCREEN_HEIGHT).enumerate() {
            let sy = SCREEN_HEIGHT - 1 - inv_sy;
            let color = framebuffer[sy * SCREEN_WIDTH + sx];
            let dst = (dy * SCREEN_HEIGHT + dx) * BYTES_PER_PIXEL;
            out[dst..dst + BYTES_PER_PIXEL].copy_from_slice(&rgb444_to_rgba(color));
        }
    }
}

/// Convert the framebuffer into `out`, rotated 90° counter-clockwise.
///
/// The counter-clockwise twin of [`write_rgba_rotated_cw`]: same 144×224
/// destination, but a source pixel `(sx, sy)` maps to destination
/// `(sy, SCREEN_WIDTH-1-sx)`.
///
/// # Panics
///
/// Panics if `framebuffer` is not exactly `SCREEN_WIDTH * SCREEN_HEIGHT` long,
/// or if `out` is shorter than `framebuffer.len() * 4`.
pub fn write_rgba_rotated_ccw(framebuffer: &[u16], out: &mut [u8]) {
    assert_eq!(
        framebuffer.len(),
        SCREEN_WIDTH * SCREEN_HEIGHT,
        "rotation expects a full {SCREEN_WIDTH}×{SCREEN_HEIGHT} framebuffer"
    );
    assert!(
        out.len() >= framebuffer.len() * BYTES_PER_PIXEL,
        "output buffer too small: {} < {}",
        out.len(),
        framebuffer.len() * BYTES_PER_PIXEL
    );
    // Destination is SCREEN_HEIGHT wide by SCREEN_WIDTH tall. Walk it row-major.
    for (inv_dy, sx) in (0..SCREEN_WIDTH).enumerate() {
        let dy = SCREEN_WIDTH - 1 - inv_dy;
        for (dx, sy) in (0..SCREEN_HEIGHT).enumerate() {
            let color = framebuffer[sy * SCREEN_WIDTH + sx];
            let dst = (dy * SCREEN_HEIGHT + dx) * BYTES_PER_PIXEL;
            out[dst..dst + BYTES_PER_PIXEL].copy_from_slice(&rgb444_to_rgba(color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_rgb444_is_white() {
        assert_eq!(rgb444_to_rgba(0x0FFF), [0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn black_rgb444_is_black() {
        assert_eq!(rgb444_to_rgba(0x0000), [0x00, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn channels_map_to_their_positions() {
        // Pure red, green, blue at full 4-bit intensity.
        assert_eq!(rgb444_to_rgba(0x0F00), [0xFF, 0x00, 0x00, 0xFF]);
        assert_eq!(rgb444_to_rgba(0x00F0), [0x00, 0xFF, 0x00, 0xFF]);
        assert_eq!(rgb444_to_rgba(0x000F), [0x00, 0x00, 0xFF, 0xFF]);
    }

    #[test]
    fn conversion_is_always_opaque() {
        assert_eq!(rgb444_to_rgba(0x0777)[3], 0xFF);
    }

    #[test]
    fn high_nibble_is_ignored() {
        assert_eq!(rgb444_to_rgba(0xF000), rgb444_to_rgba(0x0000));
    }

    #[test]
    fn framebuffer_to_rgba_has_four_bytes_per_pixel() {
        let fb = vec![0u16; SCREEN_WIDTH * SCREEN_HEIGHT];
        assert_eq!(
            framebuffer_to_rgba(&fb).len(),
            SCREEN_WIDTH * SCREEN_HEIGHT * 4
        );
    }

    #[test]
    fn framebuffer_to_rgba_converts_first_pixel() {
        let mut fb = vec![0u16; SCREEN_WIDTH * SCREEN_HEIGHT];
        fb[0] = 0x0FFF;
        assert_eq!(&framebuffer_to_rgba(&fb)[0..4], &[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn write_rgba_reuses_buffer() {
        let fb = [0x0000u16, 0x0FFF];
        let mut out = vec![0u8; 8];
        write_rgba(&fb, &mut out);
        assert_eq!(out, vec![0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    #[should_panic(expected = "output buffer too small")]
    fn write_rgba_panics_on_small_buffer() {
        write_rgba(&[0u16; 4], &mut [0u8; 4]);
    }

    #[test]
    fn rotated_output_has_same_pixel_count() {
        let fb = vec![0u16; SCREEN_WIDTH * SCREEN_HEIGHT];
        let mut out = vec![0u8; fb.len() * BYTES_PER_PIXEL];
        write_rgba_rotated_cw(&fb, &mut out);
        assert_eq!(out.len(), SCREEN_WIDTH * SCREEN_HEIGHT * BYTES_PER_PIXEL);
    }

    #[test]
    fn rotated_top_left_source_lands_bottom_left_of_rotated_image() {
        // Source (0,0) → destination (SCREEN_HEIGHT-1, 0): rotating CW, the
        // original top-left corner ends up at the bottom-left of the new image.
        let mut fb = vec![0u16; SCREEN_WIDTH * SCREEN_HEIGHT];
        fb[0] = 0x0F00; // pure red
        let mut out = vec![0u8; fb.len() * BYTES_PER_PIXEL];
        write_rgba_rotated_cw(&fb, &mut out);
        // Rotated image is SCREEN_HEIGHT wide; dst (dx, dy) = (SCREEN_HEIGHT-1, 0),
        // i.e. row 0, column SCREEN_HEIGHT-1.
        let dst = (SCREEN_HEIGHT - 1) * BYTES_PER_PIXEL;
        assert_eq!(&out[dst..dst + 4], &[0xFF, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn rotated_top_right_source_lands_top_left_of_rotated_image() {
        // Source (SCREEN_WIDTH-1, 0) → destination (SCREEN_HEIGHT-1, SCREEN_WIDTH-1).
        let mut fb = vec![0u16; SCREEN_WIDTH * SCREEN_HEIGHT];
        fb[SCREEN_WIDTH - 1] = 0x00F0; // pure green
        let mut out = vec![0u8; fb.len() * BYTES_PER_PIXEL];
        write_rgba_rotated_cw(&fb, &mut out);
        let dst = ((SCREEN_HEIGHT - 1) + (SCREEN_WIDTH - 1) * SCREEN_HEIGHT) * BYTES_PER_PIXEL;
        assert_eq!(&out[dst..dst + 4], &[0x00, 0xFF, 0x00, 0xFF]);
    }

    #[test]
    #[should_panic(expected = "full")]
    fn write_rgba_rotated_cw_panics_on_wrong_size() {
        write_rgba_rotated_cw(&[0u16; 4], &mut [0u8; 16]);
    }

    #[test]
    fn rotated_ccw_top_left_source_lands_top_right_of_rotated_image() {
        // Source (0,0) → destination (0, SCREEN_WIDTH-1): rotating CCW, the
        // original top-left corner ends up at the top-right of the new image.
        let mut fb = vec![0u16; SCREEN_WIDTH * SCREEN_HEIGHT];
        fb[0] = 0x0F00; // pure red
        let mut out = vec![0u8; fb.len() * BYTES_PER_PIXEL];
        write_rgba_rotated_ccw(&fb, &mut out);
        // Rotated image is SCREEN_HEIGHT wide; dst (dx, dy) = (0, SCREEN_WIDTH-1).
        let dst = ((SCREEN_WIDTH - 1) * SCREEN_HEIGHT) * BYTES_PER_PIXEL;
        assert_eq!(&out[dst..dst + 4], &[0xFF, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn cw_and_ccw_are_mirror_images_of_each_other() {
        // Rotating CW then reading a pixel that CCW would place at the opposite
        // corner: the two rotations must not coincide for an asymmetric input.
        let mut fb = vec![0u16; SCREEN_WIDTH * SCREEN_HEIGHT];
        fb[0] = 0x0FFF;
        let mut cw = vec![0u8; fb.len() * BYTES_PER_PIXEL];
        let mut ccw = vec![0u8; fb.len() * BYTES_PER_PIXEL];
        write_rgba_rotated_cw(&fb, &mut cw);
        write_rgba_rotated_ccw(&fb, &mut ccw);
        assert_ne!(cw, ccw);
    }

    #[test]
    #[should_panic(expected = "full")]
    fn write_rgba_rotated_ccw_panics_on_wrong_size() {
        write_rgba_rotated_ccw(&[0u16; 4], &mut [0u8; 16]);
    }
}
