//! Swanium frontend: a Slint window that plays a WonderSwan ROM.
//!
//! Scope (see `docs/dev/DevelopmentPlan.md` Phase 7): open a window, run the
//! core one frame at a time, show the framebuffer, play audio (cpal), and
//! accept keyboard and gamepad input. The menu bar exposes ROM history, window
//! scale, fullscreen, vertical rotation, renderer choice, a settings window,
//! and a non-macOS About window. Settings persist to `config.toml`.
//!
//! The Slint markup lives in `ui/*.slint` and is compiled by `build.rs`;
//! [`slint::include_modules!`] brings the generated `MainWindow`,
//! `SettingsWindow`, `AboutWindow`, `BindingRow`, and `Renderer` into scope.
//!
//! Debug: pressing `P` prints the current frame's display registers and a
//! coarse per-layer map to stderr — see [`dump_display_registers`].

mod keymap;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::config::{BiosRomKind, Config, RendererKind, RotationKind};
use input::Button;
use keymap::Keymap;
use slint::{
    ComponentHandle, Image, LogicalSize, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString,
    Timer, TimerMode, VecModel,
};
use swanium_core::keypad::KeyState;
use swanium_core::system::System;
use swanium_core::HardwareModel;

slint::include_modules!();

/// Polling interval for the frame-pacing timer.
///
/// We poll at ~4 ms rather than the WS frame period (~13.25 ms) so that the
/// audio ring-buffer fill level — not a fixed wall-clock timer — governs when
/// the next emulated frame runs.
const POLL_INTERVAL: Duration = Duration::from_millis(4);

/// Ring-buffer fill fraction above which we hold off running another frame.
const FILL_HOLD_NUM: usize = 3;
const FILL_HOLD_DEN: usize = 4;

/// Height of the bottom status bar, in logical pixels.
const STATUS_BAR_HEIGHT: f32 = 22.0;

/// How often the status bar's FPS readout is refreshed.
const FPS_REFRESH: Duration = Duration::from_millis(500);
const BIOS_SETTINGS_START_FRAMES: u8 = 12;

/// Which input device an in-progress rebind is capturing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Device {
    Keyboard,
    Gamepad,
}

/// Shared, interior-mutable state threaded through the Slint callbacks.
///
/// Grouping the handles keeps the many closures below to a single clone each
/// rather than a fistful of individually-cloned `Rc`s.
#[derive(Clone)]
struct App {
    config: Rc<RefCell<Config>>,
    system: Rc<RefCell<Option<System>>>,
    last_dir: Rc<RefCell<Option<PathBuf>>>,
    rom_path: Rc<RefCell<Option<PathBuf>>>,
    rom_label: Rc<RefCell<String>>,
    pressed: Rc<RefCell<HashSet<Button>>>,
    keymap: Rc<RefCell<Keymap>>,
    gamepad: Rc<RefCell<Option<input::gamepad::Gamepad>>>,
    /// The input the settings UI is waiting to (re)bind, if any.
    capture: Rc<Cell<Option<(Device, Button)>>>,
    /// Target of a fullscreen change we requested, until the OS transition
    /// settles. Guards the per-tick sync from fighting our own toggle while
    /// macOS animates; `None` means the OS state is the source of truth (so an
    /// external change via the title-bar button is adopted).
    pending_fullscreen: Rc<Cell<Option<bool>>>,
    /// Master volume, 0–100, read by the frame timer and pushed to the audio
    /// stream each frame. Persisted to config on change.
    volume: Rc<Cell<u8>>,
    /// Whether emulation is paused. Runtime-only — never persisted.
    paused: Rc<Cell<bool>>,
    dump_request: Rc<Cell<bool>>,
    pending_open_path: Rc<RefCell<Option<PathBuf>>>,
    /// True while the native file dialog is running its modal event loop.
    open_dialog_active: Rc<Cell<bool>>,
    reset_request: Rc<Cell<bool>>,
    bios_settings_start_frames: Rc<Cell<u8>>,
    settings: Rc<RefCell<Option<SettingsWindow>>>,
    about: Rc<RefCell<Option<AboutWindow>>>,
}

fn main() -> Result<(), Box<dyn Error>> {
    common::logging::init();
    let initial = std::env::args().nth(1).map(PathBuf::from);
    run(initial)
}

