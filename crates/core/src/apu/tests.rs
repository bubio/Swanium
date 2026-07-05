//! Unit tests for the APU: wave-channel stepping, mixing, voice, sweep, and
//! noise. Expected sample values are computed analytically from the register
//! settings (one assertion per test, per Apollo Ch.5.1).

use super::*;

/// A blank port file and a 16 KiB WRAM, the shapes the APU expects.
fn blank() -> ([u8; 0x100], Vec<u8>) {
    ([0u8; 0x100], vec![0u8; 0x4000])
}

/// Write a 16-byte waveform (32× 4-bit samples) for channel `ch` at the
/// default wave base (`SND_WAVE_BASE` == 0 → address 0).
fn write_waveform(wram: &mut [u8], ch: usize, bytes: [u8; 16]) {
    let base = ch * 16;
    wram[base..base + 16].copy_from_slice(&bytes);
}

/// Set channel `ch`'s 11-bit pitch register.
fn set_pitch(ports: &mut [u8], ch: usize, pitch: u16) {
    let b = pitch.to_le_bytes();
    ports[SND_CH_PITCH + ch * 2] = b[0];
    ports[SND_CH_PITCH + ch * 2 + 1] = b[1];
}

// ── WaveChannel stepping ─────────────────────────────────────────────────────

#[test]
fn wave_channel_starts_silent() {
    let ch = WaveChannel::new();
    assert_eq!(ch.sample, 0);
}

#[test]
fn wave_channel_first_tick_outputs_index_one_sample() {
    // pitch 0x7FF → period 1 → advance every tick. byte0 = 0x21: idx0 low = 1,
    // idx1 high = 2. The first tick advances idx 0 → 1, so it outputs 2.
    let mut wram = vec![0u8; 0x4000];
    wram[0] = 0x21;
    let mut ch = WaveChannel::new();
    assert_eq!(ch.tick(0x7FF, &wram, 0), 2);
}

#[test]
fn wave_channel_walks_consecutive_samples() {
    // period 1: each tick advances one nibble. byte1 = 0x43 → idx2 = 3, idx3 = 4.
    let mut wram = vec![0u8; 0x4000];
    wram[0] = 0x21;
    wram[1] = 0x43;
    let mut ch = WaveChannel::new();
    ch.tick(0x7FF, &wram, 0); // idx1 → 2
    ch.tick(0x7FF, &wram, 0); // idx2 → 3
    assert_eq!(ch.tick(0x7FF, &wram, 0), 4); // idx3 → 4
}

#[test]
fn wave_channel_holds_sample_across_period() {
    // pitch 0x7FE → period 2: the sample is held for two ticks before advancing.
    let mut wram = vec![0u8; 0x4000];
    wram[0] = 0x21; // idx1 high = 2
    let mut ch = WaveChannel::new();
    ch.tick(0x7FE, &wram, 0); // tick1: advance to idx1 = 2
    assert_eq!(ch.tick(0x7FE, &wram, 0), 2); // tick2: still 2
}

#[test]
fn wave_channel_advances_after_period_elapses() {
    let mut wram = vec![0u8; 0x4000];
    wram[0] = 0x21;
    wram[1] = 0x43; // idx2 low = 3
    let mut ch = WaveChannel::new();
    ch.tick(0x7FE, &wram, 0); // idx1 = 2
    ch.tick(0x7FE, &wram, 0); // hold
    assert_eq!(ch.tick(0x7FE, &wram, 0), 3); // idx2 = 3
}

#[test]
fn wave_channel_wraps_after_32_samples() {
    // With period 1, 32 ticks return to idx 0. byte0 low nibble (idx0) = 1.
    let mut wram = vec![0u8; 0x4000];
    wram[0] = 0x21; // idx0 = 1
    let mut ch = WaveChannel::new();
    let mut out = 0;
    for _ in 0..32 {
        out = ch.tick(0x7FF, &wram, 0);
    }
    assert_eq!(out, 1); // idx wrapped 1→…→31→0
}

