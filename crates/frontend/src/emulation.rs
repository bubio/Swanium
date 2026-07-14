//! Dedicated emulator worker and its GUI-facing message boundary.
//!
//! The worker owns the mutable [`System`] and audio producer. Slint remains on
//! the main thread and exchanges only small commands, input snapshots, status
//! events, and copied framebuffers with this module.

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU16, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use audio::AudioProducer;
use swanium_core::keypad::KeyState;
use swanium_core::ppu::{Rgb444, SCREEN_HEIGHT, SCREEN_WIDTH};
use swanium_core::system::{System, CYCLES_PER_FRAME, MASTER_CLOCK_HZ};

const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(4);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
const FPS_REFRESH: Duration = Duration::from_millis(500);
const AUDIO_LOW_NUM: usize = 1;
const AUDIO_LOW_DEN: usize = 4;
const AUDIO_HIGH_NUM: usize = 3;
const AUDIO_HIGH_DEN: usize = 4;

/// A status update produced by the emulation thread for the Slint thread.
pub(crate) enum EmulationEvent {
    Fps(f32),
    Stopped(String),
}

enum Command {
    ReplaceSystem(Box<System>),
    ClearSystem,
    SetPaused(bool),
    HoldStart(u8),
    DumpFrame,
    SaveData(mpsc::SyncSender<Option<Vec<u8>>>),
    SaveState(mpsc::SyncSender<Result<Vec<u8>, String>>),
    LoadState(Vec<u8>, mpsc::SyncSender<Result<(), String>>),
    Shutdown,
}

struct PublishedFrame {
    pixels: Vec<Rgb444>,
    generation: u64,
}

impl PublishedFrame {
    fn new() -> Self {
        Self {
            pixels: vec![0; SCREEN_WIDTH * SCREEN_HEIGHT],
            generation: 0,
        }
    }

    fn publish(&mut self, pixels: &[Rgb444]) {
        self.pixels.copy_from_slice(pixels);
        self.generation = self.generation.wrapping_add(1).max(1);
    }
}

struct SharedState {
    input_bits: AtomicU16,
    volume: AtomicU8,
    frame: Mutex<PublishedFrame>,
}

/// GUI-side handle for the dedicated emulator thread.
pub(crate) struct EmulationWorker {
    command_tx: mpsc::Sender<Command>,
    event_rx: mpsc::Receiver<EmulationEvent>,
    shared: Arc<SharedState>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl EmulationWorker {
    /// Start the worker, optionally attaching the producer half of host audio.
    pub(crate) fn spawn(audio: Option<AudioProducer>) -> std::io::Result<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let shared = Arc::new(SharedState {
            input_bits: AtomicU16::new(0),
            volume: AtomicU8::new(100),
            frame: Mutex::new(PublishedFrame::new()),
        });
        let worker_shared = shared.clone();
        let thread = thread::Builder::new()
            .name("swanium-emulation".to_string())
            .spawn(move || worker_loop(command_rx, event_tx, worker_shared, audio))?;

        Ok(Self {
            command_tx,
            event_rx,
            shared,
            thread: Mutex::new(Some(thread)),
        })
    }

    pub(crate) fn replace_system(&self, system: System) -> Result<(), String> {
        self.send(Command::ReplaceSystem(Box::new(system)))
    }

    pub(crate) fn clear_system(&self) -> Result<(), String> {
        self.send(Command::ClearSystem)
    }

    pub(crate) fn set_paused(&self, paused: bool) -> Result<(), String> {
        self.send(Command::SetPaused(paused))
    }

    pub(crate) fn set_input(&self, keys: KeyState) {
        self.shared.input_bits.store(keys.bits(), Ordering::Relaxed);
    }

    pub(crate) fn set_volume(&self, volume: u8) {
        self.shared.volume.store(volume.min(100), Ordering::Relaxed);
    }

    pub(crate) fn hold_start(&self, frames: u8) -> Result<(), String> {
        self.send(Command::HoldStart(frames))
    }

    pub(crate) fn request_dump(&self) -> Result<(), String> {
        self.send(Command::DumpFrame)
    }

    pub(crate) fn save_data(&self) -> Result<Option<Vec<u8>>, String> {
        self.request(Command::SaveData)
    }

    pub(crate) fn save_state(&self) -> Result<Vec<u8>, String> {
        self.request(Command::SaveState)?
    }

