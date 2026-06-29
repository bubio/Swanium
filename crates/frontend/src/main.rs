//! Swanium frontend: a minimal Slint window that plays a WonderSwan ROM.
//!
//! Scope (see `docs/dev/DevelopmentPlan.md` Phase 7): open a window, run the
//! core one frame at a time, show the framebuffer, and accept keyboard input.
//! Audio output (cpal), gamepad input (gilrs), an in-app file picker, and a
//! settings UI remain follow-ups; the ROM path is given on the command line.
//!
//! Debug: pressing `P` prints the current frame's display registers and a
//! coarse per-layer map to stderr — see [`dump_display_registers`], used to
//! diagnose the PPU background-update issue noted in the development plan.

mod keymap;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::error::Error;
use std::rc::Rc;
use std::time::Duration;

use input::Button;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode};
use swanium_core::keypad::KeyState;
use swanium_core::system::System;

/// Polling interval for the frame-pacing timer.
///
/// We poll at ~4 ms rather than the WS frame period (~13.25 ms) so that the
/// audio ring-buffer fill level — not a fixed wall-clock timer — governs when
/// the next emulated frame runs.  This decouples execution rate from OS
/// scheduler jitter and drives the emulator at exactly the audio device's
/// clock, eliminating tempo drift and reducing underrun stuttering.
const POLL_INTERVAL: Duration = Duration::from_millis(4);

/// Ring-buffer fill fraction above which we hold off running another frame.
///
/// 3/4 of 16 384 samples ≈ 128 ms of audio at 48 kHz — enough headroom to
/// absorb OS scheduling spikes while keeping audio latency acceptable.
const FILL_HOLD_NUM: usize = 3;
const FILL_HOLD_DEN: usize = 4;

