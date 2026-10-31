//! `microhear <file.wav>`: one CSV row per 32 ms frame: level, floor, pitch, relative pitch and
//! loudness, tone scores.
//!
//! `microhear events <file.wav>`: the event timeline, one line per event, the thing to compare
//! when tuning on a recording.
//!
//! Takes 16 kHz mono, which is what the duck records:
//! `arecord -D plughw:aic3104,0 -f S16_LE -r 16000 -c 1 -d 30 clip.wav`
//!
//! `microhear check <clips…>`: one person's labelled clips, scored. The label is in the file name
//! (`calm`, `cutesy`, `angry`, `quiet`, `loud`, `startle`), as the page names its downloads. Calm
//! clips go first through one ear, because they are what set the speaker's baseline; every clip
//! after is heard against it, the way the duck would hear that person. Startles in clips not
//! labelled `startle` are counted as false alarms.
//!
//! `MICROHEAR_SET="name=value,…"` overrides parameters for any command, for tuning runs:
//! `MICROHEAR_SET=floor_min_tracker=1 microhear check clips/*.wav`.
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
use microhear_core::{
    Ear, Event, EventKind, FRAME, FrameReport, Loudness, SAMPLE_RATE, Tone, ToneScores,
    frame_time_s,
};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["probe"] => probe(),
        ["events", path] => events(path),
        ["check", ref paths @ ..] if !paths.is_empty() => check(paths),
        [path] => frames(path),
        _ => bail!("usage: microhear <file.wav> | events <file.wav> | check <clips…> | probe"),
    }
}

/// An ear with `MICROHEAR_SET` applied.
fn ear() -> Result<Ear> {
    let mut ear = Ear::new();
    if let Ok(set) = std::env::var("MICROHEAR_SET") {
        for pair in set.split(',').filter(|p| !p.is_empty()) {
            let (name, value) = pair
                .split_once('=')
                .context("MICROHEAR_SET is name=value,…")?;
            let value: f32 = value
                .parse()
                .with_context(|| format!("{name} wants a number"))?;
            if !ear.params.set(name, value) {
                bail!(
                    "no parameter {name}; there are {}",
                    microhear_core::Params::NAMES.join(", ")
                );
            }
        }
    }
    Ok(ear)
}

fn read_wav(path: &str) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path).with_context(|| format!("opening {path}"))?;
    let spec = reader.spec();
    if spec.sample_rate != SAMPLE_RATE || spec.channels != 1 {
        bail!(
            "{path} is {} Hz x{}; want {SAMPLE_RATE} Hz mono (sox in.wav -r 16000 -c 1 out.wav)",
            spec.sample_rate,
            spec.channels
        );
    }
    Ok(match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1u32 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|s| s as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    })
}

fn hear(path: &str) -> Result<(Vec<FrameReport>, Vec<Event>)> {
    let samples = read_wav(path)?;
    let (mut frames, mut events) = (Vec::new(), Vec::new());
    ear()?.push(&samples, &mut frames, &mut events);
    Ok((frames, events))
}

fn opt(v: Option<f32>) -> String {
    v.map_or(String::new(), |v| format!("{v:.2}"))
}

fn frames(path: &str) -> Result<()> {
    let (frames, _) = hear(path)?;
    println!(
        "t_s,level_db,floor_db,over_floor_db,rise_db,voiced,f0_hz,aperiodicity,brightness_hz,rel_pitch_st,rel_loud_db,calm,cutesy,angry"
    );
    for r in frames {
        let tone = |f: fn(&ToneScores) -> f32| opt(r.tone.as_ref().map(f));
        println!(
            "{:.3},{:.2},{:.2},{:.2},{:.2},{},{},{},{:.0},{},{},{},{},{}",
            frame_time_s(r.index),
            r.level_db,
            r.floor_db,
            r.over_floor_db,
            r.rise_db,
            r.voiced as u8,
            opt(r.pitch.map(|p| p.hz)),
            opt(r.pitch.map(|p| p.aperiodicity)),
            r.brightness_hz,
            opt(r.rel_pitch_st),
            opt(r.rel_loud_db),
            tone(|t| t.calm),
            tone(|t| t.cutesy),
            tone(|t| t.angry),
        );
    }
    Ok(())
}

