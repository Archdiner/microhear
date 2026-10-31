//! L5 tone: calm, cutesy or angry, from the last second of voice relative to the speaker.
//!
//! The cues, and why these two are separable even though both raise pitch:
//!
//! - cutesy (pet- and infant-directed speech): pitch well over the speaker's own, swinging
//!   sing-song, at an ordinary loudness.
//! - angry (hot anger): louder than the speaker and loud outright, with energy pushed above 1 kHz
//!   (brightness); pitch may rise too, which is why loudness gates it and not pitch.
//! - calm: neither.
//!
//! Each cue is a soft step around its threshold so the scores slide rather than flip, which is what
//! makes the thresholds tunable by ear on the page.

use crate::params::Params;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Calm,
    Cutesy,
    Angry,
}

impl Tone {
    pub fn as_str(self) -> &'static str {
        match self {
            Tone::Calm => "calm",
            Tone::Cutesy => "cutesy",
            Tone::Angry => "angry",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneScores {
    pub calm: f32,
    pub cutesy: f32,
    pub angry: f32,
}

impl ToneScores {
    pub fn tone(&self) -> Tone {
        if self.angry >= self.cutesy && self.angry >= self.calm {
            Tone::Angry
        } else if self.cutesy >= self.calm {
            Tone::Cutesy
        } else {
            Tone::Calm
        }
    }
}

/// The last second of voice, relative to the speaker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub rel_pitch_st: f32,
    pub spread_st: f32,
    pub rel_loud_db: f32,
    pub level_db: f32,
    pub brightness_hz: f32,
}

fn step(x: f32, threshold: f32, width: f32) -> f32 {
    1.0 / (1.0 + (-(x - threshold) / width).exp())
}

pub fn score(w: &Window, p: &Params) -> ToneScores {
    let raised = step(w.rel_pitch_st, p.cute_pitch_st, 1.0);
    let singsong = step(w.spread_st, p.cute_spread_st, 0.75);
    let shouting =
        step(w.rel_loud_db, p.angry_loud_db, 2.0) * step(w.level_db, p.angry_min_dbfs, 2.0);
    let bright = step(w.brightness_hz, p.angry_bright_hz, 150.0);

    let angry = shouting * (0.4 + 0.6 * bright);
    let cutesy = raised * (0.4 + 0.6 * singsong) * (1.0 - shouting);
    ToneScores {
        calm: 1.0 - angry.max(cutesy),
        cutesy,
        angry,
    }
}
