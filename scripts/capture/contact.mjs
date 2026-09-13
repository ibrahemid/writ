// Lays every capture out on one contact sheet, labelled by file name, so a
// whole run can be read at a glance.
//
//   node scripts/capture/contact.mjs <out.png> <png> [<png> ...]
import { createRequire } from 'node:module';
import { basename, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(resolve(here, '../../site/package.json'));
const sharp = require('sharp');

const [out, ...files] = process.argv.slice(2);
if (!out || files.length === 0) {
  console.error('usage: contact.mjs <out.png> <png>...');
  process.exit(2);
}

const COLS = 4;
const CELL_W = 640;
const GAP = 24;
const LABEL_H = 40;

const escape = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');

const tiles = [];
for (const file of files.sort()) {
  const meta = await sharp(file).metadata();
  const scale = CELL_W / (meta.width ?? CELL_W);
  const h = Math.round((meta.height ?? CELL_W) * scale);
  const buf = await sharp(file).resize({ width: CELL_W }).png().toBuffer();
  tiles.push({ file, buf, h });
}
const rowH = Math.max(...tiles.map((t) => t.h)) + LABEL_H + GAP;
const rows = Math.ceil(tiles.length / COLS);
const width = COLS * (CELL_W + GAP) + GAP;
const height = rows * rowH + GAP;

const composites = [];
const labels = [];
tiles.forEach((t, i) => {
  const col = i % COLS;
  const row = Math.floor(i / COLS);
  const left = GAP + col * (CELL_W + GAP);
  const top = GAP + row * rowH + LABEL_H;
  composites.push({ input: t.buf, left, top });
  labels.push(`<text x="${left}" y="${top - 14}" font-family="Menlo, monospace" font-size="18" fill="#222">${escape(basename(t.file, '.png'))}</text>`);
});
const svg = Buffer.from(`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">${labels.join('')}</svg>`);

await sharp({ create: { width, height, channels: 3, background: '#e9e6df' } })
  .composite([...composites, { input: svg, left: 0, top: 0 }])
  .png({ compressionLevel: 9 })
  .toFile(out);
console.log(`${out}\t${tiles.length} tiles\t${width}x${height}`);