fn run(initial: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    let mut config = Config::load();
    // First run: seed the binding tables from the built-in defaults so the
    // config file is complete and remains the single source of truth.
    if config.keyboard_bindings.is_empty() {
        config.keyboard_bindings = Keymap::defaults()
            .to_config()
            .into_iter()
            .collect::<BTreeMap<_, _>>();
    }
    if config.gamepad_bindings.is_empty() {
        config.gamepad_bindings = default_gamepad_config();
    }
    if let Err(e) = config.save() {
        tracing::warn!("could not save config: {e}");
    }

    let keymap = keymap_from_config(&config);
    let initial_volume = config.volume.min(100);

    let window = MainWindow::new()?;

    let app = App {
        config: Rc::new(RefCell::new(config)),
        system: Rc::new(RefCell::new(None)),
        last_dir: Rc::new(RefCell::new(None)),
        rom_path: Rc::new(RefCell::new(None)),
        rom_label: Rc::new(RefCell::new(String::new())),
        pressed: Rc::new(RefCell::new(HashSet::new())),
        keymap: Rc::new(RefCell::new(keymap)),
        gamepad: Rc::new(RefCell::new(None)),
        capture: Rc::new(Cell::new(None)),
        pending_fullscreen: Rc::new(Cell::new(None)),
        volume: Rc::new(Cell::new(initial_volume)),
        paused: Rc::new(Cell::new(false)),
        dump_request: Rc::new(Cell::new(false)),
        pending_open_path: Rc::new(RefCell::new(None)),
        open_dialog_active: Rc::new(Cell::new(false)),
        reset_request: Rc::new(Cell::new(false)),
        bios_settings_start_frames: Rc::new(Cell::new(0)),
        settings: Rc::new(RefCell::new(None)),
        about: Rc::new(RefCell::new(None)),
    };

    window.set_status_bar_height(STATUS_BAR_HEIGHT);
    window.set_use_native_about(cfg!(target_os = "macos"));
    apply_view(&window, &app.config.borrow());
    window.set_recent_files(recent_model(&app.config.borrow()));

    wire_input(&window, &app);
    wire_menu(&window, &app);

    // Load the ROM given on the command line, if any (non-fatal on error).
    if let Some(path) = initial {
        load_into(&path, &app, &window);
    }

    // Open the gilrs gamepad backend (non-fatal on failure) and apply bindings.
    match input::gamepad::Gamepad::open() {
        Ok(mut gp) => {
            gp.set_named_bindings(named_pairs(&app.config.borrow().gamepad_bindings));
            *app.gamepad.borrow_mut() = Some(gp);
        }
        Err(e) => tracing::warn!("gamepad unavailable: {e}"),
    }

    // Open the cpal output stream (non-fatal on failure).
    let mut audio_stream = audio::AudioStream::open()
        .inspect_err(|e| tracing::warn!("audio unavailable: {e}"))
        .ok();

    let width = video::SCREEN_WIDTH as u32;
    let height = video::SCREEN_HEIGHT as u32;

    let timer = Timer::default();
    let window_weak = window.as_weak();
    let app_timer = app.clone();
    let mut frames_since_refresh: u32 = 0;
    let mut fps_anchor = Instant::now();
    timer.start(TimerMode::Repeated, POLL_INTERVAL, move || {
        let app = &app_timer;

        // Keep the menu's Fullscreen check in sync with the real window state,
        // which the user can change from the macOS title-bar button.
        if let Some(window) = window_weak.upgrade() {
            sync_fullscreen(app, &window);
        }

        // Some native file dialogs run a nested event loop. If this timer is
        // re-entered while the dialog is open, keep emulation/audio/input idle
        // until the original dialog call returns.
        if app.open_dialog_active.get() {
            return;
        }

        // File dialogs are opened from the menu callback, not from this timer:
        // on macOS, NSOpenPanel runs a nested event loop and Slint panics if a
        // timer callback re-enters timer activation. The timer only consumes
        // the selected path after the dialog has closed.
        if let Some(path) = app.pending_open_path.borrow_mut().take() {
            if let Some(window) = window_weak.upgrade() {
                load_into(&path, app, &window);
            }
            if let Some(ref audio) = audio_stream {
                audio.clear();
            }
            frames_since_refresh = 0;
            fps_anchor = Instant::now();
            return;
        }
        if app.reset_request.take() {
            app.pressed.borrow_mut().clear();
            if let Some(window) = window_weak.upgrade() {
                reset_emulation(app, &window);
            }
            if let Some(ref audio) = audio_stream {
                audio.clear();
            }
            frames_since_refresh = 0;
            fps_anchor = Instant::now();
            return;
        }

        // While a controller rebind is armed, poll for the captured button and
        // do not run a frame (so the press never reaches the game).
        if let Some((Device::Gamepad, ws_button)) = app.capture.get() {
            if let Some(name) = app
                .gamepad
                .borrow_mut()
                .as_mut()
                .and_then(|gp| gp.poll_capture())
            {
                assign_gamepad(app, ws_button, name);
                if let Some(window) = window_weak.upgrade() {
                    refresh_settings_rows(app, &window);
                }
            }
            return;
        }

        let dump = app.dump_request.take();
        // Paused: do not advance the machine (a manual `P` dump still runs). The
        // audio ring simply drains to silence while nothing is pushed.
        if app.paused.get() && !dump {
            return;
        }
        let should_run = dump
            || audio_stream
                .as_ref()
                .is_none_or(|a| a.ring_fill() * FILL_HOLD_DEN < a.ring_capacity() * FILL_HOLD_NUM);
        if !should_run {
            return;
        }

        let mut system_ref = app.system.borrow_mut();
        let Some(system) = system_ref.as_mut() else {
            return; // no ROM loaded yet — the placeholder overlay is shown
        };

        let mut keys = input::keys_from(app.pressed.borrow().iter().copied());
        if let Some(gp) = app.gamepad.borrow_mut().as_mut() {
            keys |= gp.poll();
        }
        let start_frames = app.bios_settings_start_frames.get();
        if start_frames > 0 {
            keys |= KeyState::START;
            app.bios_settings_start_frames
                .set(start_frames.saturating_sub(1));
        }
        let run_result = catch_unwind(AssertUnwindSafe(|| {
            if dump {
                dump_display_registers(system, keys);
            } else {
                system.run_frame(keys);
            }
        }));
        if let Err(payload) = run_result {
            let reason = panic_payload_message(payload.as_ref());
            let context = cpu_context(system);
            tracing::error!("emulation stopped after core panic: {reason}");
            app.paused.set(true);
            if let Some(window) = window_weak.upgrade() {
                window.set_paused(true);
                window.set_status_text(
                    format!(
                        "{} — stopped: {reason} at {context}",
                        app.rom_label.borrow()
                    )
                    .into(),
                );
            }
            return;
        }
        if let Some(fault) = system.cpu().fault {
            let context = cpu_context(system);
            tracing::error!(
                opcode = format_args!("0x{:02X}", fault.opcode),
                cs = format_args!("0x{:04X}", fault.cs),
                ip = format_args!("0x{:04X}", fault.ip),
                "emulation stopped on unsupported CPU opcode"
            );
            app.paused.set(true);
            if let Some(window) = window_weak.upgrade() {
                window.set_paused(true);
                window.set_status_text(
                    format!(
                        "{} — stopped: unsupported opcode 0x{:02X} at {context}",
                        app.rom_label.borrow(),
                        fault.opcode
                    )
                    .into(),
                );
            }
            return;
        }
        if let Some(ref mut audio) = audio_stream {
            audio.set_volume(app.volume.get());
            audio.push(system.audio_samples());
        }
        system.clear_audio_samples();

        let rotation = app.config.borrow().rotation;
        let (bw, bh) = if rotation.is_rotated() {
            (height, width)
        } else {
            (width, height)
        };
        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(bw, bh);
        let fb = system.framebuffer();
        match rotation {
            RotationKind::None => video::write_rgba(fb, buffer.make_mut_bytes()),
            RotationKind::Right => video::write_rgba_rotated_cw(fb, buffer.make_mut_bytes()),
            RotationKind::Left => video::write_rgba_rotated_ccw(fb, buffer.make_mut_bytes()),
        }
        if let Some(window) = window_weak.upgrade() {
            window.set_frame(Image::from_rgba8(buffer));
        }

        frames_since_refresh += 1;
        let now = Instant::now();
        let elapsed = now.duration_since(fps_anchor);
        if elapsed >= FPS_REFRESH {
            let fps = frames_since_refresh as f32 / elapsed.as_secs_f32();
            frames_since_refresh = 0;
            fps_anchor = now;
            if let Some(window) = window_weak.upgrade() {
                window.set_status_text(format!("{} — {fps:.0} fps", app.rom_label.borrow()).into());
            }
        }
    });

    window.run()?;
    Ok(())
}

