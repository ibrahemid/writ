#!/usr/bin/env node
import { execSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..', '..');
const crateDir = path.join(repoRoot, 'crates', 'writ-render');
const outDir = path.join(repoRoot, 'site', 'src', 'wasm', 'writ-render');
const wasmFile = path.join(outDir, 'writ_render_bg.wasm');

console.log(`Building writ-render WASM → ${outDir}`);

execSync(
  `wasm-pack build "${crateDir}" --target web --out-dir "${outDir}" --features wasm --no-default-features`,
  { stdio: 'inherit' },
);

// Optimise with system wasm-opt if available (wasm-pack's bundled version is too old
// for bulk-memory instructions emitted by rustc >= 1.73).
const wasmOptBin = execSync('which wasm-opt 2>/dev/null || true', { encoding: 'utf8' }).trim();
if (wasmOptBin && existsSync(wasmFile)) {
  console.log(`Optimising ${wasmFile} with ${wasmOptBin} -Oz`);
  const tmpFile = `${wasmFile}.opt`;
  execSync(`"${wasmOptBin}" "${wasmFile}" -o "${tmpFile}" -Oz --enable-bulk-memory`, {
    stdio: 'inherit',
  });
  execSync(`mv "${tmpFile}" "${wasmFile}"`, { stdio: 'inherit' });
  console.log('wasm-opt pass complete');
} else {
  console.warn('wasm-opt not found on PATH — skipping optimisation pass (binary may be larger)');
}

console.log('wasm build complete');
