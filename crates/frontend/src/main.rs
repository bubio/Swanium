//! Swanium frontend.
//!
//! The interactive Slint window, wgpu surface, cpal output stream, and
//! gilrs/keyboard input are wired in the GUI step (see
//! `docs/dev/DevelopmentPlan.md` Phase 7 後続課題). Until then this binary
//! exercises the full data path headlessly: it loads a ROM, drives the core
//! one frame at a time, and runs each produced frame through the `video`,
//! `audio`, and `input` adapter crates. This keeps the wiring honest and gives
//! CI a "boots and runs without crashing" smoke check.

use std::path::Path;
use std::process::ExitCode;

use audio::RingBuffer;
use input::Button;
use swanium_core::system::System;

/// Number of frames the headless run advances before exiting.
const HEADLESS_FRAMES: u32 = 600;

/// Audio ring-buffer capacity (a few frames of stereo samples is plenty).
const AUDIO_BUFFER_SAMPLES: usize = 8192;

fn main() -> ExitCode {
    common::logging::init();

    let Some(rom_path) = std::env::args().nth(1) else {
        eprintln!("usage: frontend <rom.ws>");
        eprintln!("(headless runner; the Slint GUI is wired in a later step)");
        return ExitCode::FAILURE;
    };

    match run_headless(rom_path.as_ref(), HEADLESS_FRAMES) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Load `rom_path`, run `frames` frames, and push each frame through the
/// adapter crates. Returns once the run completes.
fn run_headless(rom_path: &Path, frames: u32) -> std::io::Result<()> {
    let rom = std::fs::read(rom_path)?;
    tracing::info!(rom = %rom_path.display(), bytes = rom.len(), "loaded ROM");

    let mut system = System::from_rom(rom);
    let mut audio_buffer = RingBuffer::new(AUDIO_BUFFER_SAMPLES);
    let mut frame_rgba =
        vec![0u8; video::SCREEN_WIDTH * video::SCREEN_HEIGHT * video::BYTES_PER_PIXEL];

    // No host input in the headless runner; fold an empty button set through
    // the input adapter to keep the keyboard/gamepad path wired.
    let keys = input::keys_from(Vec::<Button>::new());

    for _ in 0..frames {
        system.run_frame(keys);

        // Video: framebuffer shade indices -> RGBA8 (what the GPU would upload).
        video::write_rgba(system.framebuffer(), &mut frame_rgba);

        // Audio: drain the core's samples into the output ring buffer.
        audio_buffer.push(system.audio_samples());
        system.clear_audio_samples();
    }

    tracing::info!(frames, "headless run complete");
    Ok(())
}