/// Wire the game-input focus scope and quit callback.
fn wire_input(window: &MainWindow, app: &App) {
    window.on_key_event({
        let app = app.clone();
        move |text, is_down| {
            // `text` is a `SharedString` that derefs to `str`; use it directly
            // rather than allocating a `String` on every key event.
            if is_down && text.eq_ignore_ascii_case("p") {
                app.dump_request.set(true);
            }
            if let Some(button) = app.keymap.borrow().resolve(&text) {
                if is_down {
                    app.pressed.borrow_mut().insert(button);
                } else {
                    app.pressed.borrow_mut().remove(&button);
                }
            }
        }
    });
    window.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
}

/// Wire the File/View/Emulation menu callbacks.
fn wire_menu(window: &MainWindow, app: &App) {
    window.on_open_rom({
        let app = app.clone();
        move || {
            if app.open_dialog_active.replace(true) {
                return;
            }
            app.pressed.borrow_mut().clear();
            let start = app.last_dir.borrow().clone();
            let picked = pick_rom(start.as_deref());
            *app.pending_open_path.borrow_mut() = picked;
            app.open_dialog_active.set(false);
        }
    });
    window.on_open_recent({
        let app = app.clone();
        let weak = window.as_weak();
        move |index| {
            let path = app
                .config
                .borrow()
                .recent_files
                .get(index as usize)
                .map(PathBuf::from);
            if let (Some(path), Some(window)) = (path, weak.upgrade()) {
                load_into(&path, &app, &window);
            }
        }
    });
    window.on_clear_recent({
        let app = app.clone();
        let weak = window.as_weak();
        move || {
            app.config.borrow_mut().clear_recent();
            save(&app);
            if let Some(window) = weak.upgrade() {
                window.set_recent_files(recent_model(&app.config.borrow()));
            }
        }
    });
    window.on_set_scale({
        let app = app.clone();
        let weak = window.as_weak();
        move |scale| {
            app.config.borrow_mut().scale = scale.max(1) as u32;
            save(&app);
            if let Some(window) = weak.upgrade() {
                apply_view(&window, &app.config.borrow());
            }
        }
    });
    window.on_toggle_fullscreen({
        let app = app.clone();
        let weak = window.as_weak();
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            // Flip the *actual* OS state so the menu stays correct even after the
            // user toggled fullscreen from the title-bar button.
            let target = !window.window().is_fullscreen();
            app.config.borrow_mut().fullscreen = target;
            save(&app);
            app.pending_fullscreen.set(Some(target));
            window.set_fullscreen(target);
            apply_view(&window, &app.config.borrow());
        }
    });
    window.on_set_rotation({
        let app = app.clone();
        let weak = window.as_weak();
        move |dir| {
            let requested = match dir {
                1 => RotationKind::Right,
                2 => RotationKind::Left,
                _ => RotationKind::None,
            };
            {
                let mut c = app.config.borrow_mut();
                // Selecting the active rotation again turns it back off.
                c.rotation = if c.rotation == requested {
                    RotationKind::None
                } else {
                    requested
                };
            }
            save(&app);
            if let Some(window) = weak.upgrade() {
                apply_view(&window, &app.config.borrow());
            }
        }
    });
    window.on_set_renderer({
        let app = app.clone();
        let weak = window.as_weak();
        move |renderer| {
            app.config.borrow_mut().renderer = from_slint_renderer(renderer);
            save(&app);
            if let Some(window) = weak.upgrade() {
                window.set_renderer(renderer);
            }
        }
    });
    window.on_set_volume({
        let app = app.clone();
        let weak = window.as_weak();
        move |volume| {
            let volume = volume.clamp(0, 100) as u8;
            app.config.borrow_mut().volume = volume;
            app.volume.set(volume);
            save(&app);
            if let Some(window) = weak.upgrade() {
                window.set_volume(volume as i32);
            }
        }
    });
    window.on_toggle_pause({
        let app = app.clone();
        let weak = window.as_weak();
        move || {
            let paused = !app.paused.get();
            app.paused.set(paused);
            if let Some(window) = weak.upgrade() {
                window.set_paused(paused);
                if paused {
                    window.set_status_text(format!("{} — paused", app.rom_label.borrow()).into());
                }
            }
        }
    });
    window.on_reset_emulation({
        let app = app.clone();
        move || {
            app.bios_settings_start_frames.set(0);
            app.reset_request.set(true);
        }
    });
    window.on_open_bios_settings({
        let app = app.clone();
        let weak = window.as_weak();
        move || {
            if app.config.borrow().bios_rom == BiosRomKind::Disabled {
                if let Some(window) = weak.upgrade() {
                    window.set_status_text("Select a BIOS ROM in Settings first".into());
                }
                return;
            }
            app.bios_settings_start_frames
                .set(BIOS_SETTINGS_START_FRAMES);
            app.reset_request.set(true);
        }
    });
    window.on_open_settings({
        let app = app.clone();
        let weak = window.as_weak();
        move || {
            if let Some(window) = weak.upgrade() {
                open_settings(&app, &window);
            }
        }
    });
    window.on_show_about({
        let app = app.clone();
        move || open_about(&app)
    });
}

