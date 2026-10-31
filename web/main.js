import init, { Ear, sample_rate } from "./pkg/microhear_wasm.js";

const button = document.querySelector("#start");
const readout = document.querySelector("#level");
const canvas = document.querySelector("#trace");
const ctx2d = canvas.getContext("2d");

// 10 s of 32 ms frames.
const HISTORY = 312;
const MIN_DB = -90;
const levels = [];

// What is open while listening, so stopping can release the mic and the browser's indicator goes out.
let session = null;

button.addEventListener("click", () => (session ? stop() : start()));

async function start() {
  button.disabled = true;
  await init();
  const ear = new Ear();

  // The browser's voice processing would rewrite exactly what the ear measures: AGC flattens
  // loudness relative to the room, and noise suppression eats the floor.
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
  });
  const audio = new AudioContext({ sampleRate: sample_rate() });
  await audio.audioWorklet.addModule("./worklet.js");
  const tap = new AudioWorkletNode(audio, "tap");
  audio.createMediaStreamSource(stream).connect(tap);

  tap.port.onmessage = ({ data }) => {
    for (const db of ear.push(data)) {
      levels.push(db);
      if (levels.length > HISTORY) levels.shift();
    }
  };
  session = { stream, audio, ear, frame: requestAnimationFrame(draw) };
  button.textContent = "stop";
  button.disabled = false;
}

function stop() {
  const { stream, audio, ear, frame } = session;
  session = null;
  cancelAnimationFrame(frame);
  stream.getTracks().forEach((t) => t.stop());
  audio.close();
  ear.free();
  button.textContent = "start mic";
}

function draw() {
  const { width, height } = canvas;
  ctx2d.clearRect(0, 0, width, height);
  ctx2d.strokeStyle = getComputedStyle(canvas).color;
  ctx2d.beginPath();
  levels.forEach((db, i) => {
    const x = (i / (HISTORY - 1)) * width;
    const y = height * Math.min(1, Math.max(0, db / MIN_DB));
    i ? ctx2d.lineTo(x, y) : ctx2d.moveTo(x, y);
  });
  ctx2d.stroke();
  const last = levels.at(-1);
  if (last !== undefined) readout.textContent = `${last.toFixed(1)} dBFS`;
  if (session) session.frame = requestAnimationFrame(draw);
}
