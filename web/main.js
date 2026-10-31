import init, { Ear, sample_rate } from "./pkg/microhear_wasm.js";

const $ = (id) => document.getElementById(id);
const css = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

// 10 s of 32 ms frames.
const HISTORY = 312;
const FRAME_S = 512 / 16000;
const RULER_ST = 12;

// The sliders: every one is a parameter of the rust core (crates/microhear-core/src/params.rs).
const SLIDERS = [
  ["room floor", "floor_min_tracker", "min-tracking floor (experiment)", 0, 1, 1, "", "0 = soundsentry's floor, what the duck uses. 1 = fast-down slow-up tracker: talking cannot ratchet it up"],
  ["reflex", "onset_db", "onset over floor", 3, 30, 0.1, "dB", "a sound rising out of the room; soundsentry's 6× is 15.6"],
  ["reflex", "startle_db", "startle over floor", 6, 50, 0.5, "dB", "how far over the room a startle must be"],
  ["reflex", "startle_rise_db", "startle suddenness", 3, 40, 0.5, "dB / 64 ms", "how fast it must climb: a clap, not a swell"],
  ["reflex", "startle_min_dbfs", "startle loud outright", -60, 0, 0.5, "dBFS", "above talking, under a clap or bang"],
  ["reflex", "startle_min_aperiodicity", "startle noise-like", 0, 1, 0.01, "min", "a clap is noise (0.5+), a voice is not (≤ 0.2); checked on the frame after the rise"],
  ["voice", "voiced_db", "voice over floor", 0, 20, 0.5, "dB", "loud enough to look for a pitch"],
  ["voice", "voiced_aperiodicity", "voice periodicity", 0.05, 0.6, 0.01, "max", "yin: 0 is a pure tone, 1 is noise"],
  ["voice", "baseline_frames", "baseline learn time", 10, 150, 1, "frames", "voiced frames that set your baseline (47 ≈ 1.5 s)"],
  ["cutesy", "cute_pitch_st", "raised pitch", 1, 12, 0.25, "st", "semitones over your baseline"],
  ["cutesy", "cute_spread_st", "sing-song swing", 0.25, 6, 0.25, "st", "pitch spread over the last second"],
  ["angry", "angry_loud_db", "louder than you", 1, 20, 0.5, "dB", "over your baseline loudness"],
  ["angry", "angry_min_dbfs", "loud outright", -60, 0, 0.5, "dBFS", "the floor on loudness angry needs"],
  ["angry", "angry_richness", "pressed voice", 1, 20, 0.1, "× pitch", "brightness ÷ pitch: how far up the harmonics the energy reaches. live value shown above"],
  ["loudness", "loud_db", "loud voice", 1, 20, 0.5, "dB", "over your baseline loudness"],
  ["loudness", "quiet_db", "quiet voice", -20, -1, 0.5, "dB", "under your baseline loudness"],
];

// The one server: a Vercel function that keeps clips in a private Blob store (collect/).
const COLLECT = "https://microhear-collect.vercel.app/api/clip";
const GOAL = 15;

let ear = null;
let recording = null;
const clips = [];
let session = null;
const frames = [];
const spans = []; // utterances: {start, end, tone}
const log = [];
let startleUntil = 0;

const toneColor = (tone) => (tone ? css(`--${tone}`) : css("--floor"));

$("start").addEventListener("click", () => (session ? stop() : start()));
$("takes").addEventListener("click", (e) => {
  const button = e.target.closest("button[data-label]");
  if (!button) return;
  const label = button.dataset.label;
  if (session?.label === label) stop();
  else start(label);
});
$("clips").addEventListener("click", (e) => {
  const button = e.target.closest("button[data-delete]");
  if (button) deleteClip(clips[Number(button.dataset.delete)]);
});
$("reset").addEventListener("click", () => ear?.reset_speaker());
$("defaults").addEventListener("click", () => {
  for (const [, name] of SLIDERS) setParam(name, Ear.defaults(name));
});

const pet = (on) => {
  ear?.set_petting(on);
  $("pet").classList.toggle("held", on);
};
$("pet").addEventListener("pointerdown", () => pet(true));
for (const ev of ["pointerup", "pointerleave"]) $("pet").addEventListener(ev, () => pet(false));
addEventListener("keydown", (e) => e.key === "p" && !e.repeat && pet(true));
addEventListener("keyup", (e) => e.key === "p" && pet(false));


