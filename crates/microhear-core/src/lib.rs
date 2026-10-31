//! The ear: a stream of 16 kHz mono samples in, one report per 32 ms frame out.
//!
//! The frame matches microduck's `SoundSentry` (512 samples at 16 kHz) so the two agree on what
//! a frame is when this crate feeds it.
//!
//! Layers, each reading the one above it:
//!
//! - L0 level: frame loudness in dBFS (here)
//! - L1 baseline + onset: dB over the room floor, fast rise, startle (next)
//! - L3 pitch, L4 speaker-relative, L5 tone: downstream

pub const SAMPLE_RATE: u32 = 16_000;

/// 32 ms at 16 kHz.
pub const FRAME: usize = 512;

/// What a frame of digital silence reports, so `log10(0)` never reaches a caller.
pub const SILENCE_DB: f32 = -120.0;

/// RMS level of `samples` in dBFS, where a full-scale square wave is 0 dB.
pub fn level_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return SILENCE_DB;
    }
    let mean_sq = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    if mean_sq <= 0.0 {
        return SILENCE_DB;
    }
    (10.0 * mean_sq.log10()).max(SILENCE_DB)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameReport {
    /// Index of the frame since the ear started; `index * FRAME / SAMPLE_RATE` is its time.
    pub index: u64,
    pub level_db: f32,
}

/// Cuts an arbitrary-sized stream into frames. The mic delivers 128 ms batches on the robot and
/// 2.7 ms ones in a browser worklet; neither lines up with a frame, so the remainder carries over.
#[derive(Debug, Default)]
pub struct Ear {
    pending: Vec<f32>,
    frames: u64,
}

impl Ear {
    pub fn new() -> Self {
        Self {
            pending: Vec::with_capacity(FRAME),
            frames: 0,
        }
    }

    pub fn push(&mut self, samples: &[f32], out: &mut Vec<FrameReport>) {
        for &s in samples {
            self.pending.push(s);
            if self.pending.len() == FRAME {
                out.push(FrameReport {
                    index: self.frames,
                    level_db: level_db(&self.pending),
                });
                self.frames += 1;
                self.pending.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(amplitude: f32, hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                amplitude * (2.0 * std::f32::consts::PI * hz * i as f32 / SAMPLE_RATE as f32).sin()
            })
            .collect()
    }

    #[test]
    fn silence_reports_the_floor_not_negative_infinity() {
        assert_eq!(level_db(&[0.0; FRAME]), SILENCE_DB);
        assert_eq!(level_db(&[]), SILENCE_DB);
    }

    /// A full-scale sine is -3.01 dBFS; halving the amplitude takes 6.02 dB off.
    #[test]
    fn sine_levels_match_the_textbook() {
        let full = level_db(&sine(1.0, 1000.0, FRAME * 4));
        let half = level_db(&sine(0.5, 1000.0, FRAME * 4));
        assert!((full - -3.01).abs() < 0.05, "{full}");
        assert!((full - half - 6.02).abs() < 0.05, "{}", full - half);
    }

    /// Batch size must not change the answer: the robot and the browser deliver different sizes.
    #[test]
    fn framing_is_independent_of_batch_size() {
        let signal = sine(0.3, 440.0, FRAME * 10 + 77);
        let mut whole = Vec::new();
        Ear::new().push(&signal, &mut whole);

        let mut ear = Ear::new();
        let mut chunked = Vec::new();
        for chunk in signal.chunks(128) {
            ear.push(chunk, &mut chunked);
        }

        assert_eq!(whole.len(), 10);
        assert_eq!(whole, chunked);
        assert_eq!(chunked.last().unwrap().index, 9);
    }
}