    pub(crate) fn load_state(&self, data: Vec<u8>) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.send(Command::LoadState(data, reply_tx))?;
        reply_rx
            .recv_timeout(COMMAND_TIMEOUT)
            .map_err(|e| format!("emulation worker did not load state: {e}"))?
    }

    pub(crate) fn try_event(&self) -> Option<EmulationEvent> {
        self.event_rx.try_recv().ok()
    }

    pub(crate) fn shutdown(&self) {
        let _ = self.command_tx.send(Command::Shutdown);
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }

    /// Copy the newest framebuffer into `out` without holding its mutex during
    /// RGBA conversion. Returns its generation, or `None` when unchanged.
    pub(crate) fn copy_frame(
        &self,
        previous_generation: u64,
        force: bool,
        out: &mut Vec<Rgb444>,
    ) -> Option<u64> {
        let frame = self
            .shared
            .frame
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if frame.generation == 0 || (!force && frame.generation == previous_generation) {
            return None;
        }
        out.resize(frame.pixels.len(), 0);
        out.copy_from_slice(&frame.pixels);
        Some(frame.generation)
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.command_tx
            .send(command)
            .map_err(|_| "emulation worker has stopped".to_string())
    }

    fn request<T>(
        &self,
        command: impl FnOnce(mpsc::SyncSender<T>) -> Command,
    ) -> Result<T, String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.send(command(reply_tx))?;
        reply_rx
            .recv_timeout(COMMAND_TIMEOUT)
            .map_err(|e| format!("emulation worker did not reply: {e}"))
    }
}

impl Drop for EmulationWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct WorkerState {
    system: Option<System>,
    audio: Option<AudioProducer>,
    paused: bool,
    dump_requested: bool,
    start_frames: u8,
    next_frame_at: Instant,
    frames_since_refresh: u32,
    fps_anchor: Instant,
}

impl WorkerState {
    fn new(audio: Option<AudioProducer>) -> Self {
        let now = Instant::now();
        Self {
            system: None,
            audio,
            paused: false,
            dump_requested: false,
            start_frames: 0,
            next_frame_at: now,
            frames_since_refresh: 0,
            fps_anchor: now,
        }
    }

    fn handle_command(&mut self, command: Command, shared: &SharedState) -> bool {
        match command {
            Command::ReplaceSystem(system) => {
                self.clear_audio();
                publish_frame(&system, shared);
                self.system = Some(*system);
                self.start_frames = 0;
                self.reset_pacing();
            }
            Command::ClearSystem => {
                self.clear_audio();
                self.system = None;
                self.start_frames = 0;
            }
            Command::SetPaused(paused) => {
                self.paused = paused;
                self.clear_audio();
                self.reset_pacing();
            }
            Command::HoldStart(frames) => self.start_frames = frames,
            Command::DumpFrame => self.dump_requested = true,
            Command::SaveData(reply) => {
                let data = self
                    .system
                    .as_ref()
                    .map(|system| system.save_data().to_vec());
                let _ = reply.send(data);
            }
            Command::SaveState(reply) => {
                let result = self
                    .system
                    .as_ref()
                    .ok_or_else(|| "no system loaded".to_string())
                    .and_then(|system| system.save_state_bytes().map_err(|e| e.to_string()));
                let _ = reply.send(result);
            }
            Command::LoadState(data, reply) => {
                self.clear_audio();
                let result = self
                    .system
                    .as_mut()
                    .ok_or_else(|| "no system loaded".to_string())
                    .and_then(|system| {
                        system.load_state_bytes(&data).map_err(|e| e.to_string())?;
                        publish_frame(system, shared);
                        Ok(())
                    });
                self.reset_pacing();
                let _ = reply.send(result);
            }
            Command::Shutdown => return false,
        }
        true
    }

    fn clear_audio(&self) {
        if let Some(audio) = &self.audio {
            audio.clear();
        }
    }

    fn reset_pacing(&mut self) {
        let now = Instant::now();
        self.next_frame_at = now;
        self.frames_since_refresh = 0;
        self.fps_anchor = now;
    }

    fn runnable(&self) -> bool {
        self.system.is_some() && (!self.paused || self.dump_requested)
    }

    fn audio_low(&self) -> bool {
        self.audio.as_ref().is_some_and(|audio| {
            audio.ring_fill() * AUDIO_LOW_DEN < audio.ring_capacity() * AUDIO_LOW_NUM
        })
    }

    fn audio_high(&self) -> bool {
        self.audio.as_ref().is_some_and(|audio| {
            audio.ring_fill() * AUDIO_HIGH_DEN >= audio.ring_capacity() * AUDIO_HIGH_NUM
        })
    }
}

