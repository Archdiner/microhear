//! L3 pitch: YIN (de Cheveigné & Kawahara 2002) over the last two frames, 64 ms.
//!
//! Two frames because the lowest voice it looks for, 70 Hz, has a 229-sample period and YIN needs
//! a whole frame of comparisons at every lag. Cost is FRAME × max lag ≈ 117k multiply-adds, paid
//! only on frames loud enough to be voiced.

use crate::{FRAME, SAMPLE_RATE};

pub const MIN_HZ: f32 = 70.0;
pub const MAX_HZ: f32 = 1000.0;

const TAU_MIN: usize = (SAMPLE_RATE as f32 / MAX_HZ) as usize;
const TAU_MAX: usize = (SAMPLE_RATE as f32 / MIN_HZ) as usize;

/// The standard YIN dip threshold: the first lag under it is the period.
const DIP: f32 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pitch {
    pub hz: f32,
    /// YIN's cumulative-mean-normalised difference at the period: 0 is a perfect repeat, 1 is noise.
    pub aperiodicity: f32,
}

/// Scratch for one YIN call, kept by the caller so the hot path does not allocate.
#[derive(Debug, Clone)]
pub struct Yin {
    d: Vec<f32>,
}

impl Default for Yin {
    fn default() -> Self {
        Self {
            d: vec![0.0; TAU_MAX + 2],
        }
    }
}

impl Yin {
    /// `window` is the previous frame then this one: 2 × FRAME samples.
    pub fn estimate(&mut self, window: &[f32]) -> Pitch {
        debug_assert_eq!(window.len(), 2 * FRAME);
        let d = &mut self.d;
        d[0] = 1.0;
        let mut running = 0.0;
        for tau in 1..=TAU_MAX + 1 {
            let mut sum = 0.0;
            for j in 0..FRAME {
                let diff = window[j] - window[j + tau];
                sum += diff * diff;
            }
            running += sum;
            d[tau] = if running > 0.0 {
                sum * tau as f32 / running
            } else {
                1.0
            };
        }

        let mut best = (TAU_MIN..=TAU_MAX)
            .min_by(|&a, &b| d[a].total_cmp(&d[b]))
            .expect("lag range is not empty");
        if let Some(mut tau) = (TAU_MIN..=TAU_MAX).find(|&t| d[t] < DIP) {
            while tau < TAU_MAX && d[tau + 1] < d[tau] {
                tau += 1;
            }
            best = tau;
        }

        // A parabola through the dip and its neighbours puts the period between samples.
        let (a, b, c) = (d[best - 1], d[best], d[best + 1]);
        let denom = a - 2.0 * b + c;
        let shift = if denom.abs() > f32::EPSILON {
            (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        Pitch {
            hz: SAMPLE_RATE as f32 / (best as f32 + shift),
            aperiodicity: b.clamp(0.0, 1.0),
        }
    }
}

/// Semitones above 100 Hz: pitch on the scale people hear it, where an interval is a difference.
pub fn semitones(hz: f32) -> f32 {
    12.0 * (hz / 100.0).log2()
}

/// How bright a frame is, as the frequency of a sine with the same slope-to-level ratio.
///
/// `Σ(x[n]-x[n-1])² / Σx²` is exactly `4 sin²(πf/fs)` for a sine at `f`, so inverting it puts a
/// one-line spectral-tilt measure in hertz: a voice pushing energy above 1 kHz, as hot anger does,
/// reads higher. O(FRAME), no FFT.
pub fn brightness_hz(frame: &[f32], previous_sample: f32) -> f32 {
    let mut energy = 0.0;
    let mut slope = 0.0;
    let mut prev = previous_sample;
    for &s in frame {
        energy += s * s;
        slope += (s - prev) * (s - prev);
        prev = s;
    }
    if energy <= 0.0 {
        return 0.0;
    }
    let ratio = (slope / energy).min(4.0);
    SAMPLE_RATE as f32 / std::f32::consts::PI * (ratio.sqrt() / 2.0).asin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_signals::*;

    #[test]
    fn finds_sines_across_the_voice_range() {
        let mut yin = Yin::default();
        for hz in [80.0, 110.0, 220.0, 440.0, 880.0] {
            let p = yin.estimate(&sine(0.3, hz, 2 * FRAME));
            assert!((p.hz - hz).abs() / hz < 0.005, "{hz}: {p:?}");
            assert!(p.aperiodicity < 0.05, "{hz}: {p:?}");
        }
    }

    /// A voice's fundamental, not its loudest harmonic: the octave error YIN exists to avoid.
    #[test]
    fn finds_the_fundamental_of_a_harmonic_voice() {
        let p = Yin::default().estimate(&voice(0.3, 150.0, 1.0, 2 * FRAME));
        assert!((p.hz - 150.0).abs() < 1.0, "{p:?}");
    }

    #[test]
    fn noise_is_aperiodic() {
        let p = Yin::default().estimate(&noise(0.3, 2 * FRAME, 7));
        assert!(p.aperiodicity > 0.5, "{p:?}");
    }

    #[test]
    fn brightness_of_a_sine_is_its_frequency() {
        let s = sine(0.5, 1000.0, FRAME + 1);
        let b = brightness_hz(&s[1..], s[0]);
        assert!((b - 1000.0).abs() < 2.0, "{b}");
    }

    #[test]
    fn an_octave_is_twelve_semitones() {
        assert!((semitones(400.0) - semitones(200.0) - 12.0).abs() < 1e-4);
    }
}