/// Create (or re-show) the settings window and wire its callbacks.
fn open_settings(app: &App, main: &MainWindow) {
    if let Some(existing) = app.settings.borrow().as_ref() {
        let _ = existing.show();
        return;
    }
    let settings = match SettingsWindow::new() {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("could not open settings window: {e}");
            return;
        }
    };

    settings.on_rebind_key({
        let app = app.clone();
        let weak = settings.as_weak();
        move |index| {
            if let Some(button) = Button::ALL.get(index as usize).copied() {
                app.capture.set(Some((Device::Keyboard, button)));
                if let Some(w) = weak.upgrade() {
                    w.set_listening_hint(format!("Press a key for {}…", button.label()).into());
                }
            }
        }
    });
    settings.on_rebind_pad({
        let app = app.clone();
        let weak = settings.as_weak();
        move |index| {
            if let Some(button) = Button::ALL.get(index as usize).copied() {
                app.capture.set(Some((Device::Gamepad, button)));
                if let Some(w) = weak.upgrade() {
                    w.set_listening_hint(
                        format!("Press a controller button for {}…", button.label()).into(),
                    );
                }
            }
        }
    });
    settings.on_set_bios_rom_mode({
        let app = app.clone();
        let weak = settings.as_weak();
        let main = main.as_weak();
        move |mode| {
            let kind = bios_rom_from_index(mode);
            app.config.borrow_mut().bios_rom = kind;
            save(&app);
            if let Some(w) = weak.upgrade() {
                w.set_bios_rom_mode(bios_rom_index(kind));
            }
            if let Some(window) = main.upgrade() {
                reset_emulation(&app, &window);
            }
        }
    });
    settings.on_key_captured({
        let app = app.clone();
        let main = main.as_weak();
        move |text| {
            if let Some((Device::Keyboard, button)) = app.capture.get() {
                app.keymap.borrow_mut().rebind(button, &text);
                sync_keyboard_config(&app);
                app.capture.set(None);
                save(&app);
                if let Some(window) = main.upgrade() {
                    refresh_settings_rows(&app, &window);
                }
            }
        }
    });
    settings.on_reset_defaults({
        let app = app.clone();
        let main = main.as_weak();
        move || {
            *app.keymap.borrow_mut() = Keymap::defaults();
            sync_keyboard_config(&app);
            app.config.borrow_mut().gamepad_bindings = default_gamepad_config();
            if let Some(gp) = app.gamepad.borrow_mut().as_mut() {
                gp.set_named_bindings(named_pairs(&app.config.borrow().gamepad_bindings));
            }
            app.capture.set(None);
            save(&app);
            if let Some(window) = main.upgrade() {
                refresh_settings_rows(&app, &window);
            }
        }
    });
    settings.on_close_settings({
        let app = app.clone();
        let weak = settings.as_weak();
        move || {
            app.capture.set(None);
            if let Some(w) = weak.upgrade() {
                let _ = w.hide();
            }
        }
    });

    settings.set_bios_rom_mode(bios_rom_index(app.config.borrow().bios_rom));
    settings.set_rows(binding_rows(&app.config.borrow(), &app.keymap.borrow()));
    if let Err(e) = settings.show() {
        tracing::error!("could not show settings window: {e}");
        return;
    }
    *app.settings.borrow_mut() = Some(settings);
}