// Mic and recording go through the same worklet, so a recording is heard exactly as the mic is.
async function listen(connect, label) {
  if (session) stop();
  $("error").textContent = "";
  const audio = new AudioContext({ sampleRate: sample_rate() });
  await audio.audioWorklet.addModule("./worklet.js");
  const tap = new AudioWorkletNode(audio, "tap");
  tap.port.onmessage = ({ data }) => take(data);
  const release = await connect(audio, tap);
  session = { audio, release };
  $("start").textContent = "stop";
  $("status").textContent = label;
}

async function start(label = "unlabelled") {
  $("start").disabled = true;
  try {
    // The browser's voice processing would rewrite exactly what the ear measures: AGC flattens
    // loudness relative to the room, and noise suppression eats the floor.
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
    });
    await listen((audio, tap) => {
      audio.createMediaStreamSource(stream).connect(tap);
      return () => stream.getTracks().forEach((t) => t.stop());
    }, label === "unlabelled" ? "listening" : `recording ${label}`);
    session.label = label;
    session.began = performance.now();
    recording = [];
    renderTakes();
  } catch (e) {
    $("error").textContent = `could not start the mic: ${e.message}. firefox cannot open a 16 kHz mic stream; try chrome or safari.`;
  }
  $("start").disabled = false;
}

$("file").addEventListener("change", async (e) => {
  const file = e.target.files[0];
  if (!file) return;
  e.target.value = "";
  await listen(async (audio, tap) => {
    // Decoded straight to 16 kHz by the context; played to the speakers and the ear at once.
    const buffer = await audio.decodeAudioData(await file.arrayBuffer());
    const src = new AudioBufferSourceNode(audio, { buffer });
    src.connect(tap);
    src.connect(audio.destination);
    src.onended = () => session && stop();
    src.start();
    return () => src.stop();
  }, `playing ${file.name}`);
});

// Every block from the worklet: kept for the recorder, then batched the way the duck batches if
// asked (arecord hands robotd 2048 samples, 128 ms, at a time) and timed.
const DUCK_BATCH = 2048;
let batch = [];
let batched = 0;
const cpu = { us: 0, frames: 0 };
function take(block) {
  if (recording) recording.push(block);
  if (!$("duck").checked) return push(block);
  batch.push(block);
  batched += block.length;
  if (batched < DUCK_BATCH) return;
  const joined = new Float32Array(batched);
  let at = 0;
  for (const b of batch) joined.set(b, (at += b.length) - b.length);
  batch = [];
  batched = 0;
  push(joined);
}
function push(samples) {
  const t = performance.now();
  const json = ear.push(samples);
  const spent = (performance.now() - t) * 1000;
  const parsed = hear(json);
  if (parsed) {
    cpu.us += spent;
    cpu.frames += parsed;
  }
}

function stop() {
  const { audio, release, label } = session;
  session = null;
  release();
  audio.close();
  finishRecording(label);
  renderTakes();
  $("start").textContent = "start mic";
  $("status").textContent = "mic off";
}

function hear(json) {
  const { frames: batch, events } = JSON.parse(json);
  for (const f of batch) {
    frames.push(f);
    if (frames.length > HISTORY) frames.shift();
  }
  for (const e of events) {
    if (e.kind === "voiceStart") spans.push({ start: e.start, end: null, tone: null });
    if (e.kind === "voiceEnd") {
      const s = spans.findLast((s) => s.start === e.start);
      if (s) Object.assign(s, { end: e.end, tone: e.tone });
      renderLastVoice(e);
    }
    if (e.kind === "startle") startleUntil = performance.now() + 700;
    log.unshift(e);
  }
  log.length = Math.min(log.length, 60);
  if (events.length) renderLog();
  if (batch.length) renderNow(batch.at(-1));
  return batch.length;
}

function setParam(name, value) {
  ear.set(name, value);
  const input = $(`p-${name}`);
  input.value = value;
  $(`v-${name}`).textContent = fmt(value);
  if (name === "cute_pitch_st") buildRuler();
  if (name === "floor_min_tracker") $("v-floor_min_tracker").textContent = value >= 0.5 ? "min-tracker" : "soundsentry";
}

const fmt = (v) => (Math.abs(v) >= 100 ? v.toFixed(0) : Math.abs(v) >= 10 ? v.toFixed(1) : v.toFixed(2));

