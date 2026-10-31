# microhear

how [microduck](https://github.com/pollen-robotics/microduck) hears. one rust core, run in the
browser over wasm now and inside the duck's mic worker later.

everything is relative: loudness against the room, pitch against the speaker. that is what lets
it work for someone it has never heard.

```
L0 level      dBFS per 32 ms frame                         ✓ here
L1 onset      dB over the room floor, fast rise ─► startle
L3 pitch      YIN on voiced frames
L4 relative   pitch + loudness vs this speaker
L5 tone       calm │ cutesy │ angry
```

**petting mute.** on the duck the mic is practically a contact mic, so a head scratch is the
loudest thing it hears. microduck's `SoundSentry` mutes sound events while the petting classifier
says petting, plus ~1 s after. whatever microhear emits on the duck must honour the same mute, or
every pet reads as a startle.

## layout

| | |
|---|---|
| `crates/microhear-core` | the ear. pure dsp, no deps |
| `crates/microhear-wasm` | browser binding |
| `crates/microhear-cli` | `microhear clip.wav` → per-frame csv, for tuning on recordings |
| `web/` | live mic page, deployed to github pages |

## run

```sh
cargo test --workspace
cargo run -p microhear-cli -- clip.wav

rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.100
scripts/build-web.sh && python3 -m http.server -d web 8000
```

record on the duck: `arecord -D plughw:aic3104,0 -f S16_LE -r 16000 -c 1 -d 30 clip.wav`
