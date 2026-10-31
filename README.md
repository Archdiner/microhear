# microhear

how [microduck](https://github.com/pollen-robotics/microduck) hears. one rust core, run in the
browser over wasm now and inside the duck's mic worker later.

everything is relative: loudness against the room, pitch against the speaker. that is what lets
it work for someone it has never heard.

```
reflex (from the frame alone)
L0 level      dBFS per 32 ms frame                                     ✓
L1 onset      dB over soundsentry's room floor; startle on a sudden     ✓
              loud rise, on the frame it lands in
interpretation (needs a baseline and a second of context)
L2 voicing    loud enough over the floor and periodic enough            ✓
L3 pitch      YIN over the last 64 ms, 70–1000 Hz                       ✓
L4 relative   pitch (semitones) and loudness (dB) vs this one speaker   ✓
L5 tone       calm │ cutesy │ angry, plus quiet │ normal │ loud voice   ✓ first cut
```

one speaker. the first 1.5 s of voice sets their neutral pitch and loudness (the baseline); it then
follows slowly, from calm speech only, so a minute of baby talk stays baby talk. every threshold is a
named parameter (`crates/microhear-core/src/params.rs`); the page's sliders move the same numbers.

events, not frames, are the contract: onset, startle, voice start, voice end with its tone, baseline
set. `scene_timeline` in the core's tests freezes when each fires; moving one is a decision.

**petting mute.** on the duck the mic is practically a contact mic, so a head scratch is the
loudest thing it hears. microduck's `SoundSentry` mutes sound events while the petting classifier
says petting, plus ~1 s after. whatever microhear emits on the duck must honour the same mute, or
every pet reads as a startle.

**one floor.** the room floor belongs to `SoundSentry`, which already adapts it (EMA, τ ≈ 6 s,
outside events, absorbing anything loud for 3 s+). L1 reads that floor; it does not keep a second
one that could drift from it.

## downstream

- **persistence.** the baseline lives in memory and is gone on restart. the duck should remember its
  person's voice: worth a few bytes on disk, once there is a reason to trust what it learned.
- **orient like a single-eared animal.** the duck has one wired mic, so no time difference between
  two ears to locate a sound. instead: sweep the head yaw, read L0 at each angle, turn toward the
  loudest. near-zero cpu, uses kinematics the duck already has.

## layout

| | |
|---|---|
| `crates/microhear-core` | the ear. pure dsp, no deps |
| `crates/microhear-wasm` | browser binding |
| `crates/microhear-cli` | `microhear clip.wav` → per-frame csv. `microhear events clip.wav` → the event timeline, for tuning on recordings. `microhear probe` → how raw pcm arrives on stdin and what the ear costs |
| `web/` | live mic or a recording through the same worklet, every layer drawn, sliders for every threshold. deployed to github pages |

## run

```sh
cargo test --workspace
cargo run -p microhear-cli -- clip.wav

rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.100
scripts/build-web.sh && python3 -m http.server -d web 8000
```

cpu under a constrained aarch64 linux (the duck is an rk3566, 4× cortex-a55):

```sh
docker run --rm --platform linux/arm64 -v "$PWD":/src:ro -v microhear-target:/target \
  -e CARGO_TARGET_DIR=/target rust:1-alpine sh -c 'cd /src && cargo build --release -p microhear-cli'
docker run --rm --platform linux/arm64 --cpus 0.25 --memory 256m -v microhear-target:/target \
  alpine sh -c 'head -c 3200000 /dev/urandom | /target/release/microhear probe'
```

a container on an apple-silicon mac is a ceiling, not the duck: its cores are several times an
a55's. there is no physical duck yet, so nothing here has run on the board.

record on the duck: `arecord -D plughw:aic3104,0 -f S16_LE -r 16000 -c 1 -d 30 clip.wav`