fn events(path: &str) -> Result<()> {
    let (_, events) = hear(path)?;
    for e in events {
        let what = match e.kind {
            EventKind::Onset { over_floor_db } => {
                format!("onset    {over_floor_db:+.1} dB over floor")
            }
            EventKind::Startle {
                over_floor_db,
                rise_db,
            } => format!("startle  {over_floor_db:+.1} dB over floor, {rise_db:+.1} dB rise"),
            EventKind::UtteranceStart { start } => {
                format!("voice    from {:.3} s", frame_time_s(start))
            }
            EventKind::BaselineSet { hz, level_db } => {
                format!("baseline {hz:.0} Hz at {level_db:.1} dBFS")
            }
            EventKind::UtteranceEnd {
                start,
                end,
                tone,
                rel_pitch_st,
                rel_loud_db,
                loudness,
            } => format!(
                "voice    {:.3}–{:.3} s {} {} st {} dB {}",
                frame_time_s(start),
                frame_time_s(end),
                tone.map_or("-", Tone::as_str),
                rel_pitch_st.map_or("-".into(), |x| format!("{x:+.1}")),
                rel_loud_db.map_or("-".into(), |x| format!("{x:+.1}")),
                loudness.map_or("-", Loudness::as_str),
            ),
        };
        println!("{:>8.3} s  #{:<6} {what}", frame_time_s(e.index), e.index);
    }
    Ok(())
}

const LABELS: [&str; 6] = ["calm", "cutesy", "angry", "quiet", "loud", "startle"];

#[derive(Default)]
struct Score {
    clips: u32,
    voices: u32,
    hits: u32,
    unjudged: u32,
    startles: u32,
}

fn check(paths: &[&str]) -> Result<()> {
    let label_of = |path: &str| {
        let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
        LABELS.into_iter().find(|l| name.contains(l))
    };
    let mut clips: Vec<(&str, &str)> = Vec::new();
    for &p in paths {
        match label_of(p) {
            Some(l) => clips.push((p, l)),
            None => eprintln!("skipping {p}: no label in its name ({})", LABELS.join(", ")),
        }
    }
    // Calm first: it sets the baseline everything else is relative to.
    clips.sort_by_key(|&(_, l)| l != "calm");

    let mut ear = ear()?;
    let mut scores: Vec<Score> = LABELS.iter().map(|_| Score::default()).collect();
    let mut false_startles = 0;
    let (mut frames, mut events) = (Vec::new(), Vec::new());
    for (path, label) in &clips {
        frames.clear();
        events.clear();
        ear.push(&read_wav(path)?, &mut frames, &mut events);
        let score = &mut scores[LABELS.iter().position(|l| l == label).expect("known label")];
        score.clips += 1;
        let mut clip_startles = 0;
        for e in &events {
            match e.kind {
                EventKind::Startle { .. } => clip_startles += 1,
                EventKind::UtteranceEnd { tone, loudness, .. } => {
                    score.voices += 1;
                    let heard = match *label {
                        "quiet" | "loud" => loudness.map(Loudness::as_str),
                        _ => tone.map(Tone::as_str),
                    };
                    match heard {
                        None => score.unjudged += 1,
                        Some(h) if h == *label => score.hits += 1,
                        Some(_) => {}
                    }
                }
                _ => {}
            }
        }
        if *label == "startle" {
            score.voices += 1;
            score.hits += (clip_startles > 0) as u32;
        } else {
            false_startles += clip_startles;
        }
        score.startles += clip_startles;
        println!(
            "{:<8} {path}: {} events, {clip_startles} startles",
            label,
            events.len()
        );
    }

    println!("\nlabel     clips  heard  right  no-baseline  accuracy");
    for (label, s) in LABELS.iter().zip(&scores) {
        if s.clips == 0 {
            continue;
        }
        let judged = s.voices - s.unjudged;
        let acc = if judged > 0 {
            format!("{:.0}%", 100.0 * s.hits as f32 / judged as f32)
        } else {
            "-".into()
        };
        println!(
            "{label:<9} {:>5}  {:>5}  {:>5}  {:>11}  {acc:>8}",
            s.clips, s.voices, s.hits, s.unjudged
        );
    }
    println!("false startles (in clips not labelled startle): {false_startles}");
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
    let mut events: Vec<Event> = Vec::new();
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
        events.clear();
        let t = Instant::now();
        ear.push(&samples, &mut reports, &mut events);
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