function buildSliders() {
  const root = $("sliders");
  let group = null;
  for (const [g, name, label, min, max, step, unit, help] of SLIDERS) {
    if (g !== group) {
      group = g;
      root.insertAdjacentHTML("beforeend", `<div class="group" style="color:${g in { cutesy: 1, angry: 1 } ? `var(--${g})` : "inherit"}">${g}</div>`);
    }
    root.insertAdjacentHTML(
      "beforeend",
      `<div class="slider"><label><span>${label}</span><span><b id="v-${name}"></b> ${unit}</span></label>
       <input type="range" id="p-${name}" min="${min}" max="${max}" step="${step}">
       <small>${help} · default ${fmt(Ear.defaults(name))}</small></div>`,
    );
    $(`p-${name}`).addEventListener("input", (e) => setParam(name, Number(e.target.value)));
    setParam(name, ear.get(name));
  }
}

function buildRuler() {
  const cute = ear.get("cute_pitch_st");
  const at = (st) => `${((st + RULER_ST) / (2 * RULER_ST)) * 100}%`;
  const width = (a, b) => `${((b - a) / (2 * RULER_ST)) * 100}%`;
  $("ruler").innerHTML = `
    <div class="zone" style="left:0;width:${width(-RULER_ST, -cute)};background:var(--quiet);opacity:.25"></div>
    <div class="zone" style="left:${at(-cute)};width:${width(-cute, cute)};background:var(--calm);opacity:.35"></div>
    <div class="zone" style="left:${at(cute)};width:${width(cute, RULER_ST)};background:var(--cutesy);opacity:.4"></div>
    <div class="center" style="left:50%"></div>
    ${[-12, -6, 0, cute, 12].map((st) => `<div class="tick" style="left:${at(st)}">${st > 0 ? "+" : ""}${+st.toFixed(2)}</div>`).join("")}
    <div class="needle" id="needle" style="left:50%;opacity:0"></div>`;
}

// What can be recorded on purpose, each one button. Calm is first because cutesy and angry are
// heard relative to it; the noises are scored by whether they startle.
const TAKES = [
  ["calm", "your normal voice; record first, it sets the baseline"],
  ["cutesy", "the voice you would use on a pet"],
  ["angry", "stern, clenched, telling off"],
  ["clap", "single claps, a second or two apart"],
  ["snap", "finger snaps, a second or two apart"],
];
const takeColor = (label) => (["calm", "cutesy", "angry"].includes(label) ? `var(--${label})` : "var(--startle)");
let counts = {};

function renderTakes() {
  $("takes").innerHTML = TAKES.map(([label, hint]) => {
    const live = session?.label === label;
    const n = counts[label] ?? 0;
    return `<button data-label="${label}" class="take${live ? " live" : ""}" style="--c:${takeColor(label)}">
      <b>${live ? "■ stop" : "● record"} ${label}</b><span class="secs" data-secs="${label}"></span>
      <small>${hint}</small><small class="count${n >= GOAL ? " done" : ""}">${n}/${GOAL} kept</small></button>`;
  }).join("");
}

// The raw 16 kHz samples between start and stop become one wav, uploaded under its label. A bad
// take is deleted from the store with its delete button.
// Under half a second is a double-click or a mic that never started, not a take.
const MIN_TAKE_SAMPLES = 8000;

function finishRecording(label) {
  const n = (recording ?? []).reduce((a, b) => a + b.length, 0);
  if (n < MIN_TAKE_SAMPLES) {
    recording = null;
    if (label !== "unlabelled") $("error").textContent = `that ${label} take had no audio (under 0.5 s), so it was not kept.`;
    return;
  }
  const wav = new DataView(new ArrayBuffer(44 + 2 * n));
  const text = (at, s) => [...s].forEach((c, i) => wav.setUint8(at + i, c.charCodeAt(0)));
  text(0, "RIFF"); wav.setUint32(4, 36 + 2 * n, true); text(8, "WAVEfmt ");
  wav.setUint32(16, 16, true); wav.setUint16(20, 1, true); wav.setUint16(22, 1, true);
  wav.setUint32(24, 16000, true); wav.setUint32(28, 32000, true); wav.setUint16(32, 2, true);
  wav.setUint16(34, 16, true); text(36, "data"); wav.setUint32(40, 2 * n, true);
  let at = 44;
  for (const block of recording)
    for (const v of block) {
      wav.setInt16(at, Math.max(-1, Math.min(1, v)) * 32767, true);
      at += 2;
    }
  recording = null;
  const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  const blob = new Blob([wav], { type: "audio/wav" });
  const clip = { name: `microhear-${label}-${stamp}.wav`, label, seconds: n / 16000, url: URL.createObjectURL(blob), status: "uploading…" };
  clips.unshift(clip);
  renderClips();
  fetch(`${COLLECT}?label=${label}`, { method: "POST", headers: { "content-type": "audio/wav" }, body: blob })
    .then(async (r) => {
      const body = await r.json().catch(() => ({}));
      if (!r.ok) return `not kept: ${body.error ?? r.status}`;
      clip.pathname = body.pathname;
      return "kept ✓";
    })
    .catch(() => "not kept: offline?")
    .then((status) => {
      clip.status = status;
      renderClips();
      refreshCounts();
    });
}