#[test]
fn wave_channel_reads_from_wave_address_offset() {
    // A non-zero base address selects a different channel's waveform slot.
    let mut wram = vec![0u8; 0x4000];
    wram[16] = 0x70; // idx1 high at slot 1 = 7
    let mut ch = WaveChannel::new();
    assert_eq!(ch.tick(0x7FF, &wram, 16), 7); // first tick → idx1 = 7
}

// ── pitch register decode ────────────────────────────────────────────────────

#[test]
fn pitch_of_reads_little_endian() {
    let mut ports = [0u8; 0x100];
    set_pitch(&mut ports, 0, 0x123);
    assert_eq!(pitch_of(&ports, 0), 0x123);
}

#[test]
fn pitch_of_masks_to_11_bits() {
    let mut ports = [0u8; 0x100];
    ports[SND_CH_PITCH] = 0xFF;
    ports[SND_CH_PITCH + 1] = 0xFF;
    assert_eq!(pitch_of(&ports, 0), 0x7FF);
}

// ── Sample buffer plumbing ───────────────────────────────────────────────────

#[test]
fn new_apu_has_no_samples() {
    let apu = Apu::new();
    assert!(apu.samples().is_empty());
}

#[test]
fn cycles_per_sample_is_128() {
    assert_eq!(Apu::CYCLES_PER_SAMPLE, 128);
}

#[test]
fn no_sample_before_cycles_per_sample() {
    let (mut ports, wram) = blank();
    let mut apu = Apu::new();
    apu.tick(127, &wram, &mut ports, false);
    assert!(apu.samples().is_empty());
}

#[test]
fn one_sample_after_cycles_per_sample() {
    let (mut ports, wram) = blank();
    let mut apu = Apu::new();
    apu.tick(128, &wram, &mut ports, false);
    assert_eq!(apu.samples().len(), STEREO_CHANNELS);
}

#[test]
fn clear_samples_empties_buffer() {
    let (mut ports, wram) = blank();
    let mut apu = Apu::new();
    apu.tick(128, &wram, &mut ports, false);
    apu.clear_samples();
    assert!(apu.samples().is_empty());
}

#[test]
fn reset_clears_samples() {
    let (mut ports, wram) = blank();
    let mut apu = Apu::new();
    apu.tick(256, &wram, &mut ports, false);
    apu.reset();
    assert!(apu.samples().is_empty());
}

// ── Channel enable / mixing ──────────────────────────────────────────────────

#[test]
fn disabled_channel_is_silent() {
    // No enable bits set: the emitted sample is zero on both sides.
    let (mut ports, mut wram) = blank();
    write_waveform(&mut wram, 0, [0x55; 16]);
    let mut apu = Apu::new();
    apu.tick(128, &wram, &mut ports, false);
    assert_eq!(apu.samples()[0], 0);
}

#[test]
fn enabled_channel_left_volume_scales_sample() {
    // ch1 enabled, pitch 0 (sample held at idx1 = high nibble of byte0 = 5),
    // left volume 3 → raw 5 × 3 = 15, scaled by MIX_SCALE (32) = 480.
    let (mut ports, mut wram) = blank();
    write_waveform(&mut wram, 0, {
        let mut w = [0u8; 16];
        w[0] = 0x50; // idx1 high = 5
        w
    });
    ports[0x90] = CTRL_ENABLE[0];
    ports[SND_CH_VOL] = 0x30; // L = 3, R = 0
    set_pitch(&mut ports, 0, 0);
    let mut apu = Apu::new();
    apu.tick(128, &wram, &mut ports, false);
    assert_eq!(apu.samples()[0], 480);
}

#[test]
fn enabled_channel_right_volume_is_independent() {
    let (mut ports, mut wram) = blank();
    write_waveform(&mut wram, 0, {
        let mut w = [0u8; 16];
        w[0] = 0x50; // idx1 = 5
        w
    });
    ports[0x90] = CTRL_ENABLE[0];
    ports[SND_CH_VOL] = 0x30; // L = 3, R = 0
    set_pitch(&mut ports, 0, 0);
    let mut apu = Apu::new();
    apu.tick(128, &wram, &mut ports, false);
    assert_eq!(apu.samples()[1], 0);
}

