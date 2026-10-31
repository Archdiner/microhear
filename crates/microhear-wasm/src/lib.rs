use std::fmt::Write;

use microhear_core::{EventKind, Loudness, Params, Tone};
use wasm_bindgen::prelude::*;

/// `microhear_core::Ear` for JavaScript: push worklet samples, get back JSON of the frames this
/// batch finished and the events they raised. JSON by hand, so the core stays dependency-free and
/// the page reads one string per batch rather than crossing the boundary per field.
#[wasm_bindgen]
pub struct Ear {
    inner: microhear_core::Ear,
    frames: Vec<microhear_core::FrameReport>,
    events: Vec<microhear_core::Event>,
}

#[wasm_bindgen]
impl Ear {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: microhear_core::Ear::new(),
            frames: Vec::new(),
            events: Vec::new(),
        }
    }

    /// `{"frames":[…],"events":[…]}` for this batch. Often both empty.
    pub fn push(&mut self, samples: &[f32]) -> String {
        self.frames.clear();
        self.events.clear();
        self.inner.push(samples, &mut self.frames, &mut self.events);

        let mut out = String::from("{\"frames\":[");
        for (n, r) in self.frames.iter().enumerate() {
            if n > 0 {
                out.push(',');
            }
            let tone = r.tone;
            let _ = write!(
                out,
                "{{\"i\":{},\"level\":{},\"floor\":{},\"over\":{},\"rise\":{},\"petting\":{},\
                 \"voiced\":{},\"hz\":{},\"aperiodicity\":{},\"bright\":{},\"relPitch\":{},\
                 \"relLoud\":{},\"loudness\":{},\"calm\":{},\"cutesy\":{},\"angry\":{},\"tone\":{}}}",
                r.index,
                num(Some(r.level_db)),
                num(Some(r.floor_db)),
                num(Some(r.over_floor_db)),
                num(Some(r.rise_db)),
                r.petting,
                r.voiced,
                num(r.pitch.map(|p| p.hz)),
                num(r.pitch.map(|p| p.aperiodicity)),
                num(Some(r.brightness_hz)),
                num(r.rel_pitch_st),
                num(r.rel_loud_db),
                text(r.loudness.map(Loudness::as_str)),
                num(tone.map(|t| t.calm)),
                num(tone.map(|t| t.cutesy)),
                num(tone.map(|t| t.angry)),
                text(tone.map(|t| t.tone().as_str())),
            );
        }
        out.push_str("],\"events\":[");
        for (n, e) in self.events.iter().enumerate() {
            if n > 0 {
                out.push(',');
            }
            let _ = write!(out, "{{\"i\":{},", e.index);
            let _ = match e.kind {
                EventKind::Onset { over_floor_db } => {
                    write!(
                        out,
                        "\"kind\":\"onset\",\"over\":{}}}",
                        num(Some(over_floor_db))
                    )
                }
                EventKind::Startle {
                    over_floor_db,
                    rise_db,
                } => write!(
                    out,
                    "\"kind\":\"startle\",\"over\":{},\"rise\":{}}}",
                    num(Some(over_floor_db)),
                    num(Some(rise_db))
                ),
                EventKind::UtteranceStart { start } => {
                    write!(out, "\"kind\":\"voiceStart\",\"start\":{start}}}")
                }
                EventKind::BaselineSet { hz, level_db } => write!(
                    out,
                    "\"kind\":\"baseline\",\"hz\":{},\"level\":{}}}",
                    num(Some(hz)),
                    num(Some(level_db))
                ),
                EventKind::UtteranceEnd {
                    start,
                    end,
                    tone,
                    rel_pitch_st,
                    rel_loud_db,
                    loudness,
                } => write!(
                    out,
                    "\"kind\":\"voiceEnd\",\"start\":{start},\"end\":{end},\"tone\":{},\
                     \"relPitch\":{},\"relLoud\":{},\"loudness\":{}}}",
                    text(tone.map(Tone::as_str)),
                    num(rel_pitch_st),
                    num(rel_loud_db),
                    text(loudness.map(Loudness::as_str)),
                ),
            };
        }
        out.push_str("]}");
        out
    }

    /// Change a threshold by name (see `param_names`); `false` if there is no such parameter.
    pub fn set(&mut self, name: &str, value: f32) -> bool {
        self.inner.params.set(name, value)
    }

    pub fn get(&self, name: &str) -> f32 {
        self.inner.params.get(name).unwrap_or(f32::NAN)
    }

    pub fn defaults(name: &str) -> f32 {
        Params::default().get(name).unwrap_or(f32::NAN)
    }

    pub fn param_names() -> Vec<String> {
        Params::NAMES.iter().map(|s| s.to_string()).collect()
    }

    pub fn set_petting(&mut self, petting: bool) {
        self.inner.set_petting(petting);
    }

    pub fn reset_speaker(&mut self) {
        self.inner.reset_speaker();
    }

    /// The speaker's baseline pitch in Hz, or NaN while it is still being learned.
    pub fn baseline_hz(&self) -> f32 {
        self.inner
            .speaker()
            .baseline()
            .map_or(f32::NAN, |b| 100.0 * 2f32.powf(b.pitch_st / 12.0))
    }

    /// The speaker's baseline loudness in dBFS, or NaN while it is still being learned.
    pub fn baseline_db(&self) -> f32 {
        self.inner
            .speaker()
            .baseline()
            .map_or(f32::NAN, |b| b.level_db)
    }

    /// Voiced frames heard toward the baseline while it is being learned.
    pub fn learned(&self) -> u32 {
        self.inner.speaker().learned() as u32
    }
}

impl Default for Ear {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
pub fn sample_rate() -> u32 {
    microhear_core::SAMPLE_RATE
}

fn num(v: Option<f32>) -> String {
    match v {
        Some(v) if v.is_finite() => format!("{v:.3}"),
        _ => "null".into(),
    }
}

fn text(v: Option<&str>) -> String {
    v.map_or("null".into(), |s| format!("\"{s}\""))
}
