//! Every threshold the ear decides with, by name, so a slider on the page and a tuning run on a
//! recording move the same number. Defaults carry their reason; those marked "tune" are first
//! guesses waiting for recordings.

macro_rules! params {
    ($($(#[doc = $doc:literal])* $name:ident = $default:expr;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Params {
            $($(#[doc = $doc])* pub $name: f32,)*
        }

        impl Default for Params {
            fn default() -> Self {
                Self { $($name: $default,)* }
            }
        }

        impl Params {
            pub const NAMES: &[&str] = &[$(stringify!($name),)*];

            pub fn get(&self, name: &str) -> Option<f32> {
                match name {
                    $(stringify!($name) => Some(self.$name),)*
                    _ => None,
                }
            }

            /// `false` for a name that is not a parameter.
            pub fn set(&mut self, name: &str, value: f32) -> bool {
                match name {
                    $(stringify!($name) => { self.$name = value; true })*
                    _ => false,
                }
            }
        }
    };
}

params! {
    /// dB over the floor where a sound starts: the sentry's 6×, so the two agree on "a sound".
    onset_db = 15.6;
    /// dB over the floor for a startle: well past a sound, into a bang.
    startle_db = 24.0;
    /// dB the level must climb within 64 ms for a startle: sudden, not swelling. Tune.
    startle_rise_db = 15.0;
    /// Absolute dBFS under which nothing startles: above talking (≈ -29 in the scene test), under a
    /// shout or a clap, so a voice starting in a silent room is a sound, not a fright. Tune.
    startle_min_dbfs = -25.0;
    /// Seconds after a startle in which another cannot fire.
    startle_refractory_s = 1.0;
    /// dB over the floor a frame needs before pitch is looked for. Under 6.02 on purpose: the
    /// sentry absorbs a sound that lasts 3 s+ by raising its floor to half of it, which leaves a
    /// sustained voice exactly 6.02 dB over; a higher gate would lose a long sentence mid-way.
    /// Periodicity does the real work.
    voiced_db = 5.0;
    /// Highest YIN aperiodicity still counted as voiced. Tune.
    voiced_aperiodicity = 0.25;
    /// Voiced frames that set the speaker's baseline: 1.5 s of voice.
    baseline_frames = 47.0;
    /// Semitones over the speaker's baseline where pitch reads as raised. Pet- and
    /// infant-directed speech sit several semitones up. Tune.
    cute_pitch_st = 4.0;
    /// Pitch spread (standard deviation, semitones) over the last second that reads as sing-song.
    cute_spread_st = 2.0;
    /// dB over the speaker's baseline where a voice reads as raised in anger. Tune.
    angry_loud_db = 6.0;
    /// Absolute dBFS a voice must reach to read as angry, whoever is speaking.
    angry_min_dbfs = -30.0;
    /// Brightness (Hz) over which a voice has the high-frequency push of hot anger. Tune.
    angry_bright_hz = 1000.0;
    /// dB from the speaker's baseline loudness that reads as a loud / a quiet voice.
    loud_db = 6.0;
    quiet_db = -6.0;
}