#[test]
fn mix_sums_two_channels_on_left() {
    // ch1 sample 5 × vol 1 + ch2 sample 5 × vol 2 = raw 15, scaled = 480.
    let mut ports = [0u8; 0x100];
    ports[0x90] = CTRL_ENABLE[0] | CTRL_ENABLE[1];
    ports[SND_CH_VOL] = 0x10; // ch1 L = 1
    ports[SND_CH_VOL + 1] = 0x20; // ch2 L = 2
    let (left, _) = mix(&[5, 5, 0, 0], ports[0x90], &ports);
    assert_eq!(left, 480);
}

// ── Voice (channel 2 PCM) ────────────────────────────────────────────────────

#[test]
fn voice_overrides_channel2_sample_with_port_0x89() {
    // VOICE set: channel 2's sample comes from port 0x89 (8-bit), voice volume
    // 0x04 → full left. 0x89 = 200 → raw 200, scaled by MIX_SCALE (32) = 6400.
    let (mut ports, wram) = blank();
    ports[0x90] = CTRL_ENABLE[1] | CTRL_VOICE;
    ports[SND_CH_VOL + 1] = 200; // 0x89 doubles as the PCM sample register
    ports[SND_VOICE_VOL] = 0x04; // left full
    let mut apu = Apu::new();
    apu.tick(128, &wram, &mut ports, false);
    assert_eq!(apu.samples()[0], 6400);
}

#[test]
fn voice_volume_half_left_halves_sample() {
    let (left, _) = voice_output(200, 0x08); // bit3: half left
    assert_eq!(left, 100);
}

#[test]
fn voice_volume_mute_left_is_zero() {
    let (left, _) = voice_output(200, 0x00);
    assert_eq!(left, 0);
}

#[test]
fn voice_volume_full_right_passes_sample() {
    let (_, right) = voice_output(200, 0x01); // bit0: full right
    assert_eq!(right, 200);
}

#[test]
fn voice_volume_half_right_halves_sample() {
    let (_, right) = voice_output(200, 0x02); // bit1: half right
    assert_eq!(right, 100);
}

// ── Noise (channel 4 LFSR) ───────────────────────────────────────────────────

#[test]
fn noise_advances_lfsr_into_random_port() {
    // NOISE + ENB4, gate open (0x8E bit4), tap 0. Seed 0, XNOR feedback
    // (1 ^ bit7 ^ bit14 = 1) → after one step the LFSR is 1, exposed at the low
    // random port 0x92.
    let (mut ports, wram) = blank();
    ports[0x90] = CTRL_NOISE | CTRL_ENABLE[3];
    ports[SND_NOISE] = 0x10; // gate open, tap 0
    let mut apu = Apu::new();
    apu.tick(1, &wram, &mut ports, false);
    assert_eq!(ports[SND_RANDOM], 1);
}

#[test]
fn noise_gate_closed_holds_lfsr() {
    // Gate bit (0x10) clear: the LFSR must not advance.
    let (mut ports, wram) = blank();
    ports[0x90] = CTRL_NOISE | CTRL_ENABLE[3];
    ports[SND_NOISE] = 0x00; // gate closed
    let mut apu = Apu::new();
    apu.tick(1, &wram, &mut ports, false);
    assert_eq!(ports[SND_RANDOM], 0);
}

#[test]
fn noise_reset_bit_self_clears() {
    // Reset request (bit 3) self-clears after the step.
    let (mut ports, wram) = blank();
    ports[0x90] = CTRL_NOISE | CTRL_ENABLE[3];
    ports[SND_NOISE] = 0x18; // gate open + reset request
    let mut apu = Apu::new();
    apu.tick(1, &wram, &mut ports, false);
    assert_eq!(ports[SND_NOISE] & 0x08, 0);
}

