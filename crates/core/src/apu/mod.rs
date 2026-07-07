//! WonderSwan APU (audio): four wave-table channels plus the voice (PCM),
//! sweep, and noise features.
//!
//! All four channels are 32-step / 4-bit-per-sample wave-table oscillators that
//! read their waveform out of the shared internal WRAM (the same memory the PPU
//! and CPU see). On top of the plain wave sampler:
//!
//! - **Channel 2** can act as an 8-bit PCM "voice" (port `0x89` supplies the
//!   sample, port `0x94` the left/right voice volume).
//! - **Channel 3** can be frequency-swept (ports `0x8C`/`0x8D`).
//! - **Channel 4** can be driven by a 15-bit noise LFSR (port `0x8E`), whose
//!   current value is exposed back through the read-only `0x92`/`0x93` ports.
//!
//! The unit is clocked at the 3.072 MHz sound clock — one tick per CPU cycle —
//! and emits one interleaved stereo [`i16`] sample every
//! [`Apu::CYCLES_PER_SAMPLE`] ticks, i.e. at [`Apu::OUTPUT_SAMPLE_RATE`].
//!
//! The noise generator (channel 4) follows StoicGoose's `SoundChannel4`: a 15-bit
//! LFSR with XNOR feedback (`1 ^ bit7 ^ bit_tap`), stepped every `2048 - pitch`
//! sound-clock ticks, whose low bit drives a `0x0F`/`0x00` DAC.  The sweep-step
//! reload uses a saturating subtraction (a deviation from WonderCrab) so a zero
//! sweep-time register cannot underflow.
//!
//! **HyperVoice** (WonderSwan Color only) is an extra PCM source, separate from
//! the four wave channels. The normal path expands 8-bit samples to a signed
//! ~11-bit sample and sums it into the stereo output. The direct path accepts
//! signed 16-bit left/right output words at ports `0x64`–`0x67`, matching Sacred
//! Tech Scroll's direct-access description. The Color/video-mode gate lives in
//! the bus, so mono or color-mode-disabled hardware skips the mix.

#[cfg(test)]
mod tests;

/// Width of the visible audio output: interleaved stereo (`L, R`) samples.
pub const STEREO_CHANNELS: usize = 2;

// ── Sound control register (port 0x90) bits ──────────────────────────────────
const CTRL_ENABLE: [u8; 4] = [0x01, 0x02, 0x04, 0x08];
const CTRL_VOICE: u8 = 0x20; // channel 2 → 8-bit PCM voice
const CTRL_SWEEP: u8 = 0x40; // channel 3 → frequency sweep
const CTRL_NOISE: u8 = 0x80; // channel 4 → noise LFSR

// ── I/O port map (sound block) ───────────────────────────────────────────────
const SND_CH_PITCH: usize = 0x80; // 0x80..0x88: 4× 11-bit period (lo, hi)
const SND_CH_VOL: usize = 0x88; // 0x88..0x8C: 4× volume (L nibble, R nibble)
const SND_SWEEP_VALUE: usize = 0x8C; // signed sweep delta
const SND_SWEEP_TIME: usize = 0x8D; // sweep step count (5 bits)
const SND_NOISE: usize = 0x8E; // noise control (tap, enable, reset)
const SND_WAVE_BASE: usize = 0x8F; // waveform base address >> 6
const SND_OUTPUT_CTRL: usize = 0x91; // speaker/headphone output mode and speaker shift
const SND_RANDOM: usize = 0x92; // noise LFSR readback (lo, hi)
const SND_VOICE_VOL: usize = 0x94; // voice (channel 2) L/R volume

// ── HyperVoice (WonderSwan Color only) ───────────────────────────────────────
const HV_DIRECT_L_LO: usize = 0x64; // signed 16-bit direct left output
const HV_DIRECT_L_HI: usize = 0x65;
const HV_DIRECT_R_LO: usize = 0x66; // signed 16-bit direct right output
const HV_DIRECT_R_HI: usize = 0x67;
const HV_DATA: usize = 0x69; // 8-bit PCM data latch (Sound DMA / manual write)
const HV_CTRL: usize = 0x6A; // enable / sample-extension mode / volume shift
const HV_CHAN_CTRL: usize = 0x6B; // left/right output routing
const HV_ENABLE: u8 = 0x80; // HV_CTRL bit 7: HyperVoice active
const HV_EXT_MASK: u8 = 0x0C; // HV_CTRL bits 3-2: sample-extension mode
const HV_SHIFT_MASK: u8 = 0x03; // HV_CTRL bits 1-0: volume shift (0=100% … 3=12.5%)
const HV_LEFT: u8 = 0x40; // HV_CHAN_CTRL bit 6: route to left
const HV_RIGHT: u8 = 0x20; // HV_CHAN_CTRL bit 5: route to right

