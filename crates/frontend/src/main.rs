//! Swanium frontend: a minimal Slint window that plays a WonderSwan ROM.
//!
//! Scope (see `docs/dev/DevelopmentPlan.md` Phase 7): open a window, run the
//! core one frame at a time, show the framebuffer, and accept keyboard input.
//! Audio output (cpal), gamepad input (gilrs), an in-app file picker, and a
//! settings UI remain follow-ups; the ROM path is given on the command line.

mod keymap;

use std::cell::RefCell;
use std::collections::HashSet;
use std::error::Error;
use std::rc::Rc;
use std::time::Duration;

use input::Button;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode};
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
    window.on_key_event({
        let pressed = pressed.clone();
        move |text, is_down| {
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
        system.run_frame(keys);
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
