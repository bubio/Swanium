//! Conversion of the emulator core's framebuffer into an RGBA8 image.
//!
//! The core produces a `224 × 144` buffer of monochrome *shade indices*
//! (`0`–`15`, see [`swanium_core::system::System::framebuffer`]). This crate
//! turns those indices into packed `RGBA8888` pixels the GPU can upload as a
//! texture. The actual wgpu surface, swap chain, and scaling pipeline are wired
//! in a later step (see `docs/dev/DevelopmentPlan.md` Phase 7 後続課題); keeping
//! the pixel conversion here as a pure, testable function lets the rest of the
//! frontend and CI exercise the data path without a GPU.

/// Visible screen width in pixels.
pub const SCREEN_WIDTH: usize = 224;

/// Visible screen height in pixels.
pub const SCREEN_HEIGHT: usize = 144;

/// Number of distinct monochrome shades the core emits (`0`–`15`).
pub const SHADE_LEVELS: u8 = 16;

/// Bytes per RGBA8 pixel.
pub const BYTES_PER_PIXEL: usize = 4;

/// Map a monochrome shade index (`0` = lightest, `15` = darkest) to an opaque
/// RGBA8 grey. Indices above the maximum are clamped to the darkest shade.
///
/// Shade `0` is white (`0xFF`) and shade `15` is black (`0x00`); intermediate
/// shades are spread evenly (`255 − shade × 17`, since `15 × 17 = 255`).
pub fn shade_to_rgba(shade: u8) -> [u8; 4] {
    let clamped = shade.min(SHADE_LEVELS - 1);
    let grey = 255 - clamped * 17;
    [grey, grey, grey, 0xFF]
}

/// Convert a framebuffer of shade indices into a freshly allocated RGBA8 buffer.
///
/// The returned vector has `framebuffer.len() * 4` bytes. Prefer
/// [`write_rgba`] in a hot loop to reuse an existing allocation.
pub fn framebuffer_to_rgba(framebuffer: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; framebuffer.len() * BYTES_PER_PIXEL];
    write_rgba(framebuffer, &mut out);
    out
}

/// Convert a framebuffer of shade indices into `out`, reusing its allocation.
///
/// # Panics
///
/// Panics if `out` is shorter than `framebuffer.len() * 4`.
pub fn write_rgba(framebuffer: &[u8], out: &mut [u8]) {
    assert!(
        out.len() >= framebuffer.len() * BYTES_PER_PIXEL,
        "output buffer too small: {} < {}",
        out.len(),
        framebuffer.len() * BYTES_PER_PIXEL
    );
    for (shade, pixel) in framebuffer
        .iter()
        .zip(out.chunks_exact_mut(BYTES_PER_PIXEL))
    {
        pixel.copy_from_slice(&shade_to_rgba(*shade));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shade_zero_is_white() {
        assert_eq!(shade_to_rgba(0), [0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn shade_max_is_black() {
        assert_eq!(shade_to_rgba(15), [0x00, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn shade_is_always_opaque() {
        assert_eq!(shade_to_rgba(7)[3], 0xFF);
    }

    #[test]
    fn shade_above_max_clamps_to_black() {
        assert_eq!(shade_to_rgba(99), [0x00, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn shade_is_monotonically_darker() {
        assert!(shade_to_rgba(3)[0] > shade_to_rgba(8)[0]);
    }

    #[test]
    fn framebuffer_to_rgba_has_four_bytes_per_pixel() {
        let fb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT];
        assert_eq!(
            framebuffer_to_rgba(&fb).len(),
            SCREEN_WIDTH * SCREEN_HEIGHT * 4
        );
    }

    #[test]
    fn framebuffer_to_rgba_converts_first_pixel() {
        let mut fb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT];
        fb[0] = 15;
        assert_eq!(&framebuffer_to_rgba(&fb)[0..4], &[0x00, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn write_rgba_reuses_buffer() {
        let fb = [0u8, 15];
        let mut out = vec![0u8; 8];
        write_rgba(&fb, &mut out);
        assert_eq!(out, vec![0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0xFF]);
    }

    #[test]
    #[should_panic(expected = "output buffer too small")]
    fn write_rgba_panics_on_small_buffer() {
        write_rgba(&[0u8; 4], &mut [0u8; 4]);
    }
}