async function deleteClip(clip) {
  if (!clip) return;
  clip.status = "deleting…";
  renderClips();
  if (clip.pathname) {
    const r = await fetch(`${COLLECT}?pathname=${encodeURIComponent(clip.pathname)}`, { method: "DELETE" }).catch(() => null);
    if (!r?.ok) {
      clip.status = "could not delete; try again";
      return renderClips();
    }
  }
  URL.revokeObjectURL(clip.url);
  clips.splice(clips.indexOf(clip), 1);
  renderClips();
  refreshCounts();
}

async function refreshCounts() {
  try {
    counts = await (await fetch(COLLECT, { cache: "no-store" })).json();
  } catch {
    counts = {};
  }
  renderTakes();
}

function renderClips() {
  $("clips").innerHTML = clips
    .map((c, i) => `<div class="clip"><span class="tag" style="color:${takeColor(c.label)}">${c.label}</span>
      ${c.seconds.toFixed(1)} s <audio controls src="${c.url}"></audio>
      <span style="color:var(--muted)">${c.status}</span>
      <button data-delete="${i}" title="delete this take from the store">delete</button></div>`)
    .join("");
}

// The verdict on the last whole utterance stays up after it ends, where the live bars go blank.
function renderLastVoice(e) {
  const el = $("last-voice");
  const rel = e.relPitch === null ? "before a baseline" : `${e.relPitch >= 0 ? "+" : ""}${e.relPitch.toFixed(1)} st · ${e.relLoud >= 0 ? "+" : ""}${e.relLoud.toFixed(1)} dB · ${e.loudness}`;
  el.innerHTML = `last voice: <b style="color:${toneColor(e.tone)}">${e.tone ?? "voice"}</b> <span style="color:var(--muted)">${rel}</span>`;
}

function renderNow(f) {
  $("now-level").textContent = `${f.level.toFixed(1)} dBFS`;
  $("now-over").textContent = `${f.over >= 0 ? "+" : ""}${f.over.toFixed(1)} dB over floor`;
  $("now-pitch").textContent = f.voiced ? `${f.hz.toFixed(0)} Hz` : "no pitch";
  $("now-rich").textContent = f.voiced ? `richness ${(f.bright / f.hz).toFixed(1)}` : "richness –";
  const loud = $("now-loud");
  loud.textContent = f.loudness ? `${f.loudness} voice` : "loudness –";
  loud.style.background = f.loudness === "loud" ? css("--angry") : f.loudness === "quiet" ? css("--quiet") : "";
  loud.style.color = f.loudness && f.loudness !== "normal" ? "#fff" : "";

  const hz = ear.baseline_hz();
  const set = Number.isFinite(hz);
  $("baseline").classList.toggle("learning", !set);
  const needed = ear.get("baseline_frames");
  $("baseline-progress").style.width = `${set ? 100 : (100 * ear.learned()) / needed}%`;
  if (set) {
    $("baseline-value").innerHTML = `<span style="color:var(--calm)">${hz.toFixed(0)} Hz</span> · ${ear.baseline_db().toFixed(1)} dBFS`;
    $("baseline-note").textContent = `your neutral pitch and loudness. cutesy starts at ${(hz * 2 ** (ear.get("cute_pitch_st") / 12)).toFixed(0)} Hz.`;
  } else {
    $("baseline-value").textContent = f.petting ? "petting: not listening" : "learning…";
    $("baseline-note").textContent = `speak normally: ${ear.learned()} of ${needed} voiced frames heard.`;
  }

  const needle = $("needle");
  if (needle) {
    const show = f.relPitch !== null;
    needle.style.opacity = show ? 1 : 0;
    if (show) {
      const st = Math.max(-RULER_ST, Math.min(RULER_ST, f.relPitch));
      needle.style.left = `${((st + RULER_ST) / (2 * RULER_ST)) * 100}%`;
      needle.style.background = toneColor(f.tone);
    }
  }

  for (const t of ["calm", "cutesy", "angry"]) {
    const el = $(`tone-${t}`);
    el.classList.toggle("on", f.tone === t);
    el.querySelector(".bar").style.width = `${(f[t] ?? 0) * 100}%`;
    $(`${t}-v`).textContent = f[t] === null ? "–" : f[t].toFixed(2);
  }
  $("status").textContent = f.petting ? "petting · muted" : session ? "listening" : "mic off";
}