// ── Noise control register (port 0x8E) bits ──────────────────────────────────
const NOISE_GATE: u8 = 0x10; // 1 = noise generator running
const NOISE_RESET: u8 = 0x08; // 1 = reseed the LFSR (self-clearing)
const NOISE_TAP_MASK: u8 = 0x07; // tap-position selector

// ── Timing / bit-width constants ─────────────────────────────────────────────
const PITCH_MASK: u16 = 0x7FF; // channel pitch/period is 11-bit
const SWEEP_INTERVAL: u32 = 8192; // sound-clock ticks between sweep ticks
const LFSR_MASK: u16 = 0x7FFF; // noise LFSR is 15-bit
const LFSR_OUTPUT_BIT: u16 = 7; // LFSR bit XOR-ed with the selected tap
/// Channel index of the voice (PCM) channel.
const VOICE_CHANNEL: usize = 1;

/// One wave-table oscillator: walks a 32-entry / 4-bit-per-sample waveform held
/// in WRAM, advancing one entry every `2048 - pitch` sound-clock ticks.
#[derive(Clone, Copy)]
pub(crate) struct WaveChannel {
    /// Sound-clock ticks remaining until the next sample advance.
    period_counter: u16,
    /// Index of the current sample within the 32-entry waveform.
    sample_idx: u8,
    /// The 4-bit sample currently being output.
    sample: u8,
}

impl WaveChannel {
    pub(crate) const fn new() -> Self {
        Self {
            period_counter: 0,
            sample_idx: 0,
            sample: 0,
        }
    }

    /// Advance one sound-clock tick and return the current 4-bit sample.
    ///
    /// `pitch` is the raw 11-bit period register; the waveform's 16 bytes start
    /// at `wave_addr` in `wram` (two 4-bit samples per byte, low nibble first).
    pub(crate) fn tick(&mut self, pitch: u16, wram: &[u8], wave_addr: usize) -> u8 {
        self.period_counter = self.period_counter.saturating_sub(1);
        if self.period_counter == 0 {
            self.period_counter = 2048 - (pitch & PITCH_MASK);
            self.sample_idx = (self.sample_idx + 1) & 0x1F;
            let byte = wram[wave_addr + (self.sample_idx as usize) / 2];
            self.sample = (byte >> ((self.sample_idx & 1) * 4)) & 0x0F;
        }
        self.sample
    }
}

/// The WonderSwan audio processing unit.
///
/// Drive it with [`Apu::tick`] in lockstep with the CPU, then drain the
/// generated samples with [`Apu::samples`] / [`Apu::clear_samples`].
pub struct Apu {
    channels: [WaveChannel; 4],
    /// 15-bit noise LFSR (channel 4).
    lfsr: u16,
    /// `true` while the noise generator is overriding channel 4's output.
    noise_active: bool,
    /// Latched noise DAC level (`0x0F` or `0x00`).
    noise_output: u8,
    /// Sound-clock ticks remaining until the next noise LFSR step.
    noise_counter: u16,
    /// Sound-clock ticks accumulated toward the next sweep tick.
    sweep_counter: u32,
    /// Remaining sweep ticks before the next frequency adjustment.
    sweep_step: u8,
    /// Sound-clock ticks accumulated toward the next output sample.
    sample_accum: u32,
    /// Analog-reconstruction low-pass for the (mono) voice PCM channel, fed one
    /// sample per register write by [`Apu::write_voice`].
    voice_lp: VoiceLowPass,
    /// Latest reconstruction-filtered voice sample (signed, centred on 0).
    voice_level: i32,
    /// Interleaved stereo output samples (`L, R, L, R, …`).
    samples: Vec<i16>,
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

impl Apu {
    /// Sound clock: one tick per CPU cycle.
    pub const MASTER_CLOCK: u32 = 3_072_000;
    /// Output sample rate in Hz.
    pub const OUTPUT_SAMPLE_RATE: u32 = 24_000;
    /// Sound-clock ticks per output sample (`MASTER_CLOCK / OUTPUT_SAMPLE_RATE`).
    pub const CYCLES_PER_SAMPLE: u32 = Self::MASTER_CLOCK / Self::OUTPUT_SAMPLE_RATE;