#[test]
fn noise_output_replaces_channel4_sample() {
    // With noise active, channel 4's mixed sample is the noise DAC level, not
    // the waveform. Seed 0, tap 0 → first XNOR bit 1 → DAC 0x0F. The waveform
    // (nibble 5) would instead give 5 × volume, so the result distinguishes them.
    let (mut ports, mut wram) = blank();
    write_waveform(&mut wram, 3, {
        let mut w = [0u8; 16];
        w[0] = 0x50; // idx1 high nibble = 5: a value the noise DAC (0x0F) won't equal
        w
    });
    ports[0x90] = CTRL_NOISE | CTRL_ENABLE[3];
    ports[SND_NOISE] = 0x10;
    ports[SND_CH_VOL + 3] = 0xF0; // left volume 15
    set_pitch(&mut ports, 3, 0x700); // noise step period 256 > 128
    let mut apu = Apu::new();
    apu.tick(128, &wram, &mut ports, false);
    assert_eq!(apu.samples()[0], 0x0F * 15 * 32); // noise DAC 0x0F, not waveform 5
}

#[test]
fn noise_period_is_unmasked_2048_minus_pitch() {
    // pitch 0 → period 2048: the LFSR steps once on the first tick (seed 0 → 1),
    // then must hold for 2048 ticks.  A 9-bit period mask (2048 & 0x1FF == 0)
    // would instead step it every tick, advancing the random port past 1.
    let (mut ports, wram) = blank();
    ports[0x90] = CTRL_NOISE | CTRL_ENABLE[3];
    ports[SND_NOISE] = NOISE_GATE; // gate open, tap 0
    set_pitch(&mut ports, 3, 0); // 2048 - 0 = 2048-tick period
    let mut apu = Apu::new();
    apu.tick(512, &wram, &mut ports, false); // well within one 2048-tick period
    assert_eq!(ports[SND_RANDOM], 1); // single XNOR step only: seed 0 → 1
}

// ── Sweep (channel 3) ────────────────────────────────────────────────────────

#[test]
fn sweep_adjusts_channel3_pitch_after_threshold() {
    // SWEEP + ENB3, sweep delta +5, sweep time 1. The sweep fires once the
    // 8192-tick counter overflows; pitch 0x100 → 0x105.
    let (mut ports, wram) = blank();
    ports[0x90] = CTRL_SWEEP | CTRL_ENABLE[2];
    ports[SND_SWEEP_VALUE] = 5;
    ports[SND_SWEEP_TIME] = 1;
    set_pitch(&mut ports, 2, 0x100);
    let mut apu = Apu::new();
    apu.tick(8193, &wram, &mut ports, false);
    assert_eq!(pitch_of(&ports, 2), 0x105);
}

#[test]
fn sweep_does_not_fire_before_threshold() {
    let (mut ports, wram) = blank();
    ports[0x90] = CTRL_SWEEP | CTRL_ENABLE[2];
    ports[SND_SWEEP_VALUE] = 5;
    ports[SND_SWEEP_TIME] = 1;
    set_pitch(&mut ports, 2, 0x100);
    let mut apu = Apu::new();
    apu.tick(8192, &wram, &mut ports, false);
    assert_eq!(pitch_of(&ports, 2), 0x100);
}

#[test]
fn sweep_negative_delta_decreases_pitch() {
    let (mut ports, wram) = blank();
    ports[0x90] = CTRL_SWEEP | CTRL_ENABLE[2];
    ports[SND_SWEEP_VALUE] = (-4i8) as u8;
    ports[SND_SWEEP_TIME] = 1;
    set_pitch(&mut ports, 2, 0x100);
    let mut apu = Apu::new();
    apu.tick(8193, &wram, &mut ports, false);
    assert_eq!(pitch_of(&ports, 2), 0xFC);
}

#[test]
fn sweep_disabled_leaves_pitch_unchanged() {
    let (mut ports, wram) = blank();
    ports[0x90] = CTRL_ENABLE[2]; // enabled but no SWEEP bit
    ports[SND_SWEEP_VALUE] = 5;
    ports[SND_SWEEP_TIME] = 1;
    set_pitch(&mut ports, 2, 0x100);
    let mut apu = Apu::new();
    apu.tick(8193, &wram, &mut ports, false);
    assert_eq!(pitch_of(&ports, 2), 0x100);
}

