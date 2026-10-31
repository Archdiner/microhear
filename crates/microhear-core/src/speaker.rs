//! L4: the speaker's own pitch and loudness, so everything after is relative to them.
//!
//! The first `baseline_frames` voiced frames set it, by median so a stray octave error does not.
//! After that it follows slowly (τ ≈ 20 s of voice), and only from frames that are neither raised
//! nor loud: a minute of baby talk should read as baby talk, not become the new normal.
//! In memory only; it does not survive a restart.

const FOLLOW: f32 = 1.0 / (31.25 * 20.0);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Baseline {
    /// Semitones over 100 Hz.
    pub pitch_st: f32,
    pub level_db: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Speaker {
    learning: Vec<(f32, f32)>,
    baseline: Option<Baseline>,
}

impl Speaker {
    pub fn baseline(&self) -> Option<Baseline> {
        self.baseline
    }

    /// Voiced frames heard toward the baseline, while it is still being learned.
    pub fn learned(&self) -> usize {
        self.learning.len()
    }

    pub fn reset(&mut self) {
        self.learning.clear();
        self.baseline = None;
    }

    /// Returns `true` on the frame the baseline is set.
    pub fn hear(&mut self, pitch_st: f32, level_db: f32, needed: usize, normal: bool) -> bool {
        match &mut self.baseline {
            None => {
                self.learning.push((pitch_st, level_db));
                if self.learning.len() < needed.max(1) {
                    return false;
                }
                let mut p: Vec<f32> = self.learning.iter().map(|x| x.0).collect();
                let mut l: Vec<f32> = self.learning.iter().map(|x| x.1).collect();
                self.baseline = Some(Baseline {
                    pitch_st: median(&mut p),
                    level_db: median(&mut l),
                });
                self.learning.clear();
                true
            }
            Some(b) => {
                if normal {
                    b.pitch_st += FOLLOW * (pitch_st - b.pitch_st);
                    b.level_db += FOLLOW * (level_db - b.level_db);
                }
                false
            }
        }
    }
}

fn median(v: &mut [f32]) -> f32 {
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}