/// Create (or re-show) the About window used on platforms without native About.
fn open_about(app: &App) {
    if let Some(existing) = app.about.borrow().as_ref() {
        let _ = existing.show();
        return;
    }
    let about = match AboutWindow::new() {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("could not open about window: {e}");
            return;
        }
    };
    about.set_version(env!("CARGO_PKG_VERSION").into());
    about.on_close_about({
        let weak = about.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                let _ = w.hide();
            }
        }
    });
    if let Err(e) = about.show() {
        tracing::error!("could not show about window: {e}");
        return;
    }
    *app.about.borrow_mut() = Some(about);
}

/// Assign a captured gamepad button to `ws_button`, updating live state + config.
fn assign_gamepad(app: &App, ws_button: Button, gilrs_name: &str) {
    app.config
        .borrow_mut()
        .gamepad_bindings
        .insert(ws_button.name().to_string(), gilrs_name.to_string());
    if let Some(gp) = app.gamepad.borrow_mut().as_mut() {
        gp.set_named_bindings(named_pairs(&app.config.borrow().gamepad_bindings));
    }
    app.capture.set(None);
    save(app);
}

/// Push the live keymap back into the persisted config.
fn sync_keyboard_config(app: &App) {
    let pairs = app.keymap.borrow().to_config();
    app.config.borrow_mut().keyboard_bindings = pairs.into_iter().collect();
}

