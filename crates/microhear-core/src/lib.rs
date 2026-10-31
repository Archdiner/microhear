//! The ear: a stream of 16 kHz mono samples in, one report per 32 ms frame out, and events when
//! something happens.
//!
//! The frame matches microduck's `SoundSentry` (512 samples at 16 kHz) so the two agree on what
//! a frame is when this crate feeds it.
//!
//! Two paths, the way a mammal's hearing splits:
//!
//! - reflex (L1): level against the room floor, and a startle on a sudden loud rise. Decided
//!   from the frame alone, the brainstem's shortcut.
//! - interpretation (L2–L5): is it voice, at what pitch, relative to this speaker, in what tone.
//!   Needs a baseline and a second of context, the cortex's slower read.
//!
//! - L0 level: frame loudness in dBFS
//! - L1 floor + onset + startle: dB over the room floor ([`floor`] mirrors the sentry's)
//! - L2 voicing: loud enough over the floor and periodic enough to have a pitch
//! - L3 pitch: YIN ([`pitch`])
//! - L4 relative: pitch and loudness against this speaker ([`speaker`])
//! - L5 tone: calm, cutesy, angry ([`tone`])
//!
//! State per ear is a few kilobytes and allocation-free once warm: two frames of raw audio for
//! pitch, one second of per-frame features for tone, nothing longer.

pub mod floor;
pub mod params;
pub mod pitch;
pub mod speaker;
pub mod tone;

use std::collections::VecDeque;

pub use params::Params;
pub use pitch::Pitch;
pub use speaker::Baseline;
pub use tone::{Tone, ToneScores};

pub const SAMPLE_RATE: u32 = 16_000;

/// 32 ms at 16 kHz.
pub const FRAME: usize = 512;

/// What a frame of digital silence reports, so `log10(0)` never reaches a caller.
pub const SILENCE_DB: f32 = -120.0;

/// Frames per second, for turning seconds into frames.
const FPS: f32 = SAMPLE_RATE as f32 / FRAME as f32;

/// The petting mute outlasts the petting by ~1 s, as the sentry's does.
const PETTING_HOLD_FRAMES: u32 = 31;

/// One second of frames: the context tone is read over.
const TONE_FRAMES: usize = 31;
/// Voiced frames that context needs before a tone is called.
const TONE_MIN_VOICED: usize = 8;

/// Voiced frames in a row that start an utterance, and unvoiced ones that end it: three is 96 ms,
/// past a click; eight is 256 ms, longer than the gap between syllables.
const UTTERANCE_START: u32 = 3;
const UTTERANCE_END: u32 = 8;

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

fn rms_db(rms: f32) -> f32 {
    if rms <= 0.0 {
        SILENCE_DB
    } else {
        (20.0 * rms.log10()).max(SILENCE_DB)
    }
}