function renderLog() {
  const t = (i) => (i * FRAME_S).toFixed(2);
  const rows = log.map((e) => {
    switch (e.kind) {
      case "onset":
        return [css("--muted"), "onset", `+${e.over.toFixed(1)} dB over the room`];
      case "startle":
        return [css("--startle"), "startle", `+${e.over.toFixed(1)} dB over the room, climbed ${e.rise.toFixed(1)} dB in 64 ms`];
      case "voiceStart":
        return [css("--fg"), "voice", `began ${t(e.start)} s, sure of it ${((e.i - e.start + 1) * FRAME_S * 1000).toFixed(0)} ms in`];
      case "baseline":
        return [css("--calm"), "baseline", `${e.hz.toFixed(0)} Hz at ${e.level.toFixed(1)} dBFS`];
      case "voiceEnd": {
        const rel = e.relPitch === null ? "" : `${e.relPitch >= 0 ? "+" : ""}${e.relPitch.toFixed(1)} st, ${e.relLoud >= 0 ? "+" : ""}${e.relLoud.toFixed(1)} dB, ${e.loudness}`;
        return [toneColor(e.tone), e.tone ?? "voice", `${t(e.start)}–${t(e.end)} s ${rel} · known ${((e.i - e.end + 1) * FRAME_S * 1000).toFixed(0)} ms after`];
      }
    }
  });
  $("log").innerHTML = rows
    .map(([color, tag, text], n) => `<div><span style="color:var(--muted)">${t(log[n].i)} s #${log[n].i}</span> <span class="tag" style="color:${color}">${tag}</span> ${text}</div>`)
    .join("");
}

// The CSS height is read once and kept apart: setting canvas.height rewrites the height
// attribute, so reading it back each frame grew the canvas by devicePixelRatio every frame.
function fit(canvas) {
  canvas.dataset.cssHeight ??= canvas.getAttribute("height");
  const cssHeight = Number(canvas.dataset.cssHeight);
  const ratio = devicePixelRatio || 1;
  const w = Math.round(canvas.clientWidth * ratio);
  const h = Math.round(cssHeight * ratio);
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
    canvas.style.height = `${cssHeight}px`;
  }
  return [canvas.getContext("2d"), w, h, ratio];
}

function draw() {
  if (session?.began) {
    const el = document.querySelector(`[data-secs="${session.label}"]`);
    if (el) el.textContent = ` ${((performance.now() - session.began) / 1000).toFixed(1)} s`;
  }
  if (performance.now() > startleUntil) $("startle").classList.remove("on");
  else $("startle").classList.add("on");
  const per = cpu.frames ? cpu.us / cpu.frames : 0;
  $("mem").textContent = `ear ≈ 6 KB (64 ms raw + 1 s features) · page history ${frames.length} frames`;
  $("cpu").textContent = cpu.frames
    ? `${per.toFixed(1)} µs of cpu per 32 ms frame (${((per / 32000) * 100).toFixed(3)}% of one core, this machine)`
    : "–";
  if (frames.length) {
    const last = frames.at(-1).i;
    const x = (i, w) => ((i - (last - HISTORY + 1)) / (HISTORY - 1)) * w;
    drawLevel(last, x);
    drawPitch(last, x);
  }
  requestAnimationFrame(draw);
}

function shade(ctx, w, h, last, x) {
  for (const s of spans) {
    const end = s.end ?? last + 1;
    if (end < last - HISTORY) continue;
    ctx.fillStyle = toneColor(s.tone);
    ctx.globalAlpha = s.tone ? 0.14 : 0.07;
    ctx.fillRect(x(s.start, w), 0, x(end, w) - x(s.start, w), h);
  }
  ctx.globalAlpha = 1;
}

function line(ctx, pts, color, width, dash = []) {
  ctx.strokeStyle = color;
  ctx.lineWidth = width;
  ctx.setLineDash(dash);
  ctx.beginPath();
  let pen = false;
  for (const [px, py] of pts) {
    if (py === null) {
      pen = false;
      continue;
    }
    pen ? ctx.lineTo(px, py) : ctx.moveTo(px, py);
    pen = true;
  }
  ctx.stroke();
  ctx.setLineDash([]);
}