/// Rebuild the settings window's binding rows and clear the listening hint.
fn refresh_settings_rows(app: &App, _main: &MainWindow) {
    if let Some(settings) = app.settings.borrow().as_ref() {
        settings.set_bios_rom_mode(bios_rom_index(app.config.borrow().bios_rom));
        settings.set_rows(binding_rows(&app.config.borrow(), &app.keymap.borrow()));
        settings.set_listening_hint(SharedString::new());
    }
}

/// Persist the current config, logging (but not failing on) an error.
fn save(app: &App) {
    if let Err(e) = app.config.borrow().save() {
        tracing::warn!("could not save config: {e}");
    }
}

/// Reconcile our fullscreen state with the OS's, once per tick.
///
/// While a fullscreen change we requested is still animating, we wait for it to
/// settle (guarded by `pending_fullscreen`) so we don't fight our own toggle.
/// Otherwise the OS is authoritative: an external change (the title-bar button)
/// is adopted into the config and the menu check, restoring the windowed size
/// when leaving fullscreen.
fn sync_fullscreen(app: &App, window: &MainWindow) {
    let actual = window.window().is_fullscreen();
    match app.pending_fullscreen.get() {
        Some(target) => {
            if actual == target {
                app.pending_fullscreen.set(None);
            }
        }
        None => {
            if app.config.borrow().fullscreen != actual {
                app.config.borrow_mut().fullscreen = actual;
                window.set_fullscreen(actual);
                if !actual {
                    apply_view(window, &app.config.borrow());
                }
                save(app);
            }
        }
    }
}

/// Apply the window's view-related state (size, fullscreen, renderer) from config.
fn apply_view(window: &MainWindow, config: &Config) {
    let scale = config.scale.max(1);
    let (bw, bh) = if config.rotation.is_rotated() {
        (video::SCREEN_HEIGHT as u32, video::SCREEN_WIDTH as u32)
    } else {
        (video::SCREEN_WIDTH as u32, video::SCREEN_HEIGHT as u32)
    };
    let w = (bw * scale) as f32;
    let h = (bh * scale) as f32 + STATUS_BAR_HEIGHT;
    window.set_view_width(w);
    window.set_view_height(h);
    window.set_current_scale(scale as i32);
    window.set_rotation(match config.rotation {
        RotationKind::None => 0,
        RotationKind::Right => 1,
        RotationKind::Left => 2,
    });
    window.set_fullscreen(config.fullscreen);
    window.set_renderer(to_slint_renderer(config.renderer));
    window.set_volume(config.volume.min(100) as i32);
    window.window().set_fullscreen(config.fullscreen);
    if !config.fullscreen {
        window.window().set_size(LogicalSize::new(w, h));
    }
}

/// Build the recent-ROM display model (file names, most-recent first).
fn recent_model(config: &Config) -> ModelRc<SharedString> {
    let names: Vec<SharedString> = config
        .recent_files
        .iter()
        .map(|p| {
            Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(p)
                .into()
        })
        .collect();
    ModelRc::from(Rc::new(VecModel::from(names)))
}

