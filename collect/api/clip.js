// Takes one labelled clip from the microhear page and keeps it in a private Blob store, for
// `microhear check` and, later, a test set. The page is static (GitHub Pages), so this is the only
// server: it checks the clip is what the page records (16 kHz mono 16-bit wav, a known label,
// under the function body limit) and stores it under clips/<label>/.
import { del, list, put } from "@vercel/blob";

// `unlabelled` is a session nobody said what it was: kept, but out of the labelled test set.
const LABELS = new Set([
  "calm", "cutesy", "angry", "quiet", "loud", "startle", "clap", "snap", "unlabelled",
]);

// Browsers only from the page itself. Not security, curl can say anything, but it keeps other
// sites from posting into the store from their visitors' browsers.
const ORIGINS = new Set([
  "https://archdiner.github.io",
  "http://localhost:8000",
  "http://localhost:8765",
]);

// The function body limit is 4.5 MB; 4 MB is ~2 minutes at 16 kHz mono 16-bit.
const MAX_BYTES = 4_000_000;

function cors(origin) {
  return ORIGINS.has(origin)
    ? {
        "access-control-allow-origin": origin,
        "access-control-allow-methods": "GET, POST, DELETE, OPTIONS",
        "access-control-allow-headers": "content-type",
        vary: "origin",
      }
    : null;
}

export function OPTIONS(request) {
  const headers = cors(request.headers.get("origin"));
  return new Response(null, { status: headers ? 204 : 403, headers: headers ?? {} });
}

export async function POST(request) {
  const headers = cors(request.headers.get("origin"));
  if (!headers) return new Response("origin not allowed", { status: 403 });
  const fail = (status, error) => Response.json({ error }, { status, headers });

  const label = new URL(request.url).searchParams.get("label");
  if (!LABELS.has(label)) return fail(400, `label must be one of ${[...LABELS].join(", ")}`);
  if (Number(request.headers.get("content-length") ?? 0) > MAX_BYTES) return fail(413, "clip over ~2 minutes");

  const bytes = new Uint8Array(await request.arrayBuffer());
  if (bytes.length > MAX_BYTES) return fail(413, "clip over ~2 minutes");
  const wav = new DataView(bytes.buffer);
  const text = (at, n) => String.fromCharCode(...bytes.subarray(at, at + n));
  const ok =
    bytes.length > 44 &&
    text(0, 4) === "RIFF" &&
    text(8, 4) === "WAVE" &&
    wav.getUint16(20, true) === 1 && // PCM
    wav.getUint16(22, true) === 1 && // mono
    wav.getUint32(24, true) === 16000 &&
    wav.getUint16(34, true) === 16;
  if (!ok) return fail(415, "want a 16 kHz mono 16-bit wav, as the page records");

  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const blob = await put(`clips/${label}/${stamp}-${crypto.randomUUID().slice(0, 8)}.wav`, bytes, {
    access: "private",
    contentType: "audio/wav",
  });
  return Response.json({ pathname: blob.pathname }, { headers });
}

// How many clips each label has, so the page can show progress toward a test set. Counts only.
export async function GET(request) {
  const headers = cors(request.headers.get("origin")) ?? {};
  const counts = Object.fromEntries([...LABELS].map((l) => [l, 0]));
  let cursor;
  do {
    const page = await list({ prefix: "clips/", cursor });
    for (const b of page.blobs) {
      const label = b.pathname.split("/")[1];
      if (label in counts) counts[label]++;
    }
    cursor = page.cursor;
  } while (cursor);
  return Response.json(counts, { headers: { ...headers, "cache-control": "no-store" } });
}

// A bad take, deleted by the page that recorded it. Only paths this function writes are
// accepted; the random suffix means only whoever uploaded a clip knows its path.
export async function DELETE(request) {
  const headers = cors(request.headers.get("origin"));
  if (!headers) return new Response("origin not allowed", { status: 403 });
  const pathname = new URL(request.url).searchParams.get("pathname") ?? "";
  const match = pathname.match(/^clips\/([a-z]+)\/[0-9TZ-]+-[0-9a-f]{8}\.wav$/);
  if (!match || !LABELS.has(match[1])) {
    return Response.json({ error: "not a clip path" }, { status: 400, headers });
  }
  await del(pathname);
  return Response.json({ deleted: pathname }, { headers });
}
