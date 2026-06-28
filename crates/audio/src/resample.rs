/// Zero-order-hold resampler for interleaved stereo `i16` audio.
///
/// Converts from the APU's fixed output rate (24 kHz) to any host device rate
/// using sample-and-hold. High-quality band-limited resampling is a future
/// enhancement (Phase 9 audio quality pass).
///
/// State is preserved across [`process`](Resampler::process) calls so that
/// batch boundaries do not introduce discontinuities.
pub(crate) struct Resampler {
    in_rate: u32,
    out_rate: u32,
    /// Accumulated fractional output progress (0..in_rate).
    frac: u32,
}

impl Resampler {
    pub(crate) fn new(in_rate: u32, out_rate: u32) -> Self {
        assert!(in_rate > 0 && out_rate > 0, "sample rates must be non-zero");
        Self {
            in_rate,
            out_rate,
            frac: 0,
        }
    }

    /// Consume interleaved stereo `input` samples at `in_rate` and append
    /// resampled interleaved stereo samples to `out` at `out_rate`.
    pub(crate) fn process(&mut self, input: &[i16], out: &mut Vec<i16>) {
        for chunk in input.chunks_exact(2) {
            let l = chunk[0];
            let r = chunk[1];
            // For each input pair, advance the fractional accumulator by
            // out_rate.  Each time it crosses in_rate we emit one output pair.
            self.frac += self.out_rate;
            while self.frac >= self.in_rate {
                out.push(l);
                out.push(r);
                self.frac -= self.in_rate;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_rate_produces_two_outputs_per_input() {
        let mut r = Resampler::new(24_000, 48_000);
        let input = [10i16, 20];
        let mut out = Vec::new();
        r.process(&input, &mut out);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn double_rate_repeats_sample() {
        let mut r = Resampler::new(24_000, 48_000);
        let mut out = Vec::new();
        r.process(&[7i16, 8], &mut out);
        assert_eq!(out, [7, 8, 7, 8]);
    }

    #[test]
    fn same_rate_produces_one_output_per_input() {
        let mut r = Resampler::new(24_000, 24_000);
        let input = [1i16, 2, 3, 4];
        let mut out = Vec::new();
        r.process(&input, &mut out);
        assert_eq!(out, [1, 2, 3, 4]);
    }

    #[test]
    fn fractional_ratio_preserves_state_across_calls() {
        // 24 kHz → 44100 Hz: ratio ≈ 1.8375 output samples per input.
        // Over two calls of 4 pairs (8 pairs total) we expect floor(8 * 1.8375) = 14
        // output pairs = 28 i16 values.
        let mut r = Resampler::new(24_000, 44_100);
        let input = [0i16; 8]; // 4 stereo pairs
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.process(&input, &mut out);
        // 8 input pairs → 8 * 44100 / 24000 = 14.7 → 14 output pairs
        assert_eq!(out.len(), 28);
    }

    #[test]
    fn empty_input_produces_no_output() {
        let mut r = Resampler::new(24_000, 48_000);
        let mut out = Vec::new();
        r.process(&[], &mut out);
        assert!(out.is_empty());
    }
}