// ── HyperVoice (WonderSwan Color PCM) ─────────────────────────────────────────

#[test]
fn hypervoice_unsigned_mode_expands_and_rescales() {
    // mode 0 (unsigned), shift 0: 0x40 << 8 = 16384, then >> 5 = 512.
    assert_eq!(hypervoice_sample(0x00, 0x40), 512);
}

#[test]
fn hypervoice_signed_mode_yields_negative_sample() {
    // mode 0x8 (signed), shift 0: (i8)0x80 = -128, << 8 = -32768, >> 5 = -1024.
    assert_eq!(hypervoice_sample(0x08, 0x80), -1024);
}

#[test]
fn hypervoice_volume_shift_halves_sample() {
    // shift 1 scales by 50%: 0x40 << 7 = 8192, >> 5 = 256 (half of shift-0's 512).
    assert_eq!(hypervoice_sample(0x01, 0x40), 256);
}

#[test]
fn hypervoice_output_routes_left_only() {
    // Enabled, data 0x40 → sample 512 × MIX_SCALE (32) = 16384; routed to left.
    let mut ports = [0u8; 0x100];
    ports[HV_CTRL] = HV_ENABLE; // enable, mode 0, shift 0
    ports[HV_DATA] = 0x40;
    ports[HV_CHAN_CTRL] = HV_LEFT;
    assert_eq!(hypervoice_output(&ports, true), (16384, 0));
}

#[test]
fn hypervoice_output_silent_when_disabled() {
    // Data and routing set but the enable bit (0x80) is clear → no contribution.
    let mut ports = [0u8; 0x100];
    ports[HV_DATA] = 0x40;
    ports[HV_CHAN_CTRL] = HV_LEFT | HV_RIGHT;
    assert_eq!(hypervoice_output(&ports, true), (0, 0));
}

#[test]
fn hypervoice_negated_mode_matches_unsigned_after_truncation() {
    // mode 0x4 (unsigned, negated): `(data | -0x100) << 8` differs from unsigned
    // by exactly -0x10000, which vanishes on the `i16` truncation — so it lands
    // on the same value as mode 0 (0x40 → 512). Locks in Mednafen's behaviour.
    assert_eq!(hypervoice_sample(0x04, 0x40), 512);
}

#[test]
fn hypervoice_raw_mode_ignores_volume_shift() {
    // mode 0xC (raw): fixed `<< 8`, ignoring the shift bits. ctrl 0x0F sets shift
    // 3, yet 0x40 still yields 0x40 << 8 = 16384 → >> 5 = 512 (not the shifted 64).
    assert_eq!(hypervoice_sample(0x0F, 0x40), 512);
}

#[test]
fn hypervoice_output_gated_off_on_mono() {
    // Enabled and routed, but `color = false` (mono model) → no contribution,
    // even though the enable bit is set in the port shadow.
    let mut ports = [0u8; 0x100];
    ports[HV_CTRL] = HV_ENABLE;
    ports[HV_DATA] = 0x40;
    ports[HV_CHAN_CTRL] = HV_LEFT | HV_RIGHT;
    assert_eq!(hypervoice_output(&ports, false), (0, 0));
}

#[test]
fn hypervoice_adds_into_output_sample() {
    // No wave channels; HyperVoice enabled, routed to both sides. After one
    // output-sample interval the pushed left sample is the HyperVoice value.
    let (mut ports, wram) = blank();
    ports[HV_CTRL] = HV_ENABLE; // mode 0, shift 0
    ports[HV_DATA] = 0x40; // → sample 512 × 32 = 16384
    ports[HV_CHAN_CTRL] = HV_LEFT | HV_RIGHT;
    let mut apu = Apu::new();
    apu.tick(128, &wram, &mut ports, true);
    assert_eq!(apu.samples()[0], 16384);
}
