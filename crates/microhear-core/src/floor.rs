//! The room floor, as microduck's `SoundSentry` keeps it (`pet-detect/src/worker.rs`).
//!
//! On the duck the sentry owns the floor and robotd hands it over with [`crate::Ear::set_floor`];
//! this mirror exists so the browser and the CLI hear against the same floor without a duck.
//! Linear RMS, like the sentry, and the same constants: change one and the other goes with it.
//!
//! Two rules for how the floor moves outside an event, because the sentry is mid-change:
//!
//! - [`Rule::Mean`]: microduck main today (cbe0a9f), an EMA with τ ≈ 6 s. It learns talking: speech
//!   under the 6× on-threshold is never an event, so it is averaged in, and so is the frame that
//!   starts an event. A few clips of conversation lift a −65 dBFS room to a −37 dBFS floor.
//! - [`Rule::FastDownSlowUp`]: the fix proposed upstream (Archdiner/microduck `sentry-min-floor`,
//!   ac1f470). Down to a quieter frame at once, up at most 3 dB/s; speech's word gaps hold it at
//!   the room. Everything else, event freezing and the 3 s absorption, is the same.

/// Where the sentry's floor starts, ≈ -50 dBFS.
const START: f32 = 0.003;
/// Per-frame EMA weight, τ ≈ 200 frames ≈ 6.4 s ([`Rule::Mean`]).
const ALPHA: f32 = 0.005;
/// 3 dB a second as a per-frame factor, and the lowest the floor goes ([`Rule::FastDownSlowUp`]).
const UP_PER_FRAME: f32 = 1.011_114;
const FLOOR_MIN: f32 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Mean,
    FastDownSlowUp,
}
/// An event starts at 6× the floor and ends after 3 frames under 3× it.
const ON_RATIO: f32 = 6.0;
const OFF_RATIO: f32 = 3.0;
const ON_MIN: f32 = 0.002;
const OFF_MIN: f32 = 0.0012;
const QUIET_FRAMES: u32 = 3;
/// Loud for longer than ~3 s is the room, not an event: the floor jumps to half of it.
const ABSORB_FRAMES: u32 = 94;
const COOLDOWN_FRAMES: u32 = 31;

#[derive(Debug, Clone)]
pub struct SentryFloor {
    floor: f32,
    in_event: bool,
    event_frames: u32,
    quiet_frames: u32,
    cooldown_frames: u32,
}

impl Default for SentryFloor {
    fn default() -> Self {
        Self {
            floor: START,
            in_event: false,
            event_frames: 0,
            quiet_frames: 0,
            cooldown_frames: 0,
        }
    }
}

impl SentryFloor {
    /// The floor this frame is judged against, then the sentry's update for it. `petting` is the
    /// sentry's petting hold: it keeps an event from starting, which keeps the floor adapting.
    pub fn frame(&mut self, rms: f32, petting: bool, rule: Rule) -> f32 {
        let floor = self.floor;
        let on = (floor * ON_RATIO).max(ON_MIN);
        let off = (floor * OFF_RATIO).max(OFF_MIN);
        self.cooldown_frames = self.cooldown_frames.saturating_sub(1);
        if !self.in_event {
            self.floor = match rule {
                Rule::Mean => (1.0 - ALPHA) * self.floor + ALPHA * rms,
                Rule::FastDownSlowUp if rms < self.floor => rms.max(FLOOR_MIN),
                Rule::FastDownSlowUp => self.floor * UP_PER_FRAME,
            };
            if rms > on && self.cooldown_frames == 0 && !petting {
                self.in_event = true;
                self.event_frames = 1;
                self.quiet_frames = 0;
            }
        } else {
            self.event_frames += 1;
            self.quiet_frames = if rms < off { self.quiet_frames + 1 } else { 0 };
            if self.event_frames > ABSORB_FRAMES {
                self.in_event = false;
                self.floor = self.floor.max(rms * 0.5);
                self.cooldown_frames = COOLDOWN_FRAMES;
            } else if self.quiet_frames >= QUIET_FRAMES {
                self.in_event = false;
                self.cooldown_frames = COOLDOWN_FRAMES;
            }
        }
        floor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: &mut SentryFloor, rule: Rule, rms: f32, n: usize) -> f32 {
        let mut last = 0.0;
        for _ in 0..n {
            last = f.frame(rms, false, rule);
        }
        last
    }

    #[test]
    fn both_rules_settle_on_a_quiet_room() {
        for rule in [Rule::Mean, Rule::FastDownSlowUp] {
            let mut f = SentryFloor::default();
            let floor = run(&mut f, rule, 0.0005, 2000);
            assert!((floor - 0.0005).abs() < 2e-5, "{rule:?}: {floor}");
        }
    }

    /// The ratchet: someone talking at ~10 dB over the room, under the 6× that makes an event,
    /// with word gaps. The mean learns it; the fix sits in the gaps.
    #[test]
    fn talking_raises_the_mean_and_not_the_fix() {
        let talk = |rule| {
            let mut f = SentryFloor::default();
            run(&mut f, rule, 0.001, 2000);
            for _ in 0..60 {
                run(&mut f, rule, 0.003, 9);
                run(&mut f, rule, 0.001, 1);
            }
            f.frame(0.001, false, rule)
        };
        assert!(talk(Rule::Mean) > 0.002);
        assert!(talk(Rule::FastDownSlowUp) < 0.0012);
    }

    /// The frame that starts an event is averaged into the mean: half a percent of a clap.
    #[test]
    fn a_clap_lifts_the_mean_and_not_the_fix() {
        let clap = |rule| {
            let mut f = SentryFloor::default();
            run(&mut f, rule, 0.001, 2000);
            run(&mut f, rule, 0.1, 10);
            f.frame(0.001, false, rule)
        };
        assert!((clap(Rule::Mean) - 0.0015).abs() < 1e-4);
        // On a steady room this floor sits within a step (0.1 dB) of it; two steps of slack.
        assert!(clap(Rule::FastDownSlowUp) < 0.00105);
    }

    #[test]
    fn three_seconds_of_loud_becomes_the_room_under_either() {
        for rule in [Rule::Mean, Rule::FastDownSlowUp] {
            let mut f = SentryFloor::default();
            run(&mut f, rule, 0.001, 2000);
            assert!(run(&mut f, rule, 0.05, 101) >= 0.025, "{rule:?}");
        }
    }
}