fn worker_loop(
    command_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::Sender<EmulationEvent>,
    shared: Arc<SharedState>,
    audio: Option<AudioProducer>,
) {
    let mut state = WorkerState::new(audio);
    loop {
        while let Ok(command) = command_rx.try_recv() {
            if !state.handle_command(command, &shared) {
                return;
            }
        }

        if !state.runnable() {
            match command_rx.recv_timeout(COMMAND_POLL_INTERVAL) {
                Ok(command) => {
                    if !state.handle_command(command, &shared) {
                        return;
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }

        let now = Instant::now();
        let audio_low = state.audio_low();
        let audio_high = state.audio_high();
        let should_run =
            state.dump_requested || (!audio_high && (audio_low || now >= state.next_frame_at));
        if !should_run {
            let wait = if audio_high {
                COMMAND_POLL_INTERVAL
            } else {
                state
                    .next_frame_at
                    .saturating_duration_since(now)
                    .min(COMMAND_POLL_INTERVAL)
                    .max(Duration::from_micros(100))
            };
            match command_rx.recv_timeout(wait) {
                Ok(command) => {
                    if !state.handle_command(command, &shared) {
                        return;
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }

        run_one_frame(&mut state, &shared, &event_tx);
    }
}

fn run_one_frame(
    state: &mut WorkerState,
    shared: &SharedState,
    event_tx: &mpsc::Sender<EmulationEvent>,
) {
    let dump = std::mem::take(&mut state.dump_requested);
    let mut keys = KeyState::from_bits(shared.input_bits.load(Ordering::Relaxed));
    if state.start_frames > 0 {
        keys |= KeyState::START;
        state.start_frames = state.start_frames.saturating_sub(1);
    }

    let Some(system) = state.system.as_mut() else {
        return;
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        if dump {
            dump_display_registers(system, keys);
        } else {
            system.run_frame(keys);
        }
    }));
    if let Err(payload) = result {
        let reason = panic_payload_message(payload.as_ref());
        let context = cpu_context(system);
        stop_worker(state, event_tx, format!("stopped: {reason} at {context}"));
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
        stop_worker(
            state,
            event_tx,
            format!(
                "stopped: unsupported opcode 0x{:02X} at {context}",
                fault.opcode
            ),
        );
        return;
    }

    if let Some(audio) = state.audio.as_mut() {
        audio.set_volume(shared.volume.load(Ordering::Relaxed));
        audio.push(system.audio_samples());
    }
    system.clear_audio_samples();
    publish_frame(system, shared);

    state.frames_since_refresh += 1;
    let now = Instant::now();
    let elapsed = now.duration_since(state.fps_anchor);
    if elapsed >= FPS_REFRESH {
        let fps = state.frames_since_refresh as f32 / elapsed.as_secs_f32();
        state.frames_since_refresh = 0;
        state.fps_anchor = now;
        let _ = event_tx.send(EmulationEvent::Fps(fps));
    }

    let period = frame_period();
    if now.saturating_duration_since(state.next_frame_at) > period * 2 {
        state.next_frame_at = now + period;
    } else {
        state.next_frame_at += period;
    }
}

fn stop_worker(state: &mut WorkerState, event_tx: &mpsc::Sender<EmulationEvent>, message: String) {
    tracing::error!("emulation {message}");
    state.paused = true;
    state.clear_audio();
    let _ = event_tx.send(EmulationEvent::Stopped(message));
}

fn publish_frame(system: &System, shared: &SharedState) {
    shared
        .frame
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .publish(system.framebuffer());
}

fn frame_period() -> Duration {
    let nanos = u64::from(CYCLES_PER_FRAME) * 1_000_000_000 / u64::from(MASTER_CLOCK_HZ);
    Duration::from_nanos(nanos)
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
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
                "  {:03}: {:02X} {:02X} {:02X} {:02X}",
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
        for y in (0..SCREEN_HEIGHT as u8).step_by(8) {
            let mut row = String::new();
            for x in (0..SCREEN_WIDTH).step_by(8) {
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

    fn halted_system() -> System {
        let mut rom = vec![0u8; 0x10000];
        rom[0xFFF0] = 0xF4;
        System::from_rom(rom)
    }

    fn wait_for_frame(worker: &EmulationWorker) -> (u64, Vec<Rgb444>) {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut pixels = Vec::new();
        loop {
            if let Some(generation) = worker.copy_frame(0, false, &mut pixels) {
                return (generation, pixels);
            }
            assert!(Instant::now() < deadline, "worker did not publish a frame");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn replacing_system_publishes_initial_frame() {
        let worker = EmulationWorker::spawn(None).expect("worker should start");
        worker.set_paused(true).expect("worker should accept pause");
        worker
            .replace_system(halted_system())
            .expect("worker should accept system");
        let (_, pixels) = wait_for_frame(&worker);
        assert_eq!(pixels.len(), SCREEN_WIDTH * SCREEN_HEIGHT);
    }

    #[test]
    fn unchanged_frame_is_not_copied_twice() {
        let worker = EmulationWorker::spawn(None).expect("worker should start");
        worker.set_paused(true).expect("worker should accept pause");
        worker
            .replace_system(halted_system())
            .expect("worker should accept system");
        let (generation, mut pixels) = wait_for_frame(&worker);
        assert!(worker.copy_frame(generation, false, &mut pixels).is_none());
    }

    #[test]
    fn worker_advances_without_gui_frame_polling() {
        let worker = EmulationWorker::spawn(None).expect("worker should start");
        worker
            .replace_system(halted_system())
            .expect("worker should accept system");
        let (first_generation, mut pixels) = wait_for_frame(&worker);
        let deadline = Instant::now() + Duration::from_secs(1);
        let next_generation = loop {
            if let Some(generation) = worker.copy_frame(first_generation, false, &mut pixels) {
                break generation;
            }
            assert!(Instant::now() < deadline, "worker did not advance a frame");
            thread::sleep(Duration::from_millis(1));
        };
        assert!(next_generation > first_generation);
    }

    #[test]
    fn frame_period_matches_wonderswan_clock_ratio() {
        assert_eq!(frame_period(), Duration::from_micros(13_250));
    }
}
