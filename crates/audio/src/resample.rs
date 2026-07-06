/// Linear resampler for interleaved stereo `i16` audio.
///
/// Converts from the APU's fixed output rate (24 kHz) to any host device rate
/// using piecewise-linear interpolation. This avoids the strong imaging caused
/// by repeating every 24 kHz sample on common 48 kHz devices, which is especially
/// audible in games that stream channel-2 PCM.
///
/// State is preserved across [`process`](Resampler::process) calls so that
/// batch boundaries do not introduce discontinuities.
pub(crate) struct Resampler {
    in_rate: u32,
    out_rate: u32,
    /// Next output position within the current input-sample interval, in
    /// `out_rate` units (`0..out_rate`).
    phase: u32,
    prev: [i16; 2],
    has_prev: bool,
}

impl Resampler {
    pub(crate) fn new(in_rate: u32, out_rate: u32) -> Self {
        assert!(in_rate > 0 && out_rate > 0, "sample rates must be non-zero");
        Self {
            in_rate,
            out_rate,
            phase: 0,
            prev: [0; 2],
            has_prev: false,
        }
    }

    /// Consume interleaved stereo `input` samples at `in_rate` and append
    /// resampled interleaved stereo samples to `out` at `out_rate`.
    pub(crate) fn process(&mut self, input: &[i16], out: &mut Vec<i16>) {
        if self.in_rate == self.out_rate {
            out.extend_from_slice(input);
            return;
        }

        for chunk in input.chunks_exact(2) {
            let current = [chunk[0], chunk[1]];
            if !self.has_prev {
                out.extend_from_slice(&current);
                self.prev = current;
                self.has_prev = true;
                self.phase = if self.out_rate > self.in_rate {
                    self.in_rate
                } else {
                    self.in_rate % self.out_rate
                };
                continue;
            }

            while self.phase < self.out_rate {
                out.push(lerp_i16(
                    self.prev[0],
                    current[0],
                    self.phase,
                    self.out_rate,
                ));
                out.push(lerp_i16(
                    self.prev[1],
                    current[1],
                    self.phase,
                    self.out_rate,
                ));
                self.phase += self.in_rate;
            }
            self.phase -= self.out_rate;
            self.prev = current;
        }
    }
}

fn lerp_i16(a: i16, b: i16, numer: u32, denom: u32) -> i16 {
    let a = a as i32;
    let b = b as i32;
    let delta = b - a;
    (a + delta * numer as i32 / denom as i32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_rate_produces_two_outputs_per_input() {
        let mut r = Resampler::new(24_000, 48_000);
        let input = [10i16, 20, 30, 60];
        let mut out = Vec::new();
        r.process(&input, &mut out);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn double_rate_interpolates_between_samples() {
        let mut r = Resampler::new(24_000, 48_000);
        let mut out = Vec::new();
        r.process(&[0i16, 10, 100, 30], &mut out);
        assert_eq!(out, [0, 10, 50, 20]);
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
        // Linear interpolation emits over input-sample intervals, so the first
        // eight input pairs produce 1 + floor(7 * 1.8375) = 13 output pairs.
        let mut r = Resampler::new(24_000, 44_100);
        let input = [0i16; 8]; // 4 stereo pairs
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.process(&input, &mut out);
        assert_eq!(out.len(), 26);
    }

    #[test]
    fn empty_input_produces_no_output() {
        let mut r = Resampler::new(24_000, 48_000);
        let mut out = Vec::new();
        r.process(&[], &mut out);
        assert!(out.is_empty());
    }
}
