//! Swanium frontend: a minimal Slint window that plays a WonderSwan ROM.
//!
//! Scope (see `docs/dev/DevelopmentPlan.md` Phase 7): open a window, run the
//! core one frame at a time, show the framebuffer, play audio (cpal), and
//! accept keyboard and gamepad input. A ROM can be supplied on the command
//! line or opened in-app with the `O` key (native dialog via [`rfd`]). A
//! settings/key-binding UI remains a follow-up.
//!
//! Debug: pressing `P` prints the current frame's display registers and a
//! coarse per-layer map to stderr — see [`dump_display_registers`], used to
//! diagnose the PPU background-update issue noted in the development plan.

mod keymap;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

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

/// Height of the bottom status bar, in logical pixels.
///
/// Added on top of the scaled framebuffer so the screen area keeps its exact
/// integer scale. This is the single source of truth: it is also fed to the
/// Slint markup via the `status-bar-height` property so the two never drift.
const STATUS_BAR_HEIGHT: f32 = 22.0;

/// How often the status bar's FPS readout is refreshed.
const FPS_REFRESH: Duration = Duration::from_millis(500);

slint::slint! {
    export component MainWindow inherits Window {
        in property <image> frame;
        in property <length> view-width;
        in property <length> view-height;
        in property <length> status-bar-height;
        in property <bool> has-rom;
        in property <string> window-title: "Swanium";
        in property <string> status-text: "No ROM loaded — press O or use File ▸ Open ROM";
        callback key-event(string, bool);
        callback open-rom();
        callback quit();

        title: root.window-title;
        width: root.view-width;
        height: root.view-height;
        forward-focus: scope;

        // Native menu bar (macOS system bar; in-window on Windows/Linux),
        // wired to the same actions as the keyboard shortcuts.
        MenuBar {
            Menu {
                title: "File";
                MenuItem {
                    title: "Open ROM…";
                    activated => { root.open-rom(); }
                }
                MenuItem {
                    title: "Quit";
                    activated => { root.quit(); }
                }
            }
        }

        VerticalLayout {
            // Screen area — stretches to fill everything above the status bar.
            Rectangle {
                background: #000000;
                Image {
                    width: 100%;
                    height: 100%;
                    source: root.frame;
                    image-rendering: pixelated;
                }
                // Shown until a ROM is loaded: the window opens empty so the
                // user can pick a file in-app rather than relaunching a shell.
                if !root.has-rom: Text {
                    text: "Press O to open a ROM";
                    color: #cccccc;
                    font-size: 14px;
                    horizontal-alignment: center;
                    vertical-alignment: center;
                }
            }
            // Status bar: fixed-height strip showing the ROM name and FPS.
            Rectangle {
                height: root.status-bar-height;
                background: #1e1e1e;
                HorizontalLayout {
                    padding-left: 8px;
                    padding-right: 8px;
                    Text {
                        text: root.status-text;
                        color: #cccccc;
                        font-size: 12px;
                        vertical-alignment: center;
                        horizontal-alignment: left;
                    }
                }
            }
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

    // The ROM path is now optional: with no argument the window still opens and
    // the user picks a file in-app (the `O` key). A given path is loaded eagerly.
    let initial = std::env::args().nth(1).map(PathBuf::from);
    run(initial)
}

fn run(initial: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
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
    window.set_view_height((height * scale) as f32 + STATUS_BAR_HEIGHT);
    window.set_status_bar_height(STATUS_BAR_HEIGHT);

    // Keys currently held down, updated from Slint key events.
    let pressed: Rc<RefCell<HashSet<Button>>> = Rc::new(RefCell::new(HashSet::new()));
    // Set when the user presses `P`: dump the next frame's display registers.
    let dump_request = Rc::new(Cell::new(false));
    // Set when the user presses `O` or picks File ▸ Open ROM: open the picker
    // on the next tick (deferring keeps it off the Slint event-dispatch path).
    let open_request = Rc::new(Cell::new(false));
    window.on_key_event({
        let pressed = pressed.clone();
        let dump_request = dump_request.clone();
        let open_request = open_request.clone();
        move |text, is_down| {
            if is_down && (text == "p" || text == "P") {
                dump_request.set(true);
            }
            if is_down && (text == "o" || text == "O") {
                open_request.set(true);
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
    window.on_open_rom({
        let open_request = open_request.clone();
        move || open_request.set(true)
    });
    window.on_quit(|| {
        let _ = slint::quit_event_loop();
    });

    // The running machine, `None` until a ROM is loaded. `last_dir` seeds the
    // file picker's starting directory with wherever the last ROM came from;
    // `rom_label` holds the current ROM's name for the status bar.
    let system: Rc<RefCell<Option<System>>> = Rc::new(RefCell::new(None));
    let last_dir: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let rom_label: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    // Load the ROM given on the command line, if any. A failure here is
    // non-fatal: the window still opens so the user can pick another file.
    if let Some(path) = initial {
        load_into(&path, &system, &last_dir, &rom_label, &window);
    }

    // Open the gilrs gamepad backend.  Failure (no gamepad subsystem, headless
    // CI) is non-fatal: we fall back to keyboard input alone.
    let mut gamepad = input::gamepad::Gamepad::open()
        .inspect_err(|e| tracing::warn!("gamepad unavailable: {e}"))
        .ok();

    // Open the cpal output stream.  Failure (e.g. headless CI) is non-fatal:
    // the emulator runs silently and APU samples are discarded each frame.
    let mut audio_stream = audio::AudioStream::open()
        .inspect_err(|e| tracing::warn!("audio unavailable: {e}"))
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
    // FPS accounting for the status bar: frames since the last refresh and the
    // wall-clock anchor we measure against.
    let mut frames_since_refresh: u32 = 0;
    let mut fps_anchor = Instant::now();
    timer.start(TimerMode::Repeated, POLL_INTERVAL, move || {
        // Handle an in-app open request first. The native dialog runs a modal
        // loop that blocks this timer, so emulation pauses while it is open —
        // the desired behaviour. Skip running a frame on the tick we opened it.
        if open_request.take() {
            let start = last_dir.borrow().clone();
            if let Some(path) = pick_rom(start.as_deref()) {
                if let Some(window) = window_weak.upgrade() {
                    load_into(&path, &system, &last_dir, &rom_label, &window);
                }
                // Drop the previous game's queued audio so it doesn't bleed in.
                if let Some(ref audio) = audio_stream {
                    audio.clear();
                }
                // Restart FPS accounting so the readout isn't skewed by the
                // time the modal dialog was open.
                frames_since_refresh = 0;
                fps_anchor = Instant::now();
            }
            return;
        }

        let dump = dump_request.take();

        let should_run = dump
            || audio_stream
                .as_ref()
                .is_none_or(|a| a.ring_fill() * FILL_HOLD_DEN < a.ring_capacity() * FILL_HOLD_NUM);

        if !should_run {
            return;
        }

        let mut system_ref = system.borrow_mut();
        let Some(system) = system_ref.as_mut() else {
            return; // no ROM loaded yet — the placeholder overlay is shown
        };

        let mut keys = input::keys_from(pressed.borrow().iter().copied());
        if let Some(ref mut gamepad) = gamepad {
            keys |= gamepad.poll();
        }
        if dump {
            dump_display_registers(system, keys);
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

        // Refresh the status bar's FPS readout at a fixed cadence so the number
        // is readable rather than flickering every frame.
        frames_since_refresh += 1;
        let now = Instant::now();
        let elapsed = now.duration_since(fps_anchor);
        if elapsed >= FPS_REFRESH {
            let fps = frames_since_refresh as f32 / elapsed.as_secs_f32();
            frames_since_refresh = 0;
            fps_anchor = now;
            if let Some(window) = window_weak.upgrade() {
                window.set_status_text(format!("{} — {fps:.0} fps", rom_label.borrow()).into());
            }
        }
    });

    window.run()?;
    Ok(())
}

/// Pop the native "open ROM" dialog, returning the chosen path (if any).
///
/// `start_dir`, when present, seeds the dialog's initial directory so reopening
/// lands where the last ROM was picked rather than the process's cwd.
fn pick_rom(start_dir: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Open ROM")
        .add_filter("WonderSwan ROM", &["ws", "wsc"]);
    if let Some(dir) = start_dir {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_file()
}

/// Read `path`, build a fresh [`System`] from it, and update the window.
///
/// A read failure is non-fatal: it is logged and the current machine (if any)
/// keeps running, leaving the window state unchanged.
fn load_into(
    path: &Path,
    system: &RefCell<Option<System>>,
    last_dir: &RefCell<Option<PathBuf>>,
    rom_label: &RefCell<String>,
    window: &MainWindow,
) {
    match std::fs::read(path) {
        Ok(bytes) => {
            tracing::info!(rom = %path.display(), bytes = bytes.len(), "loaded ROM");
            *system.borrow_mut() = Some(System::from_rom(bytes));
            *last_dir.borrow_mut() = path.parent().map(Path::to_path_buf);
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("ROM");
            *rom_label.borrow_mut() = name.to_string();
            window.set_window_title(format!("Swanium — {name}").into());
            window.set_status_text(format!("{name} — running").into());
            window.set_has_rom(true);
        }
        Err(e) => tracing::error!(rom = %path.display(), "could not load ROM: {e}"),
    }
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
