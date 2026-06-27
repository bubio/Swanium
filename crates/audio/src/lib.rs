//! Audio sample buffering between the emulator core and the output device.
//!
//! The core generates interleaved stereo `i16` samples one frame at a time
//! (see [`swanium_core::system::System::audio_samples`]). The output device
//! (cpal, wired in a later step — see `docs/dev/DevelopmentPlan.md` Phase 7
//! 後続課題) pulls samples from a callback running on its own thread at an
//! independent rate. [`RingBuffer`] decouples the two: the producer pushes a
//! frame's worth of samples, the consumer pops whatever the device asks for,
//! and rate mismatches degrade gracefully (overruns drop the newest samples,
//! underruns are padded with silence) rather than blocking or panicking.
//!
//! The buffer itself carries no threading; the frontend wraps it in the
//! synchronisation primitive cpal requires. Keeping it a plain, single-threaded
//! data structure makes the fill/drain logic unit-testable without an audio
//! device.

/// A fixed-capacity FIFO of interleaved `i16` audio samples.
pub struct RingBuffer {
    buffer: Box<[i16]>,
    head: usize,
    len: usize,
}

impl RingBuffer {
    /// Create a ring buffer holding up to `capacity` samples.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "ring buffer capacity must be non-zero");
        Self {
            buffer: vec![0i16; capacity].into_boxed_slice(),
            head: 0,
            len: 0,
        }
    }

    /// Total number of samples the buffer can hold.
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// Number of samples currently queued.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no samples are queued.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Free space remaining before the buffer overruns.
    pub fn free(&self) -> usize {
        self.capacity() - self.len
    }

    /// Push samples onto the back of the queue, returning how many were stored.
    ///
    /// If `samples` does not fit, the trailing samples are dropped (an overrun);
    /// the returned count is less than `samples.len()` in that case.
    pub fn push(&mut self, samples: &[i16]) -> usize {
        let n = samples.len().min(self.free());
        for &sample in &samples[..n] {
            let tail = (self.head + self.len) % self.capacity();
            self.buffer[tail] = sample;
            self.len += 1;
        }
        n
    }

    /// Fill `out` with queued samples, padding any shortfall with silence.
    ///
    /// Returns the number of real (non-silence) samples written; the rest of
    /// `out` is zero-filled (an underrun).
    pub fn pop_into(&mut self, out: &mut [i16]) -> usize {
        let n = out.len().min(self.len);
        for slot in out.iter_mut().take(n) {
            *slot = self.buffer[self.head];
            self.head = (self.head + 1) % self.capacity();
            self.len -= 1;
        }
        for slot in out.iter_mut().skip(n) {
            *slot = 0;
        }
        n
    }

    /// Drop all queued samples.
    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_is_empty() {
        assert!(RingBuffer::new(8).is_empty());
    }

    #[test]
    fn capacity_reports_requested_size() {
        assert_eq!(RingBuffer::new(8).capacity(), 8);
    }

    #[test]
    #[should_panic(expected = "non-zero")]
    fn zero_capacity_panics() {
        let _ = RingBuffer::new(0);
    }

    #[test]
    fn push_reports_stored_count() {
        let mut rb = RingBuffer::new(8);
        assert_eq!(rb.push(&[1, 2, 3]), 3);
    }

    #[test]
    fn push_updates_len() {
        let mut rb = RingBuffer::new(8);
        rb.push(&[1, 2, 3]);
        assert_eq!(rb.len(), 3);
    }

    #[test]
    fn pop_returns_pushed_samples_in_order() {
        let mut rb = RingBuffer::new(8);
        rb.push(&[1, 2, 3]);
        let mut out = [0i16; 3];
        rb.pop_into(&mut out);
        assert_eq!(out, [1, 2, 3]);
    }

    #[test]
    fn pop_reports_real_sample_count() {
        let mut rb = RingBuffer::new(8);
        rb.push(&[1, 2, 3]);
        let mut out = [0i16; 5];
        assert_eq!(rb.pop_into(&mut out), 3);
    }

    #[test]
    fn underrun_pads_with_silence() {
        let mut rb = RingBuffer::new(8);
        rb.push(&[7, 8]);
        let mut out = [99i16; 4];
        rb.pop_into(&mut out);
        assert_eq!(out, [7, 8, 0, 0]);
    }

    #[test]
    fn overrun_drops_trailing_samples() {
        let mut rb = RingBuffer::new(2);
        assert_eq!(rb.push(&[1, 2, 3, 4]), 2);
    }

    #[test]
    fn overrun_keeps_earliest_samples() {
        let mut rb = RingBuffer::new(2);
        rb.push(&[1, 2, 3, 4]);
        let mut out = [0i16; 2];
        rb.pop_into(&mut out);
        assert_eq!(out, [1, 2]);
    }

    #[test]
    fn wraps_around_capacity() {
        let mut rb = RingBuffer::new(4);
        rb.push(&[1, 2, 3]);
        let mut out = [0i16; 2];
        rb.pop_into(&mut out); // consume 1, 2; head now at index 2
        rb.push(&[4, 5, 6]); // wraps past the end
        let mut rest = [0i16; 4];
        let real = rb.pop_into(&mut rest);
        assert_eq!(&rest[..real], &[3, 4, 5, 6]);
    }

    #[test]
    fn clear_empties_the_buffer() {
        let mut rb = RingBuffer::new(4);
        rb.push(&[1, 2]);
        rb.clear();
        assert!(rb.is_empty());
    }
}