/// Build the per-button rows for the settings window.
fn binding_rows(config: &Config, keymap: &Keymap) -> ModelRc<BindingRow> {
    let rows: Vec<BindingRow> = Button::ALL
        .iter()
        .map(|&button| {
            let key = keymap
                .binding_for(button)
                .map(|t| keymap::key_display(&t))
                .unwrap_or_else(|| "—".to_string());
            let pad = config
                .gamepad_bindings
                .get(button.name())
                .cloned()
                .unwrap_or_else(|| "—".to_string());
            BindingRow {
                label: button.label().into(),
                key: key.into(),
                pad: pad.into(),
            }
        })
        .collect();
    ModelRc::from(Rc::new(VecModel::from(rows)))
}

/// Build a [`Keymap`] from the config's `button name → key text` table.
fn keymap_from_config(config: &Config) -> Keymap {
    Keymap::from_pairs(
        config
            .keyboard_bindings
            .iter()
            .filter_map(|(name, text)| Button::from_name(name).map(|b| (b, text.clone()))),
    )
}

/// The default `button name → gilrs button name` gamepad table.
fn default_gamepad_config() -> BTreeMap<String, String> {
    input::gamepad::default_gamepad_bindings()
        .into_iter()
        .filter_map(|(gbtn, btn)| {
            input::gamepad::gilrs_button_name(gbtn).map(|n| (btn.name().to_string(), n.to_string()))
        })
        .collect()
}

/// Borrow a `BTreeMap<String, String>` as `(&str, &str)` pairs for `set_named_bindings`.
fn named_pairs(map: &BTreeMap<String, String>) -> Vec<(&str, &str)> {
    map.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

fn to_slint_renderer(kind: RendererKind) -> Renderer {
    match kind {
        RendererKind::Nearest => Renderer::Nearest,
        RendererKind::Linear => Renderer::Linear,
    }
}

fn from_slint_renderer(renderer: Renderer) -> RendererKind {
    match renderer {
        Renderer::Nearest => RendererKind::Nearest,
        Renderer::Linear => RendererKind::Linear,
    }
}

fn bios_rom_index(kind: BiosRomKind) -> i32 {
    match kind {
        BiosRomKind::Disabled => 0,
        BiosRomKind::WonderSwan => 1,
        BiosRomKind::WonderSwanColor => 2,
        BiosRomKind::WonderSwanCrystal => 3,
    }
}

fn bios_rom_from_index(index: i32) -> BiosRomKind {
    match index {
        1 => BiosRomKind::WonderSwan,
        2 => BiosRomKind::WonderSwanColor,
        3 => BiosRomKind::WonderSwanCrystal,
        _ => BiosRomKind::Disabled,
    }
}

fn forced_model_from_bios(kind: BiosRomKind) -> Option<HardwareModel> {
    match kind {
        BiosRomKind::Disabled => None,
        BiosRomKind::WonderSwan => Some(HardwareModel::Mono),
        BiosRomKind::WonderSwanColor => Some(HardwareModel::Color),
        BiosRomKind::WonderSwanCrystal => Some(HardwareModel::Crystal),
    }
}

fn bios_rom_label(kind: BiosRomKind) -> &'static str {
    match kind {
        BiosRomKind::Disabled => "direct boot",
        BiosRomKind::WonderSwan => "WonderSwan BIOS",
        BiosRomKind::WonderSwanColor => "WonderSwan Color BIOS",
        BiosRomKind::WonderSwanCrystal => "SwanCrystal BIOS",
    }
}

fn load_bios_rom(kind: BiosRomKind) -> Option<(PathBuf, Vec<u8>)> {
    let path = Config::bios_path(kind)
        .inspect_err(|e| tracing::warn!("could not locate BIOS directory: {e}"))
        .ok()
        .flatten()?;
    match std::fs::read(&path) {
        Ok(bytes) => {
            tracing::info!(
                bios = %path.display(),
                bytes = bytes.len(),
                "loaded BIOS ROM"
            );
            Some((path, bytes))
        }
        Err(e) => {
            tracing::warn!(bios = %path.display(), "could not load BIOS ROM: {e}");
            None
        }
    }
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "core panic".to_string()
    }
}

fn cpu_context(system: &System) -> String {
    let cpu = system.cpu();
    let opcode_ip = cpu.regs.ip.wrapping_sub(1);
    format!(
        "CS:IP={:04X}:{:04X} AX={:04X} BX={:04X} CX={:04X} DX={:04X} SP={:04X}",
        cpu.regs.cs, opcode_ip, cpu.regs.ax, cpu.regs.bx, cpu.regs.cx, cpu.regs.dx, cpu.regs.sp
    )
}

