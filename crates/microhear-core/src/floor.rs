//! The room floor, as microduck's `SoundSentry` keeps it (`pet-detect/src/worker.rs`, cbe0a9f).
//!
//! On the duck the sentry owns the floor and robotd hands it over with [`crate::Ear::set_floor`];
//! this mirror exists so the browser and the CLI hear against the same floor without a duck.
//! Linear RMS, like the sentry, and the same constants: change one and the other goes with it.

/// Where the sentry's floor starts, ≈ -50 dBFS.
const START: f32 = 0.003;
/// Per-frame EMA weight, τ ≈ 200 frames ≈ 6.4 s.
const ALPHA: f32 = 0.005;
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
    pub fn frame(&mut self, rms: f32, petting: bool) -> f32 {
        let floor = self.floor;
        let on = (floor * ON_RATIO).max(ON_MIN);
        let off = (floor * OFF_RATIO).max(OFF_MIN);
        self.cooldown_frames = self.cooldown_frames.saturating_sub(1);
        if !self.in_event {
            self.floor = (1.0 - ALPHA) * self.floor + ALPHA * rms;
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

    #[test]
    fn adapts_down_to_a_quiet_room() {
        let mut f = SentryFloor::default();
        for _ in 0..2000 {
            f.frame(0.0005, false);
        }
        assert!((f.frame(0.0005, false) - 0.0005).abs() < 1e-5);
    }

    #[test]
    fn a_short_sound_does_not_move_it() {
        let mut f = SentryFloor::default();
        for _ in 0..2000 {
            f.frame(0.001, false);
        }
        for _ in 0..10 {
            f.frame(0.1, false);
        }
        // Only the frame that starts the event is averaged in (the sentry updates the floor before
        // it checks the threshold): 0.5% of 0.1 lands on top, nothing after it.
        assert!((f.frame(0.001, false) - 0.0015).abs() < 1e-4);
    }

    #[test]
    fn three_seconds_of_loud_becomes_the_room() {
        let mut f = SentryFloor::default();
        for _ in 0..2000 {
            f.frame(0.001, false);
        }
        for _ in 0..100 {
            f.frame(0.05, false);
        }
        assert!(f.frame(0.05, false) >= 0.025);
    }
}