function drawLevel(last, x) {
  const [ctx, w, h, r] = fit($("level"));
  const TOP = 0, BOTTOM = -90;
  const y = (db) => ((TOP - Math.max(BOTTOM, Math.min(TOP, db))) / (TOP - BOTTOM)) * h;
  ctx.clearRect(0, 0, w, h);
  shade(ctx, w, h, last, x);

  ctx.fillStyle = css("--muted");
  ctx.font = `${11 * r}px ui-monospace, monospace`;
  for (const db of [0, -20, -40, -60, -80]) {
    ctx.fillRect(0, y(db), w, r * 0.5);
    ctx.fillText(`${db} dBFS`, 4 * r, y(db) + 12 * r);
  }
  const onset = ear.get("onset_db"), startle = ear.get("startle_db");
  line(ctx, frames.map((f) => [x(f.i, w), y(f.floor + onset)]), css("--muted"), 1 * r, [4 * r, 4 * r]);
  line(ctx, frames.map((f) => [x(f.i, w), y(Math.max(f.floor + startle, ear.get("startle_min_dbfs")))]), css("--startle"), 1.5 * r, [6 * r, 4 * r]);
  line(ctx, frames.map((f) => [x(f.i, w), y(f.floor)]), css("--floor"), 2 * r);

  const base = ear.baseline_db();
  if (Number.isFinite(base)) {
    line(ctx, [[0, y(base)], [w, y(base)]], css("--calm"), 2 * r, [2 * r, 3 * r]);
    const angry = Math.max(base + ear.get("angry_loud_db"), ear.get("angry_min_dbfs"));
    line(ctx, [[0, y(angry)], [w, y(angry)]], css("--angry"), 1.5 * r, [2 * r, 3 * r]);
  }

  line(ctx, frames.map((f) => [x(f.i, w), y(f.level)]), css("--fg"), 1.5 * r);
  for (const e of log) {
    if (e.kind !== "startle" || e.i < last - HISTORY) continue;
    ctx.fillStyle = css("--startle");
    ctx.fillRect(x(e.i, w) - 1.5 * r, 0, 3 * r, h);
  }
}

function drawPitch(last, x) {
  const [ctx, w, h, r] = fit($("pitch"));
  const LO = 60, HI = 900;
  const y = (hz) => h - (Math.log(hz / LO) / Math.log(HI / LO)) * h;
  ctx.clearRect(0, 0, w, h);
  shade(ctx, w, h, last, x);

  ctx.fillStyle = css("--muted");
  ctx.font = `${11 * r}px ui-monospace, monospace`;
  for (const hz of [100, 200, 400, 800]) {
    ctx.fillRect(0, y(hz), w, r * 0.5);
    ctx.fillText(`${hz} Hz`, 4 * r, y(hz) - 4 * r);
  }

  const base = ear.baseline_hz();
  if (Number.isFinite(base)) {
    const cute = base * 2 ** (ear.get("cute_pitch_st") / 12);
    ctx.fillStyle = css("--cutesy");
    ctx.globalAlpha = 0.08;
    ctx.fillRect(0, 0, w, y(cute));
    ctx.globalAlpha = 1;
    line(ctx, [[0, y(cute)], [w, y(cute)]], css("--cutesy"), 1.5 * r, [6 * r, 4 * r]);
    line(ctx, [[0, y(base)], [w, y(base)]], css("--calm"), 3 * r);
    ctx.fillStyle = css("--calm");
    ctx.font = `bold ${13 * r}px ui-monospace, monospace`;
    ctx.fillText(`your baseline ${base.toFixed(0)} Hz`, w - 210 * r, y(base) - 6 * r);
    ctx.fillStyle = css("--cutesy");
    ctx.fillText(`cutesy ≥ ${cute.toFixed(0)} Hz`, w - 210 * r, y(cute) - 6 * r);
  }

  for (const f of frames) {
    if (!f.voiced) continue;
    ctx.fillStyle = toneColor(f.tone);
    ctx.beginPath();
    ctx.arc(x(f.i, w), y(f.hz), 3.2 * r, 0, 2 * Math.PI);
    ctx.fill();
  }
}

// Last, so every function and constant above exists before the first frame is drawn.
refreshCounts();
await init();
ear = new Ear();
buildSliders();
buildRuler();
requestAnimationFrame(draw);