fn reset_emulation(app: &App, window: &MainWindow) {
    if let Some(path) = app.rom_path.borrow().clone() {
        load_into(&path, app, window);
        if app.paused.get() {
            window.set_status_text(format!("{} — paused", app.rom_label.borrow()).into());
        }
        return;
    }

    let bios_kind = app.config.borrow().bios_rom;
    let Some((bios_path, boot_rom)) = load_bios_rom(bios_kind) else {
        *app.system.borrow_mut() = None;
        app.rom_label.borrow_mut().clear();
        window.set_window_title("Swanium".into());
        window.set_status_text("No ROM loaded".into());
        window.set_has_rom(false);
        return;
    };

    let mut sys = System::from_rom_with_boot_rom(empty_cartridge_rom(), boot_rom);
    if let Some(model) = forced_model_from_bios(bios_kind) {
        sys.set_model(model);
    }
    let kind = if sys.model().is_color() {
        "Color"
    } else {
        "Mono"
    };
    let name = bios_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| bios_rom_label(bios_kind));
    *app.system.borrow_mut() = Some(sys);
    *app.rom_label.borrow_mut() = bios_rom_label(bios_kind).to_string();
    window.set_window_title(format!("Swanium — {}", bios_rom_label(bios_kind)).into());
    window.set_status_text(
        format!("{} [{kind}, {name}] — running", bios_rom_label(bios_kind)).into(),
    );
    window.set_has_rom(true);
}

fn empty_cartridge_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x10000];
    // Real BIOSes validate that the cartridge reset entry is a far jump before
    // entering the splash/configuration path. Point it at a HLT instruction so
    // BIOS-only startup can hand off cleanly instead of taking the error stop.
    rom[0x0000] = 0xF4;
    rom[0xFFF0..0xFFF5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0x40]);
    rom
}

/// Pop the native "open ROM" dialog, returning the chosen path (if any).
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
fn load_into(path: &Path, app: &App, window: &MainWindow) {
    match std::fs::read(path) {
        Ok(bytes) => {
            tracing::info!(rom = %path.display(), bytes = bytes.len(), "loaded ROM");
            let bios_kind = app.config.borrow().bios_rom;
            let mut loaded_bios = None;
            let mut sys = match load_bios_rom(bios_kind) {
                Some((bios_path, boot_rom)) => {
                    loaded_bios = Some(bios_path);
                    System::from_rom_with_boot_rom(bytes, boot_rom)
                }
                None => System::from_rom(bytes),
            };
            // Run `.wsc` images as WonderSwan Color hardware (colour support is
            // a property of the console, not the cartridge header).
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wsc"))
            {
                sys.set_model(HardwareModel::Color);
            }
            if let Some(model) = forced_model_from_bios(app.config.borrow().bios_rom) {
                sys.set_model(model);
            }
            let kind = if sys.model().is_color() {
                "Color"
            } else {
                "Mono"
            };
            *app.system.borrow_mut() = Some(sys);
            *app.last_dir.borrow_mut() = path.parent().map(Path::to_path_buf);
            *app.rom_path.borrow_mut() = Some(path.to_path_buf());
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("ROM");
            *app.rom_label.borrow_mut() = name.to_string();

            app.config.borrow_mut().push_recent(path.to_string_lossy());
            save(app);
            window.set_recent_files(recent_model(&app.config.borrow()));
            window.set_window_title(format!("Swanium — {name}").into());
            let boot = match (bios_kind, loaded_bios.as_ref()) {
                (BiosRomKind::Disabled, _) => "direct boot".to_string(),
                (_, Some(path)) => path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_else(|| bios_rom_label(bios_kind))
                    .to_string(),
                (_, None) => format!("{} missing; direct boot", bios_rom_label(bios_kind)),
            };
            window.set_status_text(format!("{name} [{kind}, {boot}] — running").into());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cartridge_rom_has_real_bios_compatible_reset_jump() {
        let rom = empty_cartridge_rom();
        assert_eq!(&rom[0xFFF0..0xFFF5], &[0xEA, 0x00, 0x00, 0x00, 0x40]);
        assert_eq!(rom[0x0000], 0xF4);
    }
}
