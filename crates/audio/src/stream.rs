//! cpal output stream wired to the emulator's [`RingBuffer`].
//!
//! [`AudioStream::open`] opens the default host output device and returns both
//! the cpal stream lifetime guard and an [`AudioProducer`]. The emulator moves
//! the producer to its worker thread and pushes interleaved stereo `i16`
//! samples each frame; the cpal callback drains the [`RingBuffer`] on its own
//! thread at the device's native sample rate.
//!
//! The emulator's APU produces samples at 24 kHz ([`APU_SAMPLE_RATE`]).  If
//! the device uses a different rate the samples are first passed through a
//! linear [`Resampler`] before being queued.  Band-limited resampling is still
//! a possible Phase 9 follow-up.
//!
//! **Audio-video sync**: overruns (emulator faster than device) silently drop
//! the newest samples; underruns pad with silence.  Adaptive frame-pacing
//! based on ring-buffer fill level is a follow-up task.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, Stream, StreamConfig};

use crate::resample::Resampler;
use crate::{scale_volume, RingBuffer};

/// Sample rate the APU generates — must match `swanium_core::apu::Apu::OUTPUT_SAMPLE_RATE`.
const APU_SAMPLE_RATE: u32 = 24_000;

/// Ring buffer capacity in `i16` samples (≈170 ms stereo at 48 kHz).
const RING_CAPACITY: usize = 16_384;

/// Pre-allocated scratch buffer size for the cpal callback (avoids heap
/// allocation in the hot audio path).
const SCRATCH_SIZE: usize = 8_192;

/// Keeps the host cpal output stream alive.
///
/// [`AudioStream::open`] also returns an [`AudioProducer`] that can be moved to
/// the emulator thread. Dropping this value stops host audio output.
pub struct AudioStream {
    /// Keeps the cpal stream alive for the lifetime of this struct.
    _stream: Stream,
}

/// Producer-side audio state owned by the emulator worker thread.
///
/// Resamples one emulated frame at a time and queues it for the independent
/// cpal callback. This type contains no platform stream handle and is `Send`.
pub struct AudioProducer {
    ring: Arc<Mutex<RingBuffer>>,
    /// Cached capacity so callers can read it without locking.
    ring_capacity: usize,
    resampler: Resampler,
    /// Temporary buffer reused across [`push`](Self::push) calls.
    resampled: Vec<i16>,
    /// Master volume, 0 (mute) – 100 (full), applied in [`push`](Self::push).
    volume: u8,
}

impl AudioStream {
    /// Open the default host output device and create its producer endpoint.
    ///
    /// Returns an error when no output device is available or the stream
    /// cannot be created (e.g. in a headless CI environment).
    pub fn open() -> Result<(Self, AudioProducer), Box<dyn std::error::Error>> {
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

        let producer = AudioProducer::new(ring, device_rate);
        Ok((Self { _stream: stream }, producer))
    }
}

impl AudioProducer {
    fn new(ring: Arc<Mutex<RingBuffer>>, device_rate: u32) -> Self {
        Self {
            ring,
            ring_capacity: RING_CAPACITY,
            resampler: Resampler::new(APU_SAMPLE_RATE, device_rate),
            resampled: Vec::new(),
            volume: 100,
        }
    }

    /// Set the master volume, 0 (mute) – 100 (full). Values above 100 are
    /// clamped. Applied to subsequently pushed frames.
    pub fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
    }

    /// Push one frame's worth of interleaved stereo `i16` samples from the APU.
    ///
    /// Samples are resampled to the device rate and enqueued in the ring
    /// buffer.  Excess samples are silently dropped (overrun).
    pub fn push(&mut self, samples: &[i16]) {
        self.resampled.clear();
        self.resampler.process(samples, &mut self.resampled);
        if self.volume != 100 {
            for s in &mut self.resampled {
                *s = scale_volume(*s, self.volume);
            }
        }
        let _ = self
            .ring
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(&self.resampled);
    }

    /// Number of samples currently queued (lock-based snapshot).
    ///
    /// Used by the frontend to decide whether to run another emulated frame
    /// before the ring buffer empties and an underrun occurs.
    pub fn ring_fill(&self) -> usize {
        self.ring
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Total ring buffer capacity in samples (constant after construction).
    pub fn ring_capacity(&self) -> usize {
        self.ring_capacity
    }

    /// Drop every queued sample.
    ///
    /// Used when swapping ROMs so the previous game's trailing audio does not
    /// bleed into the next one; the cpal thread then underruns to silence until
    /// the new ROM fills the buffer again.
    pub fn clear(&self) {
        self.ring
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
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
    ring.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .pop_into(&mut scratch[..n]);
    for (out, &s) in output.iter_mut().zip(&scratch[..n]) {
        *out = s as f32 / 32_768.0;
    }
}

fn drain_i16(output: &mut [i16], ring: &Arc<Mutex<RingBuffer>>, scratch: &mut Vec<i16>) {
    let n = output.len();
    ensure_scratch(scratch, n);
    ring.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .pop_into(&mut scratch[..n]);
    output.copy_from_slice(&scratch[..n]);
}

fn drain_u16(output: &mut [u16], ring: &Arc<Mutex<RingBuffer>>, scratch: &mut Vec<i16>) {
    let n = output.len();
    ensure_scratch(scratch, n);
    ring.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .pop_into(&mut scratch[..n]);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn producer_at_native_rate() -> (AudioProducer, Arc<Mutex<RingBuffer>>) {
        let ring = Arc::new(Mutex::new(RingBuffer::new(RING_CAPACITY)));
        (AudioProducer::new(ring.clone(), APU_SAMPLE_RATE), ring)
    }

    #[test]
    fn audio_producer_can_move_to_emulation_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<AudioProducer>();
    }

    #[test]
    fn producer_push_queues_samples_for_callback() {
        let (mut producer, ring) = producer_at_native_rate();
        producer.push(&[10, 20, 30, 40]);
        assert_eq!(
            ring.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len(),
            4
        );
    }

    #[test]
    fn producer_clear_drops_queued_samples() {
        let (mut producer, _) = producer_at_native_rate();
        producer.push(&[10, 20]);
        producer.clear();
        assert_eq!(producer.ring_fill(), 0);
    }
}
