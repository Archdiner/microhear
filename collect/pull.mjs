// Downloads every collected clip into a local folder, keeping clips/<label>/ so `microhear check`
// reads the label off the path: `node collect/pull.mjs clips && microhear check clips/*/*.wav`.
// Needs the store's token: `vercel env pull` in collect/ writes it to .env.local.
import { get, list } from "@vercel/blob";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";

const here = dirname(new URL(import.meta.url).pathname);
const env = await readFile(join(here, ".env.local"), "utf8").catch(() => "");
const token = env.match(/^BLOB_READ_WRITE_TOKEN="?([^"\n]+)"?/m)?.[1] ?? process.env.BLOB_READ_WRITE_TOKEN;
if (!token) throw new Error("no BLOB_READ_WRITE_TOKEN: run `vercel env pull` in collect/");

const out = process.argv[2] ?? "clips";
let cursor;
let fetched = 0;
let kept = 0;
do {
  const page = await list({ prefix: "clips/", cursor, token });
  for (const blob of page.blobs) {
    const path = join(out, blob.pathname.replace(/^clips\//, ""));
    kept++;
    if (existsSync(path)) continue;
    const file = await get(blob.pathname, { access: "private", token });
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, Buffer.from(await new Response(file.stream).arrayBuffer()));
    fetched++;
  }
  cursor = page.cursor;
} while (cursor);
console.log(`${kept} clips in the store, ${fetched} new into ${out}/`);
