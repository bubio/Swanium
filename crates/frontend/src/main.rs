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

/// WonderSwan refresh interval (~75.47 Hz → one frame every ~13.25 ms).
const FRAME_INTERVAL: Duration = Duration::from_micros(13_250);

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
    let scale = common::config::Config::default().sanitised().scale;
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

    // Drive one frame per tick and upload the result as the window's image.
    let timer = Timer::default();
    let window_weak = window.as_weak();
    timer.start(TimerMode::Repeated, FRAME_INTERVAL, move || {
        let keys = input::keys_from(pressed.borrow().iter().copied());

        let mut system = system.borrow_mut();
        if dump_request.take() {
            dump_display_registers(&mut system, keys);
        } else {
            system.run_frame(keys);
        }
        // Audio is not yet routed to an output device; drop the samples so the
        // APU buffer does not grow without bound.
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