    /// Create an APU in its power-on state (silent, empty sample buffer).
    pub fn new() -> Self {
        Self {
            channels: [WaveChannel::new(); 4],
            // Seed 0, matching StoicGoose: the noise feedback is an XNOR
            // (`1 ^ bit7 ^ bit_tap`), whose stuck state is all-ones, so a
            // zero seed is fine and reproduces the hardware sequence exactly.
            lfsr: 0,
            noise_active: false,
            noise_output: 0,
            noise_counter: 0,
            sweep_counter: 0,
            sweep_step: 0,
            sample_accum: 0,
            voice_lp: VoiceLowPass::new(),
            voice_level: 0,
            samples: Vec::new(),
        }
    }

    /// Feed one voice (PCM) register write (`port 0x89`, 8-bit) into the
    /// reconstruction low-pass. Called by the bus on every write while voice mode
    /// is active, so the filter sees the full write stream — games stream PCM by
    /// time-multiplexing two voices at roughly twice the audio rate, and only the
    /// per-write sequence (not the value sampled once per scanline) carries the
    /// signal cleanly. See [`VoiceLowPass`].
    pub fn write_voice(&mut self, sample: u8) {
        self.voice_level = self.voice_lp.filter(sample as i32 - 0x80);
    }

    /// Reset all channel and feature state, and clear the sample buffer.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// The interleaved stereo samples generated so far (`L, R, L, R, …`).
    pub fn samples(&self) -> &[i16] {
        &self.samples
    }

    /// Drop all buffered samples (call after the frontend has consumed them).
    pub fn clear_samples(&mut self) {
        self.samples.clear();
    }

    /// Advance the APU by `cycles` sound-clock ticks, reading waveform data from
    /// `wram` and channel registers from `ports`. Sweep and noise feed their
    /// results back into the register-visible `ports` (frequency `0x84`/`0x85`,
    /// noise LFSR `0x92`/`0x93`).
    ///
    /// `color` is the hardware-model gate for HyperVoice (a WonderSwan Color
    /// feature): when `false` the HyperVoice mix is skipped regardless of the
    /// register state, mirroring the bus's mono write-drop so read and write are
    /// both model-gated (as with the 8d internal-RAM window).
    ///
    /// # Panics
    /// Panics if `wram` is smaller than the addressed waveform area
    /// (`(ports[0x8F] << 6) + 64` bytes) or if `ports` is shorter than `0x100`.
    /// The real 16 KiB WRAM and 256-entry port file always satisfy this.
    pub fn tick(&mut self, cycles: u32, wram: &[u8], ports: &mut [u8], color: bool) {
        for _ in 0..cycles {
            self.step(wram, ports, color);
        }
    }

    /// Advance the APU by a single sound-clock tick.
    fn step(&mut self, wram: &[u8], ports: &mut [u8], color: bool) {
        let ctrl = ports[0x90];
        self.step_sweep(ctrl, ports);
        self.step_noise(ctrl, ports);

        let wave_base = (ports[SND_WAVE_BASE] as usize) << 6;
        let mut samples = [0u8; 4];
        for ch in 0..4 {
            if ctrl & CTRL_ENABLE[ch] != 0 {
                let pitch = pitch_of(ports, ch);
                samples[ch] = self.channels[ch].tick(pitch, wram, wave_base + ch * 16);
            }
        }
        // Channel 4 noise replaces the wave sample with the LFSR DAC level. The
        // channel-2 voice (8-bit PCM) is mixed separately below so it can be
        // low-pass filtered, so its wave sample is left untouched here.
        if self.noise_active {
            samples[3] = self.noise_output;
        }

        self.sample_accum += 1;
        if self.sample_accum >= Self::CYCLES_PER_SAMPLE {
            self.sample_accum = 0;
            let (wave_l, wave_r) = mix_waves(&samples, ctrl, ports);
            let (voice_l, voice_r) = self.mix_voice(ctrl, ports);
            // HyperVoice sums into the same output domain as the wave channels
            // (Mednafen `wswan/sound.c`). The wave+voice mix never saturates on
            // its own (4 channels × 15 × 15 × MIX_SCALE = 28 800 < i16 max), so
            // the clamp applies only once the voice and HyperVoice are added.
            let (hv_l, hv_r) = hypervoice_output(ports, color);
            let (left, right) =
                apply_output_control(wave_l + voice_l + hv_l, wave_r + voice_r + hv_r, ports);
            self.samples.push(left);
            self.samples.push(right);
        }
    }

