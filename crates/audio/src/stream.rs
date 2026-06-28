//! cpal output stream wired to the emulator's [`RingBuffer`].
//!
//! [`AudioStream::open`] opens the default host output device and spawns a
//! cpal audio thread.  The emulator pushes interleaved stereo `i16` samples
//! each frame via [`AudioStream::push`]; the cpal callback drains the
//! [`RingBuffer`] on its own thread at the device's native sample rate.
//!
//! The emulator's APU produces samples at 24 kHz ([`APU_SAMPLE_RATE`]).  If
//! the device uses a different rate the samples are first passed through a
//! zero-order-hold [`Resampler`] before being queued.  Higher-quality
//! resampling is a Phase 9 follow-up.
//!
//! **Audio-video sync**: overruns (emulator faster than device) silently drop
//! the newest samples; underruns pad with silence.  Adaptive frame-pacing
//! based on ring-buffer fill level is a follow-up task.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, Stream, StreamConfig};

use crate::resample::Resampler;
use crate::RingBuffer;

/// Sample rate the APU generates — must match `swanium_core::apu::Apu::OUTPUT_SAMPLE_RATE`.
const APU_SAMPLE_RATE: u32 = 24_000;

/// Ring buffer capacity in `i16` samples (≈170 ms stereo at 48 kHz).
const RING_CAPACITY: usize = 16_384;

/// Pre-allocated scratch buffer size for the cpal callback (avoids heap
/// allocation in the hot audio path).
const SCRATCH_SIZE: usize = 8_192;

/// Connects the emulator's APU output to a host audio device via cpal.
///
/// Call [`AudioStream::push`] once per emulated frame to supply new samples.
/// The cpal output thread drains the internal [`RingBuffer`] independently.
///
/// Dropping this value stops the audio stream.
pub struct AudioStream {
    ring: Arc<Mutex<RingBuffer>>,
    resampler: Resampler,
    /// Temporary buffer reused across [`push`](Self::push) calls.
    resampled: Vec<i16>,
    /// Keeps the cpal stream alive for the lifetime of this struct.
    _stream: Stream,
}

impl AudioStream {
    /// Open the default host output device and start the audio stream.
    ///
    /// Returns an error when no output device is available or the stream
    /// cannot be created (e.g. in a headless CI environment).
    pub fn open() -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default audio output device")?;
        let supported = device.default_output_config()?;

        let device_rate = supported.sample_rate().0;
        let config = StreamConfig {
            channels: 2,
            sample_rate: SampleRate(device_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let ring = Arc::new(Mutex::new(RingBuffer::new(RING_CAPACITY)));
        let stream = build_stream(&device, &config, supported.sample_format(), ring.clone())?;
        stream.play()?;

        tracing::info!(device_rate, "audio stream opened");

        Ok(Self {
            ring,
            resampler: Resampler::new(APU_SAMPLE_RATE, device_rate),
            resampled: Vec::new(),
            _stream: stream,
        })
    }

    /// Push one frame's worth of interleaved stereo `i16` samples from the APU.
    ///
    /// Samples are resampled to the device rate and enqueued in the ring
    /// buffer.  Excess samples are silently dropped (overrun).
    pub fn push(&mut self, samples: &[i16]) {
        self.resampled.clear();
        self.resampler.process(samples, &mut self.resampled);
        let _ = self.ring.lock().unwrap().push(&self.resampled);
    }
}

/// Build a cpal output stream for the detected sample format.
fn build_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    ring: Arc<Mutex<RingBuffer>>,
) -> Result<Stream, cpal::BuildStreamError> {
    match sample_format {
        SampleFormat::F32 => {
            let ring_cb = ring;
            let mut scratch = vec![0i16; SCRATCH_SIZE];
            device.build_output_stream(
                config,
                move |output: &mut [f32], _| drain_f32(output, &ring_cb, &mut scratch),
                |err| tracing::error!(?err, "cpal stream error"),
                None,
            )
        }
        SampleFormat::I16 => {
            let ring_cb = ring;
            let mut scratch = vec![0i16; SCRATCH_SIZE];
            device.build_output_stream(
                config,
                move |output: &mut [i16], _| drain_i16(output, &ring_cb, &mut scratch),
                |err| tracing::error!(?err, "cpal stream error"),
                None,
            )
        }
        SampleFormat::U16 => {
            let ring_cb = ring;
            let mut scratch = vec![0i16; SCRATCH_SIZE];
            device.build_output_stream(
                config,
                move |output: &mut [u16], _| drain_u16(output, &ring_cb, &mut scratch),
                |err| tracing::error!(?err, "cpal stream error"),
                None,
            )
        }
        fmt => {
            tracing::warn!(?fmt, "unsupported sample format; falling back to f32");
            let ring_cb = ring;
            let mut scratch = vec![0i16; SCRATCH_SIZE];
            device.build_output_stream(
                config,
                move |output: &mut [f32], _| drain_f32(output, &ring_cb, &mut scratch),
                |err| tracing::error!(?err, "cpal stream error"),
                None,
            )
        }
    }
}

fn drain_f32(output: &mut [f32], ring: &Arc<Mutex<RingBuffer>>, scratch: &mut Vec<i16>) {
    let n = output.len();
    ensure_scratch(scratch, n);
    ring.lock().unwrap().pop_into(&mut scratch[..n]);
    for (out, &s) in output.iter_mut().zip(&scratch[..n]) {
        *out = s as f32 / 32_768.0;
    }
}

fn drain_i16(output: &mut [i16], ring: &Arc<Mutex<RingBuffer>>, scratch: &mut Vec<i16>) {
    let n = output.len();
    ensure_scratch(scratch, n);
    ring.lock().unwrap().pop_into(&mut scratch[..n]);
    output.copy_from_slice(&scratch[..n]);
}

fn drain_u16(output: &mut [u16], ring: &Arc<Mutex<RingBuffer>>, scratch: &mut Vec<i16>) {
    let n = output.len();
    ensure_scratch(scratch, n);
    ring.lock().unwrap().pop_into(&mut scratch[..n]);
    for (out, &s) in output.iter_mut().zip(&scratch[..n]) {
        // i16 → u16: shift origin from −32768..32767 to 0..65535
        *out = (s as i32 + 32_768) as u16;
    }
}

/// Grow `scratch` to at least `n` elements without reallocating if possible.
#[inline]
fn ensure_scratch(scratch: &mut Vec<i16>, n: usize) {
    if scratch.len() < n {
        scratch.resize(n, 0);
    }
}
