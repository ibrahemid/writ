// Caps a capture at 2880 px wide and 1.2 MB, in place. Sharp comes from the
// site's own dependencies (site/node_modules), so nothing is installed for this.
//
//   node scripts/capture/shrink.mjs <png> [<png> ...]
import { createRequire } from 'node:module';
import { statSync, renameSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(resolve(here, '../../site/package.json'));
const sharp = require('sharp');

export const MAX_WIDTH = 2880;
export const MAX_BYTES = 1.2 * 1024 * 1024;

export async function shrink(file) {
  const meta = await sharp(file).metadata();
  let image = sharp(file);
  if ((meta.width ?? 0) > MAX_WIDTH) image = image.resize({ width: MAX_WIDTH, kernel: 'lanczos3' });
  const lossless = await image.clone().png({ compressionLevel: 9, effort: 10 }).toBuffer();
  let out = lossless;
  let quality = 100;
  // Lossless first; only when that is still over budget, palette-quantise
  // downward until it fits.
  while (out.length > MAX_BYTES && quality > 40) {
    quality -= 10;
    out = await image.clone().png({ palette: true, quality, effort: 10, dither: 0.6 }).toBuffer();
  }
  if (out.length > MAX_BYTES) throw new Error(`${file}: still ${out.length} bytes at quality ${quality}`);
  const tmp = `${file}.tmp`;
  await sharp(out).toFile(tmp);
  renameSync(tmp, file);
  return { width: Math.min(meta.width ?? 0, MAX_WIDTH), bytes: statSync(file).size, quality };
}

const files = process.argv.slice(2);
if (files.length > 0) {
  for (const file of files) {
    const r = await shrink(file);
    console.log(`${file}\t${r.width}w\t${(r.bytes / 1024).toFixed(0)} KB${r.quality < 100 ? `\tpalette q${r.quality}` : ''}`);
  }
}
