//! `microhear <file.wav>`: one CSV row per 32 ms frame, `t_s,level_db`.
//!
//! Takes 16 kHz mono, which is what the duck records:
//! `arecord -D plughw:aic3104,0 -f S16_LE -r 16000 -c 1 -d 30 clip.wav`
//!
//! `microhear probe`: raw S16_LE at 16 kHz on stdin, the shape robotd reads from
//! `arecord -t raw`, and a summary on stdout of how it arrived and what the ear cost:
//!
//! - live: `arecord -D plughw:aic3104,0 -f S16_LE -r 16000 -c 1 -t raw -d 10 | microhear probe`
//!   measures the batches the mic really delivers, so the latency they add is a number.
//! - synthetic: `head -c 3200000 /dev/urandom | microhear probe` is 100 s of noise as fast as the
//!   pipe goes; the arrival numbers mean nothing, the CPU numbers are the point.

use std::io::Read;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use microhear_core::{Ear, FRAME, FrameReport, SAMPLE_RATE};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["probe"] => probe(),
        [path] => wav(path),
        _ => bail!("usage: microhear <file.wav> | microhear probe"),
    }
}

fn wav(path: &str) -> Result<()> {
    let mut reader = hound::WavReader::open(path).with_context(|| format!("opening {path}"))?;
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

fn probe() -> Result<()> {
    // robotd's pump() reads 4096 bytes at a time; the same buffer sees the batches it sees.
    let mut buf = [0u8; 4096];
    let mut carry: Option<u8> = None;
    let mut stdin = std::io::stdin().lock();

    let mut ear = Ear::new();
    let mut samples: Vec<f32> = Vec::with_capacity(buf.len() / 2 + 1);
    let mut reports: Vec<FrameReport> = Vec::new();
    let mut levels: Vec<f64> = Vec::new();

    let mut batch_ms = Vec::new();
    let mut gap_ms = Vec::new();
    let mut ear_time = Duration::ZERO;
    let mut slowest_push = Duration::ZERO;
    let mut last = None;

    loop {
        let n = stdin.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let now = Instant::now();
        if let Some(prev) = last.replace(now) {
            gap_ms.push(now.duration_since(prev).as_secs_f64() * 1e3);
        }
        batch_ms.push(n as f64 / 2.0 / SAMPLE_RATE as f64 * 1e3);

        // A pipe can split a sample across reads; its first byte waits for the next one.
        samples.clear();
        let mut bytes = buf[..n].iter().copied();
        if let Some(lo) = carry.take()
            && let Some(hi) = bytes.next()
        {
            samples.push(i16::from_le_bytes([lo, hi]) as f32 / 32768.0);
        }
        while let Some(lo) = bytes.next() {
            match bytes.next() {
                Some(hi) => samples.push(i16::from_le_bytes([lo, hi]) as f32 / 32768.0),
                None => carry = Some(lo),
            }
        }

        reports.clear();
        let t = Instant::now();
        ear.push(&samples, &mut reports);
        let spent = t.elapsed();
        ear_time += spent;
        slowest_push = slowest_push.max(spent);
        levels.extend(reports.iter().map(|r| r.level_db as f64));
    }

    let frames = levels.len();
    if frames == 0 {
        bail!("no complete frame on stdin; is anything piped in?");
    }
    let audio_s = frames as f64 * FRAME as f64 / SAMPLE_RATE as f64;
    println!("audio        {audio_s:.1} s, {frames} frames");
    println!(
        "batch        {} reads, ms p50 {:.1} max {:.1}",
        batch_ms.len(),
        percentile(&mut batch_ms, 0.5),
        percentile(&mut batch_ms, 1.0)
    );
    if !gap_ms.is_empty() {
        println!(
            "arrival gap  ms p50 {:.1} p99 {:.1} max {:.1}",
            percentile(&mut gap_ms, 0.5),
            percentile(&mut gap_ms, 0.99),
            percentile(&mut gap_ms, 1.0)
        );
    }
    println!(
        "ear cpu      {:.2} us/frame, slowest push {:.1} us, {:.4}% of one core",
        ear_time.as_secs_f64() * 1e6 / frames as f64,
        slowest_push.as_secs_f64() * 1e6,
        ear_time.as_secs_f64() / audio_s * 100.0
    );
    println!(
        "level        dBFS p5 {:.1} p50 {:.1} p95 {:.1} max {:.1}",
        percentile(&mut levels, 0.05),
        percentile(&mut levels, 0.5),
        percentile(&mut levels, 0.95),
        percentile(&mut levels, 1.0)
    );
    Ok(())
}

/// Nearest-rank percentile; sorts in place.
fn percentile(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}