    /// The channel-2 voice (8-bit PCM) contribution.
    ///
    /// Uses the reconstruction-filtered [`Apu::voice_level`] (fed one sample per
    /// register write by [`Apu::write_voice`]), routed to L/R per the voice-volume
    /// register (0x94) and scaled by [`MIX_SCALE`] × [`VOICE_GAIN`]. Returns
    /// `(0, 0)` when voice mode is off — the channel is then a plain wave
    /// oscillator handled by [`mix_waves`] — and resets the filter so stale state
    /// cannot click when the voice restarts.
    fn mix_voice(&mut self, ctrl: u8, ports: &[u8]) -> (i32, i32) {
        if ctrl & CTRL_VOICE == 0 {
            self.voice_lp.reset();
            self.voice_level = 0;
            return (0, 0);
        }
        let (l, r) = voice_route(self.voice_level, ports[SND_VOICE_VOL]);
        (l * MIX_SCALE * VOICE_GAIN, r * MIX_SCALE * VOICE_GAIN)
    }

    /// Tick the channel-3 frequency sweep (port 0x8C/0x8D), writing the adjusted
    /// pitch back to the channel-3 period register (0x84/0x85).
    fn step_sweep(&mut self, ctrl: u8, ports: &mut [u8]) {
        if ctrl & CTRL_SWEEP == 0 || ctrl & CTRL_ENABLE[2] == 0 {
            return;
        }
        self.sweep_counter += 1;
        if self.sweep_counter <= SWEEP_INTERVAL {
            return;
        }
        self.sweep_counter = 0;
        if self.sweep_step != 0 {
            self.sweep_step -= 1;
            return;
        }
        // Saturating: a zero sweep-time register must not underflow (WonderCrab
        // subtracts unconditionally).
        self.sweep_step = (ports[SND_SWEEP_TIME] & 0x1F).saturating_sub(1);
        let delta = ports[SND_SWEEP_VALUE] as i8 as i16;
        let old = pitch_of(ports, 2) as i16;
        let new = match old + delta {
            n if n > 2047 => 0,
            n if n < 0 => 2047,
            n => n,
        };
        let bytes = (new as u16 & PITCH_MASK).to_le_bytes();
        ports[SND_CH_PITCH + 4] = bytes[0];
        ports[SND_CH_PITCH + 5] = bytes[1];
    }

