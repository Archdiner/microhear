//! `microhear <file.wav>`: one CSV row per 32 ms frame, `t_s,level_db`.
//!
//! Takes 16 kHz mono, which is what the duck records:
//! `arecord -D plughw:aic3104,0 -f S16_LE -r 16000 -c 1 -d 30 clip.wav`

use anyhow::{Context, Result, bail};
use microhear_core::{Ear, FRAME, SAMPLE_RATE};

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("usage: microhear <file.wav>")?;
    let mut reader = hound::WavReader::open(&path).with_context(|| format!("opening {path}"))?;
    let spec = reader.spec();
    if spec.sample_rate != SAMPLE_RATE || spec.channels != 1 {
        bail!(
            "{path} is {} Hz x{}; want {SAMPLE_RATE} Hz mono (sox in.wav -r 16000 -c 1 out.wav)",
            spec.sample_rate,
            spec.channels
        );
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1u32 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|s| s as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    };

    let mut reports = Vec::new();
    Ear::new().push(&samples, &mut reports);
    println!("t_s,level_db");
    for r in reports {
        let t = r.index as f64 * FRAME as f64 / SAMPLE_RATE as f64;
        println!("{t:.3},{:.2}", r.level_db);
    }
    Ok(())
}