slint::slint! {
    export component MainWindow inherits Window {
        in property <image> frame;
        in property <length> view-width;
        in property <length> view-height;
        callback key-event(string, bool);

        title: "Swanium";
        width: root.view-width;
        height: root.view-height;
        forward-focus: scope;

        Image {
            width: 100%;
            height: 100%;
            source: root.frame;
            image-rendering: pixelated;
        }

        scope := FocusScope {
            key-pressed(event) => {
                root.key-event(event.text, true);
                accept
            }
            key-released(event) => {
                root.key-event(event.text, false);
                reject
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    common::logging::init();

    let Some(rom_path) = std::env::args().nth(1) else {
        eprintln!("usage: frontend <rom.ws>");
        return Err("no ROM path given".into());
    };

    let rom = std::fs::read(&rom_path)?;
    tracing::info!(rom = %rom_path, bytes = rom.len(), "loaded ROM");

    run(rom)
}

fn run(rom: Vec<u8>) -> Result<(), Box<dyn Error>> {
    // Load persisted settings; first run falls back to defaults. Write the
    // file back so it exists on disk for the user to edit (and is created if
    // missing). A failure to persist is non-fatal — we just log it.
    let config = common::config::Config::load();
    if let Err(e) = config.save() {
        tracing::warn!("could not save config: {e}");
    }
    let scale = config.scale;
    let width = video::SCREEN_WIDTH as u32;
    let height = video::SCREEN_HEIGHT as u32;

    let window = MainWindow::new()?;
    window.set_view_width((width * scale) as f32);
    window.set_view_height((height * scale) as f32);

    // Keys currently held down, updated from Slint key events.
    let pressed: Rc<RefCell<HashSet<Button>>> = Rc::new(RefCell::new(HashSet::new()));
    // Set when the user presses `P`: dump the next frame's display registers.
    let dump_request = Rc::new(Cell::new(false));
    window.on_key_event({
        let pressed = pressed.clone();
        let dump_request = dump_request.clone();
        move |text, is_down| {
            if is_down && (text == "p" || text == "P") {
                dump_request.set(true);
            }
            if let Some(button) = keymap::button_from_text(&text) {
                if is_down {
                    pressed.borrow_mut().insert(button);
                } else {
                    pressed.borrow_mut().remove(&button);
                }
            }
        }
    });

    let system = Rc::new(RefCell::new(System::from_rom(rom)));

    // Open the cpal output stream.  Failure (e.g. headless CI) is non-fatal:
    // the emulator runs silently and APU samples are discarded each frame.
    let mut audio_stream = audio::AudioStream::open()
        .map_err(|e| tracing::warn!("audio unavailable: {e}"))
        .ok();

    // Pace emulated frames via audio ring-buffer fill level.
    //
    // The timer fires every 4 ms; we only run an emulated frame when the ring
    // buffer has dropped below 3/4 capacity (≈128 ms at 48 kHz).  This ties
    // the emulator's speed to the audio device clock rather than a wall-clock
    // timer, keeping tempo accurate and minimising underrun stuttering.
    // If no audio device is open we fall back to always running (timer-driven).
    let timer = Timer::default();
    let window_weak = window.as_weak();
    timer.start(TimerMode::Repeated, POLL_INTERVAL, move || {
        let dump = dump_request.take();

        let should_run = dump
            || audio_stream
                .as_ref()
                .map(|a| a.ring_fill() * FILL_HOLD_DEN < a.ring_capacity() * FILL_HOLD_NUM)
                .unwrap_or(true);

        if !should_run {
            return;
        }

        let keys = input::keys_from(pressed.borrow().iter().copied());
        let mut system = system.borrow_mut();
        if dump {
            dump_display_registers(&mut system, keys);
        } else {
            system.run_frame(keys);
        }
        if let Some(ref mut audio) = audio_stream {
            audio.push(system.audio_samples());
        }
        system.clear_audio_samples();

        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        video::write_rgba(system.framebuffer(), buffer.make_mut_bytes());
        if let Some(window) = window_weak.upgrade() {
            window.set_frame(Image::from_rgba8(buffer));
        }
    });

    window.run()?;
    Ok(())
}

/// Run one frame with per-scanline tracing and print a compact report of the
/// display registers, highlighting where they change down the frame (the
/// signature of a raster split). Triggered by the `P` key.
fn dump_display_registers(system: &mut System, keys: KeyState) {
    let trace = system.run_frame_traced(keys);
    let bus = system.bus_mut();
    eprintln!("── display register dump ──────────────────────────────");
    eprintln!(
        "disp_ctrl=0b{:08b} int_enable=0b{:08b} hbl_ctrl=0b{:08b}",
        bus.peek_io(0x00),
        bus.peek_io(0xB2),
        bus.peek_io(0xA2),
    );
    eprintln!(
        "scr2_window (x1,y1,x2,y2)=({},{},{},{})",
        bus.peek_io(0x08),
        bus.peek_io(0x09),
        bus.peek_io(0x0A),
        bus.peek_io(0x0B),
    );
    eprintln!("per-scanline changes (line: disp_ctrl scr1_y scr2_y line_cmp):");
    let mut prev: Option<swanium_core::system::ScanlineTrace> = None;
    for t in &trace {
        let changed = prev.is_none_or(|p| {
            p.disp_ctrl != t.disp_ctrl
                || p.scr1_scroll_y != t.scr1_scroll_y
                || p.scr2_scroll_y != t.scr2_scroll_y
                || p.line_compare != t.line_compare
        });
        if changed {
            eprintln!(
                "  {:3}: 0b{:08b} {:3} {:3} {:3}",
                t.line, t.disp_ctrl, t.scr1_scroll_y, t.scr2_scroll_y, t.line_compare
            );
        }
        prev = Some(*t);
    }
    let bus = system.bus_mut();
    eprintln!(
        "map_base=0x{:02X} scroll scr1=({},{}) scr2=({},{})",
        bus.peek_io(0x07),
        bus.peek_io(0x10),
        bus.peek_io(0x11),
        bus.peek_io(0x12),
        bus.peek_io(0x13),
    );
    // Each layer separately (X = non-zero pixel, . = pixel 0), 8x8 cells.
    for (label, scr2) in [("SCR1 (back)", false), ("SCR2 (front)", true)] {
        eprintln!("{label}:");
        for y in (0..video::SCREEN_HEIGHT as u8).step_by(8) {
            let mut row = String::new();
            for x in (0..video::SCREEN_WIDTH).step_by(8) {
                let (px, _) = bus.debug_bg_sample(scr2, x, y);
                row.push(if px != 0 { 'X' } else { '.' });
            }
            eprintln!("  y={y:3} {row}");
        }
    }
    eprintln!("───────────────────────────────────────────────────────");
}