/// Seconds since the ear started at the start of frame `index`.
pub fn frame_time_s(index: u64) -> f64 {
    index as f64 * FRAME as f64 / SAMPLE_RATE as f64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loudness {
    Quiet,
    Normal,
    Loud,
}

impl Loudness {
    fn of(rel_db: f32, p: &Params) -> Self {
        if rel_db >= p.loud_db {
            Loudness::Loud
        } else if rel_db <= p.quiet_db {
            Loudness::Quiet
        } else {
            Loudness::Normal
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Loudness::Quiet => "quiet",
            Loudness::Normal => "normal",
            Loudness::Loud => "loud",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameReport {
    /// Index of the frame since the ear started; [`frame_time_s`] is its time.
    pub index: u64,
    pub level_db: f32,
    /// The room floor this frame was judged against.
    pub floor_db: f32,
    pub over_floor_db: f32,
    /// dB climbed over the quieter of the two frames before: how sudden.
    pub rise_db: f32,
    /// Muted for petting: no events, no learning, while this is set.
    pub petting: bool,
    /// Looked for only on frames loud enough to be voiced.
    pub pitch: Option<Pitch>,
    pub voiced: bool,
    pub brightness_hz: f32,
    pub rel_pitch_st: Option<f32>,
    pub rel_loud_db: Option<f32>,
    pub loudness: Option<Loudness>,
    pub tone: Option<ToneScores>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EventKind {
    /// A sound rose out of the room: the cue to turn toward it.
    Onset {
        over_floor_db: f32,
    },
    /// A sudden, loud rise: the reflex.
    Startle {
        over_floor_db: f32,
        rise_db: f32,
    },
    UtteranceStart {
        start: u64,
    },
    UtteranceEnd {
        start: u64,
        end: u64,
        tone: Option<Tone>,
        rel_pitch_st: Option<f32>,
        rel_loud_db: Option<f32>,
        loudness: Option<Loudness>,
    },
    BaselineSet {
        hz: f32,
        level_db: f32,
    },
}

/// `index` is the frame on which the ear knew, so `index - start` on an utterance is how long it
/// took to be sure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    pub index: u64,
    pub kind: EventKind,
}

#[derive(Debug, Clone)]
enum Floor {
    Mirror(floor::SentryFloor),
    /// Handed over by the duck's `SoundSentry`, RMS.
    External(f32),
}

#[derive(Debug, Clone, Copy)]
struct Voiced {
    pitch_st: f32,
    level_db: f32,
    richness: f32,
}

#[derive(Debug, Clone, Default)]
struct Utterance {
    active: bool,
    start: u64,
    last_voiced: u64,
    voiced_run: u32,
    unvoiced_run: u32,
    scores: (f32, f32, f32),
    scored: u32,
    rel: (f32, f32),
    related: u32,
}

#[derive(Debug, Clone)]
pub struct Ear {
    pub params: Params,
    pending: Vec<f32>,
    /// The previous frame then this one, for pitch.
    window: Vec<f32>,
    frames: u64,
    floor: Floor,
    levels: [f32; 2],
    petting: bool,
    petting_hold: u32,
    refractory: u32,
    /// A sudden loud rise from the frame before, waiting on this frame to say whether it is noise.
    startle_candidate: Option<(f32, f32)>,
    above_onset: bool,
    yin: pitch::Yin,
    speaker: speaker::Speaker,
    recent: VecDeque<Option<Voiced>>,
    utterance: Utterance,
}

impl Default for Ear {
    fn default() -> Self {
        Self::new()
    }
}

impl Ear {
    pub fn new() -> Self {
        Self {
            params: Params::default(),
            pending: Vec::with_capacity(FRAME),
            window: vec![0.0; 2 * FRAME],
            frames: 0,
            floor: Floor::Mirror(floor::SentryFloor::default()),
            levels: [SILENCE_DB; 2],
            petting: false,
            petting_hold: 0,
            refractory: 0,
            startle_candidate: None,
            above_onset: false,
            yin: pitch::Yin::default(),
            speaker: speaker::Speaker::default(),
            recent: VecDeque::with_capacity(TONE_FRAMES + 1),
            utterance: Utterance::default(),
        }
    }

    /// What the petting classifier says now. On the duck: `detector.is_petting()` before each push.
    pub fn set_petting(&mut self, petting: bool) {
        self.petting = petting;
    }

    /// Hear against the duck's `SoundSentry` floor (RMS) from now on instead of the mirror.
    pub fn set_floor(&mut self, rms: f32) {
        self.floor = Floor::External(rms);
    }

    pub fn speaker(&self) -> &speaker::Speaker {
        &self.speaker
    }

    pub fn reset_speaker(&mut self) {
        self.speaker.reset();
    }

    pub fn push(&mut self, samples: &[f32], out: &mut Vec<FrameReport>, events: &mut Vec<Event>) {
        for &s in samples {
            self.pending.push(s);
            if self.pending.len() == FRAME {
                let report = self.frame(events);
                out.push(report);
                self.pending.clear();
            }
        }
    }

    fn frame(&mut self, events: &mut Vec<Event>) -> FrameReport {
        let p = self.params;
        let index = self.frames;
        self.frames += 1;

        let first = self.window[2 * FRAME - 1];
        self.window.copy_within(FRAME.., 0);
        self.window[FRAME..].copy_from_slice(&self.pending);
        let frame = &self.window[FRAME..];

        let level = level_db(frame);
        if self.petting {
            self.petting_hold = PETTING_HOLD_FRAMES;
        } else {
            self.petting_hold = self.petting_hold.saturating_sub(1);
        }
        let muted = self.petting || self.petting_hold > 0;

        let floor_rms = match &mut self.floor {
            Floor::Mirror(sentry) => {
                let rule = if p.floor_min_tracker >= 0.5 {
                    floor::Rule::FastDownSlowUp
                } else {
                    floor::Rule::Mean
                };
                sentry.frame(10f32.powf(level / 20.0), muted, rule)
            }
            Floor::External(rms) => *rms,
        };
        let floor_db = rms_db(floor_rms);
        let over = level - floor_db;
        let rise = level - self.levels[0].min(self.levels[1]);
        self.levels = [self.levels[1], level];

        // L2, L3: voice and its pitch, only where there is enough sound to have one. First, because
        // the startle reads periodicity too: a clap is noise, a voice starting is not.
        let brightness_hz = pitch::brightness_hz(frame, first);
        let pitch = (over >= p.voiced_db).then(|| self.yin.estimate(&self.window));
        let voiced = !muted && pitch.is_some_and(|x| x.aperiodicity <= p.voiced_aperiodicity);

        // L1, the reflex: from this frame alone.
        if !self.above_onset && over >= p.onset_db {
            self.above_onset = true;
            if !muted {
                events.push(Event {
                    index,
                    kind: EventKind::Onset {
                        over_floor_db: over,
                    },
                });
            }
        } else if self.above_onset && over < p.voiced_db {
            self.above_onset = false;
        }
        // A startle is decided a frame late, on purpose. The frame a sound starts in is aperiodic
        // whatever the sound is (the pitch window straddles silence), so the frame after says
        // whether it is noise, a clap or a bang, or a voice that started loud. A short impulse
        // that has already died away counts as noise.
        self.refractory = self.refractory.saturating_sub(1);
        if let Some((cand_over, cand_rise)) = self.startle_candidate.take()
            && !muted
            && self.refractory == 0
            && pitch.is_none_or(|x| x.aperiodicity >= p.startle_min_aperiodicity)
        {
            self.refractory = (p.startle_refractory_s * FPS).round() as u32;
            events.push(Event {
                index,
                kind: EventKind::Startle {
                    over_floor_db: cand_over,
                    rise_db: cand_rise,
                },
            });
        }
        if !muted
            && self.refractory == 0
            && level >= p.startle_min_dbfs
            && over >= p.startle_db
            && rise >= p.startle_rise_db
        {
            self.startle_candidate = Some((over, rise));
        }

        // L4: against this speaker.
        let heard = voiced.then(|| {
            let hz = pitch.expect("voiced has a pitch").hz;
            Voiced {
                pitch_st: pitch::semitones(hz),
                level_db: level,
                richness: brightness_hz / hz,
            }
        });
        if let Some(v) = heard {
            let normal = self.speaker.baseline().is_some_and(|b| {
                (v.pitch_st - b.pitch_st).abs() < p.cute_pitch_st
                    && (v.level_db - b.level_db).abs() < p.angry_loud_db
            });
            if self
                .speaker
                .hear(v.pitch_st, v.level_db, p.baseline_frames as usize, normal)
            {
                let b = self.speaker.baseline().expect("just set");
                events.push(Event {
                    index,
                    kind: EventKind::BaselineSet {
                        hz: 100.0 * 2f32.powf(b.pitch_st / 12.0),
                        level_db: b.level_db,
                    },
                });
            }
        }
        let baseline = self.speaker.baseline();
        let rel_pitch_st = heard.zip(baseline).map(|(v, b)| v.pitch_st - b.pitch_st);
        let rel_loud_db = heard.zip(baseline).map(|(v, b)| v.level_db - b.level_db);

        // L5: a second of context.
        self.recent.push_back(heard);
        if self.recent.len() > TONE_FRAMES {
            self.recent.pop_front();
        }
        let tone = baseline
            .and_then(|b| self.window_stats(b))
            .map(|w| tone::score(&w, &p));

        self.track_utterance(
            index,
            voiced,
            muted,
            tone,
            rel_pitch_st,
            rel_loud_db,
            events,
        );

        FrameReport {
            index,
            level_db: level,
            floor_db,
            over_floor_db: over,
            rise_db: rise,
            petting: muted,
            pitch,
            voiced,
            brightness_hz,
            rel_pitch_st,
            rel_loud_db,
            loudness: rel_loud_db.map(|r| Loudness::of(r, &p)),
            tone,
        }
    }

    fn window_stats(&self, b: Baseline) -> Option<tone::Window> {
        let voiced = self.recent.iter().flatten();
        let n = voiced.clone().count();
        if n < TONE_MIN_VOICED {
            return None;
        }
        let nf = n as f32;
        let mean = |f: fn(&Voiced) -> f32| voiced.clone().map(f).sum::<f32>() / nf;
        let pitch = mean(|v| v.pitch_st);
        let spread = (voiced
            .clone()
            .map(|v| (v.pitch_st - pitch).powi(2))
            .sum::<f32>()
            / nf)
            .sqrt();
        let level = mean(|v| v.level_db);
        Some(tone::Window {
            rel_pitch_st: pitch - b.pitch_st,
            spread_st: spread,
            rel_loud_db: level - b.level_db,
            level_db: level,
            richness: mean(|v| v.richness),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn track_utterance(
        &mut self,
        index: u64,
        voiced: bool,
        muted: bool,
        tone: Option<ToneScores>,
        rel_pitch_st: Option<f32>,
        rel_loud_db: Option<f32>,
        events: &mut Vec<Event>,
    ) {
        let u = &mut self.utterance;
        if muted {
            // A pet is not a sentence: whatever was under way is dropped, not reported.
            *u = Utterance::default();
            return;
        }
        if voiced {
            u.voiced_run += 1;
            u.unvoiced_run = 0;
            u.last_voiced = index;
            if !u.active && u.voiced_run >= UTTERANCE_START {
                u.active = true;
                u.start = index + 1 - UTTERANCE_START as u64;
                events.push(Event {
                    index,
                    kind: EventKind::UtteranceStart { start: u.start },
                });
            }
            if u.active {
                if let Some(t) = tone {
                    u.scores.0 += t.calm;
                    u.scores.1 += t.cutesy;
                    u.scores.2 += t.angry;
                    u.scored += 1;
                }
                if let (Some(rp), Some(rl)) = (rel_pitch_st, rel_loud_db) {
                    u.rel.0 += rp;
                    u.rel.1 += rl;
                    u.related += 1;
                }
            }
            return;
        }
        u.voiced_run = 0;
        u.unvoiced_run += 1;
        if u.active && u.unvoiced_run >= UTTERANCE_END {
            let n = u.scored.max(1) as f32;
            let tone = (u.scored > 0).then(|| {
                ToneScores {
                    calm: u.scores.0 / n,
                    cutesy: u.scores.1 / n,
                    angry: u.scores.2 / n,
                }
                .tone()
            });
            let r = u.related.max(1) as f32;
            let rel_pitch_st = (u.related > 0).then_some(u.rel.0 / r);
            let rel_loud_db = (u.related > 0).then_some(u.rel.1 / r);
            events.push(Event {
                index,
                kind: EventKind::UtteranceEnd {
                    start: u.start,
                    end: u.last_voiced + 1,
                    tone,
                    rel_pitch_st,
                    rel_loud_db,
                    loudness: rel_loud_db.map(|x| Loudness::of(x, &self.params)),
                },
            });
            *u = Utterance::default();
        }
    }
}

#[cfg(test)]
pub(crate) mod test_signals {
    use crate::SAMPLE_RATE;

    pub fn sine(amplitude: f32, hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                amplitude * (2.0 * std::f32::consts::PI * hz * i as f32 / SAMPLE_RATE as f32).sin()
            })
            .collect()
    }

    /// A buzzy voice: harmonics up to 4 kHz falling as `1/k^tilt` (1 is mellow, 0.5 is bright),
    /// following `hz(t)`, scaled to the RMS of a sine of `amplitude`.
    pub fn voice_with(amplitude: f32, tilt: f32, n: usize, hz: impl Fn(f32) -> f32) -> Vec<f32> {
        let mut phase = 0.0f32;
        let mut out: Vec<f32> = (0..n)
            .map(|i| {
                let f = hz(i as f32 / SAMPLE_RATE as f32);
                phase += 2.0 * std::f32::consts::PI * f / SAMPLE_RATE as f32;
                let harmonics = (4000.0 / f) as usize;
                (1..=harmonics)
                    .map(|k| (k as f32 * phase).sin() / (k as f32).powf(tilt))
                    .sum()
            })
            .collect();
        let rms = (out.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
        let scale = amplitude / std::f32::consts::SQRT_2 / rms;
        out.iter_mut().for_each(|s| *s *= scale);
        out
    }

    pub fn voice(amplitude: f32, hz: f32, tilt: f32, n: usize) -> Vec<f32> {
        voice_with(amplitude, tilt, n, |_| hz)
    }

    /// Uniform noise in ±`amplitude` from a fixed seed: the same room every run.
    pub fn noise(amplitude: f32, n: usize, seed: u32) -> Vec<f32> {
        let mut x = seed.wrapping_mul(2_654_435_761).max(1);
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                amplitude * (x as f32 / u32::MAX as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    /// A clap: a burst of noise dying away over ~15 ms.
    pub fn clap(peak: f32, n: usize) -> Vec<f32> {
        noise(peak, n, 99)
            .into_iter()
            .enumerate()
            .map(|(i, s)| s * (-(i as f32) / (0.015 * SAMPLE_RATE as f32)).exp())
            .collect()
    }

    /// Lay `sound` over `into` starting at `at_s` seconds.
    pub fn mix(into: &mut [f32], at_s: f32, sound: &[f32]) {
        let start = (at_s * SAMPLE_RATE as f32) as usize;
        for (o, s) in into[start..].iter_mut().zip(sound) {
            *o += s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_signals::*;
    use super::*;

    fn hear(signal: &[f32]) -> (Vec<FrameReport>, Vec<Event>) {
        let mut ear = Ear::new();
        let (mut frames, mut events) = (Vec::new(), Vec::new());
        ear.push(signal, &mut frames, &mut events);
        (frames, events)
    }

    fn seconds(s: f32) -> usize {
        (s * SAMPLE_RATE as f32) as usize
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
    fn hearing_is_independent_of_batch_size() {
        let mut signal = noise(0.001, seconds(4.0) + 77, 1);
        mix(&mut signal, 1.0, &clap(0.8, 2000));
        mix(&mut signal, 2.0, &voice(0.05, 180.0, 1.0, seconds(1.5)));
        let whole = hear(&signal);

        let mut ear = Ear::new();
        let (mut frames, mut events) = (Vec::new(), Vec::new());
        for chunk in signal.chunks(128) {
            ear.push(chunk, &mut frames, &mut events);
        }
        assert_eq!(whole.0.len(), seconds(4.0) / FRAME);
        assert_eq!(whole, (frames, events));
    }

    /// The reflex fires one frame after the clap lands: the frame it needs to know it is noise.
    #[test]
    fn a_clap_startles_one_frame_after_it_lands() {
        let mut signal = noise(0.001, seconds(3.0), 1);
        mix(&mut signal, 2.0, &clap(0.8, 2000));
        let (_, events) = hear(&signal);
        let startles: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Startle { .. }))
            .collect();
        // 2.0 s is sample 32000, inside frame 62 (31744..32256); confirmed on 63.
        assert_eq!(startles.len(), 1, "{events:?}");
        assert_eq!(startles[0].index, 63);
        assert!(
            events
                .iter()
                .any(|e| e.index == 62 && matches!(e.kind, EventKind::Onset { .. }))
        );
    }

    /// The same level, arriving over a second, is a sound and not a fright.
    #[test]
    fn a_swell_is_an_onset_not_a_startle() {
        let n = seconds(4.0);
        let mut signal = noise(0.001, n, 1);
        let tone = sine(0.3, 300.0, seconds(1.5));
        let ramp: Vec<f32> = tone
            .iter()
            .enumerate()
            .map(|(i, s)| s * (i as f32 / seconds(1.0) as f32).min(1.0).powi(3))
            .collect();
        mix(&mut signal, 2.0, &ramp);
        let (_, events) = hear(&signal);
        assert!(
            events
                .iter()
                .any(|e| matches!(e.kind, EventKind::Onset { .. }))
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e.kind, EventKind::Startle { .. })),
            "{events:?}"
        );
    }

    /// Claps while petting and inside the ~1 s hangover are the pet; the one after is a clap.
    #[test]
    fn petting_mutes_the_reflex_and_its_hangover() {
        let mut signal = noise(0.001, seconds(4.5), 1);
        for at in [2.0, 3.2, 3.8] {
            mix(&mut signal, at, &clap(0.8, 2000));
        }
        let mut ear = Ear::new();
        let (mut frames, mut events) = (Vec::new(), Vec::new());
        let (petted, after) = signal.split_at(seconds(2.5));
        ear.set_petting(true);
        ear.push(petted, &mut frames, &mut events);
        ear.set_petting(false);
        ear.push(after, &mut frames, &mut events);
        let startles: Vec<u64> = events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Startle { .. }))
            .map(|e| e.index)
            .collect();
        // 3.8 s is sample 60800, inside frame 118; confirmed on 119.
        assert_eq!(startles, [119]);
    }

    /// The scene every later change is checked against: event timings, frozen. A behaviour change
    /// that moves one of these is deliberate or it is a bug.
    #[test]
    fn scene_timeline() {
        // 10.24 s (320 frames) of room first, so the floor has settled as it has on a duck that
        // has been on a while; the scene starts at frame 320.
        let t0 = 10.24;
        let base = 150.0;
        let mut signal = noise(0.001, seconds(t0 + 12.0), 3);
        mix(&mut signal, t0 + 1.0, &clap(0.8, 2000));
        mix(&mut signal, t0 + 2.0, &voice(0.05, base, 1.0, seconds(2.0)));
        let up7 = base * 2f32.powf(7.0 / 12.0);
        mix(
            &mut signal,
            t0 + 5.0,
            &voice_with(0.05, 1.0, seconds(1.2), |t| {
                up7 * 2f32.powf(3.0 * (2.0 * std::f32::consts::PI * 3.0 * t).sin() / 12.0)
            }),
        );
        let up2 = base * 2f32.powf(2.0 / 12.0);
        mix(&mut signal, t0 + 7.5, &voice(0.2, up2, 0.5, seconds(1.2)));
        mix(
            &mut signal,
            t0 + 10.0,
            &voice(0.016, base, 1.0, seconds(1.2)),
        );

        let (_, events) = hear(&signal);
        let timeline: Vec<String> = events.iter().map(describe).collect();
        // Frame n starts at n × 32 ms; the scene starts at frame 320.
        let expected = [
            // +1.0 s clap → frame 351; the reflex confirms it is noise on the next frame.
            "351 onset",
            "352 startle",
            // +2.0 s calm voice at the speaker's own pitch → frame 382; sure of it 3 frames on.
            "382 onset",
            "385 utterance start 383",
            // 47 voiced frames (1.5 s) in, the baseline: 150 Hz at -29 dBFS.
            "429 baseline 150 Hz -29 dB",
            // Ends at +4.0 s (frame 445); sure 8 frames (256 ms) of quiet later.
            "452 utterance end 383..445 calm +0.0 st -0.1 dB normal",
            // +5.0 s: 7 semitones up with a ±3 st sing-song swing, same loudness.
            "476 onset",
            "479 utterance start 477",
            "521 utterance end 477..514 cutesy +7.2 st -0.1 dB normal",
            // +7.5 s: 12 dB louder, richer, 2 st up. Loud and sudden, but a voice: no startle.
            "554 onset",
            "557 utterance start 555",
            "599 utterance end 555..592 angry +2.0 st +11.9 dB loud",
            // +10.0 s: 10 dB quieter, and still a sound rising out of the room. Under the mean
            // rule this onset went missing: the shout's first frame had lifted the floor.
            "632 onset",
            "635 utterance start 633",
            "677 utterance end 633..670 calm +0.0 st -10.0 dB quiet",
        ];
        assert_eq!(timeline, expected, "\n{}", timeline.join("\n"));
    }

    fn describe(e: &Event) -> String {
        let i = e.index;
        match e.kind {
            EventKind::Onset { .. } => format!("{i} onset"),
            EventKind::Startle { .. } => format!("{i} startle"),
            EventKind::UtteranceStart { start } => format!("{i} utterance start {start}"),
            EventKind::BaselineSet { hz, level_db } => {
                format!("{i} baseline {hz:.0} Hz {level_db:.0} dB")
            }
            EventKind::UtteranceEnd {
                start,
                end,
                tone,
                rel_pitch_st,
                rel_loud_db,
                loudness,
            } => format!(
                "{i} utterance end {start}..{end} {} {:+.1} st {:+.1} dB {}",
                tone.map_or("-", Tone::as_str),
                rel_pitch_st.unwrap_or(f32::NAN),
                rel_loud_db.unwrap_or(f32::NAN),
                loudness.map_or("-", Loudness::as_str),
            ),
        }
    }
}