    /// Tick the channel-4 noise generator (port 0x8E), advancing the 15-bit
    /// LFSR and exposing it through the read-only 0x92/0x93 ports.
    fn step_noise(&mut self, ctrl: u8, ports: &mut [u8]) {
        if ctrl & CTRL_NOISE == 0 || ctrl & CTRL_ENABLE[3] == 0 {
            self.noise_active = false;
            return;
        }
        let noise_ctrl = ports[SND_NOISE];
        if noise_ctrl & NOISE_GATE == 0 {
            return; // noise gate closed: hold previous state
        }
        if self.noise_counter != 0 {
            self.noise_counter -= 1;
            return;
        }

        // Noise advances at the same period as the wave channels — `2048 - pitch`
        // sound-clock ticks — NOT masked to 9 bits.  StoicGoose's `SoundChannel4`
        // reloads its counter to 2048 and steps the LFSR when it reaches `pitch`,
        // giving period `2048 - pitch`.  An earlier `& 0x1FF` mask (also present in
        // WonderCrab) shortened the period up to 8× for low pitches, running the
        // noise far too fast and adding an audible high-pitched tone to drums.
        let pitch = pitch_of(ports, 3);
        self.noise_counter = 2048u16.wrapping_sub(pitch);

        // Reset request: reseed the LFSR (to 0, see `new`) and self-clear it.
        if noise_ctrl & NOISE_RESET != 0 {
            self.lfsr = 0;
            ports[SND_NOISE] = noise_ctrl & !NOISE_RESET;
            ports[SND_RANDOM] = 0;
            ports[SND_RANDOM + 1] = 0;
        }

        let tap = match noise_ctrl & NOISE_TAP_MASK {
            0 => 14,
            1 => 10,
            2 => 13,
            3 => 4,
            4 => 8,
            5 => 6,
            6 => 9,
            _ => 11,
        };
        // XNOR feedback (`1 ^ bit7 ^ bit_tap`), matching StoicGoose's
        // `SoundChannel4`.  An XOR here would invert the bit polarity and bias the
        // sequence toward ~25 % duty (a sparse, tonal pulse train); the XNOR keeps
        // it near 50 % so a drum reads as broadband noise rather than a pitched buzz.
        let feedback = (1 ^ (self.lfsr >> LFSR_OUTPUT_BIT) ^ (self.lfsr >> tap)) & 1;
        self.lfsr = ((self.lfsr << 1) | feedback) & LFSR_MASK;
        let bytes = self.lfsr.to_le_bytes();
        ports[SND_RANDOM] = bytes[0];
        ports[SND_RANDOM + 1] = bytes[1];

        // Channel-4 noise DAC: `0x0F`/`0x00` from the LFSR's low bit (the bit just
        // shifted in), kept in the 4-bit domain like the wave channels — exactly
        // StoicGoose's `(NoiseLfsr & 1) * 0x0F`.
        self.noise_output = if self.lfsr & 1 != 0 { 0x0F } else { 0x00 };
        self.noise_active = true;
    }
}

/// Read channel `ch`'s 11-bit pitch (period) register from `ports`.
fn pitch_of(ports: &[u8], ch: usize) -> u16 {
    let lo = ports[SND_CH_PITCH + ch * 2];
    let hi = ports[SND_CH_PITCH + ch * 2 + 1];
    u16::from_le_bytes([lo, hi]) & PITCH_MASK
}

/// Expand the 8-bit HyperVoice `data` latch to a signed ~11-bit sample per the
/// control register `ctrl` (port `0x6A`), following Mednafen's `wswan/sound.c`.
///
/// The extension mode (`ctrl` bits 3-2) selects how the 8-bit latch becomes a
/// 16-bit value, the volume shift (bits 1-0) scales it (0 = 100% … 3 = 12.5% via
/// a `<< (8 - shift)`), and the result is finally brought back to ~11 bits
/// (`>> 5`) so it lands in the same domain as the summed wave channels. The
/// intermediate is truncated to `i16` before the final shift, matching
/// Mednafen's `int16` assignment (mode 0/0xC can wrap large values negative).
fn hypervoice_sample(ctrl: u8, data: u8) -> i16 {
    let shift = (ctrl & HV_SHIFT_MASK) as i32;
    let expanded: i32 = match ctrl & HV_EXT_MASK {
        0x00 => (data as i32) << (8 - shift),            // unsigned
        0x04 => ((data as i32) | -0x100) << (8 - shift), // unsigned, negated
        0x08 => ((data as i8) as i32) << (8 - shift),    // signed
        _ => (data as i32) << 8,                         // 0x0C: raw, shift ignored
    };
    (expanded as i16) >> 5
}

/// HyperVoice's (`left`, `right`) contribution in the pre-clamp output domain,
/// or `(0, 0)` when disabled or on non-Color hardware. The sample is scaled by
/// [`MIX_SCALE`] to match the wave-channel mix (Mednafen sums HyperVoice into the
/// same accumulator as the four wave channels) and routed per `HV_CHAN_CTRL`
/// (port `0x6B`). `color` gates the whole feature so a stale enable bit left in
/// the port shadow cannot leak HyperVoice onto a machine modelled as mono.
fn hypervoice_output(ports: &[u8], color: bool) -> (i32, i32) {
    let ctrl = ports[HV_CTRL];
    if !color || ctrl & HV_ENABLE == 0 {
        return (0, 0);
    }
    let direct_l = hypervoice_direct_sample(ports, HV_DIRECT_L_LO, HV_DIRECT_L_HI);
    let direct_r = hypervoice_direct_sample(ports, HV_DIRECT_R_LO, HV_DIRECT_R_HI);
    if direct_l != 0 || direct_r != 0 {
        return (direct_l, direct_r);
    }
    let sample = hypervoice_sample(ctrl, ports[HV_DATA]) as i32 * MIX_SCALE;
    let chan = ports[HV_CHAN_CTRL];
    let left = if chan & HV_LEFT != 0 { sample } else { 0 };
    let right = if chan & HV_RIGHT != 0 { sample } else { 0 };
    (left, right)
}

fn hypervoice_direct_sample(ports: &[u8], lo: usize, hi: usize) -> i32 {
    i16::from_le_bytes([ports[lo], ports[hi]]) as i32
}

/// Mix the four channel samples into one interleaved stereo (`L`, `R`) frame,
/// applying per-channel volume and the channel-2 voice volume override.
/// Scale applied to the raw channel sum before converting to `i16`.
///
/// The raw sum of 4 wave channels peaks at `4 × 15 × 15 = 900`, which is only
/// 2.7 % of the i16 full-scale range (32767).  Multiplying by 32 brings the
/// maximum to 28 800 (≈ 88 % of full scale), giving adequate headroom while
/// mapping the hardware's output level into a range cpal can render at audible
/// volume via the standard `/32768.0` f32 conversion.
const MIX_SCALE: i32 = 32;

/// Extra gain applied to the voice (PCM) channel before the reconstruction
/// low-pass. Games stream PCM by zero-/silence-interleaving samples at twice the
/// audio rate (see [`VoiceLowPass`]); a unity-gain low-pass then recovers the
/// signal at half amplitude (the interpolation loss of 2× zero-stuffing —
/// `C, 0, C, 0` averages to `C/2`). This factor restores the intended level for
/// a single stream and correctly sums two simultaneously-multiplexed voices.
const VOICE_GAIN: i32 = 2;

/// Mix the wave channels into one interleaved stereo (`L`, `R`) frame (scaled by
/// [`MIX_SCALE`], pre-clamp). The channel-2 voice is excluded when voice mode is
/// active — it is filtered and summed separately in [`Apu::mix_voice`].
fn mix_waves(samples: &[u8; 4], ctrl: u8, ports: &[u8]) -> (i32, i32) {
    let mut left = 0i32;
    let mut right = 0i32;
    for (ch, &sample) in samples.iter().enumerate() {
        if ch == VOICE_CHANNEL && ctrl & CTRL_VOICE != 0 {
            continue; // voice handled by mix_voice
        }
        let vol = ports[SND_CH_VOL + ch];
        left += sample as i32 * (vol >> 4) as i32;
        right += sample as i32 * (vol & 0x0F) as i32;
    }
    (left * MIX_SCALE, right * MIX_SCALE)
}

/// Route the (already signed, reconstruction-filtered) voice sample `v` to the
/// (`L`, `R`) output per the voice-volume register (0x94): full / half / mute per
/// side. The signed convention (silence `0x80` → 0) matches Mednafen's
/// `wswan/sound.c`.
fn voice_route(v: i32, voice_vol: u8) -> (i32, i32) {
    let left = if voice_vol & 0x04 != 0 {
        v
    } else if voice_vol & 0x08 != 0 {
        v >> 1
    } else {
        0
    };
    let right = if voice_vol & 0x01 != 0 {
        v
    } else if voice_vol & 0x02 != 0 {
        v >> 1
    } else {
        0
    };
    (left, right)
}

/// Apply the WonderSwan output-control register (`0x91`) to the mixed sample.
///
/// With bit 7 clear the console speaker path mixes left and right to mono, then
/// attenuates by bits 2-1. With bit 7 set the headphone path preserves stereo.
fn apply_output_control(left: i32, right: i32, ports: &[u8]) -> (i16, i16) {
    let output_ctrl = ports[SND_OUTPUT_CTRL];
    if output_ctrl & 0x80 != 0 {
        return (clamp_i16(left), clamp_i16(right));
    }

    let shift = (output_ctrl >> 1) & 0x03;
    let mono = clamp_i16((left + right) >> shift);
    (mono, mono)
}

fn clamp_i16(sample: i32) -> i16 {
    sample.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

/// The voice (PCM) reconstruction filter: a 2-tap moving average over the raw
/// register-write stream (fed by [`Apu::write_voice`]).
///
/// Games stream PCM by *time-multiplexing* two voices onto the single voice
/// register, writing them alternately at twice the audio rate (e.g. *Last Alive*
/// writes one sample per visible scanline, ~10.9 kHz, ping-ponging a music voice
/// with a second — often silent — voice). That interleave puts the multiplex
/// component exactly at Nyquist of the write stream; a 2-tap average `y =
/// (x + x₋₁)/2` has a zero there, so it averages each `music, other` pair back
/// together — the reconstruction real hardware's analog output stage performs —
/// while leaving the audio band (and its treble) essentially untouched. A single
/// stream reconstructs at half amplitude, which [`VOICE_GAIN`] restores.
#[derive(Clone, Copy)]
struct VoiceLowPass {
    prev: i32,
}

impl VoiceLowPass {
    const fn new() -> Self {
        Self { prev: 0 }
    }

    fn reset(&mut self) {
        self.prev = 0;
    }

    /// Feed one written sample and return the 2-tap moving average.
    fn filter(&mut self, x: i32) -> i32 {
        let y = (x + self.prev) / 2;
        self.prev = x;
        y
    }
}
