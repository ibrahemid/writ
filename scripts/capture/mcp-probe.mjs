// Talks to `writ mcp` over stdio as an approved client, so the Activity
// panel has real rows to show: a few reads, nothing written.
//
//   node scripts/capture/mcp-probe.mjs <writ binary> <data dir> <notes dir> [client name]
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';

const [bin, dataDir, notesDir, clientName] = process.argv.slice(2);
if (!bin || !dataDir || !notesDir) {
  console.error('usage: mcp-probe.mjs <writ binary> <data dir> <notes dir>');
  process.exit(2);
}

const CLIENT_NAME = clientName || 'Scribe CLI';

const child = spawn(bin, ['mcp'], {
  env: { ...process.env, WRIT_DATA_DIR: dataDir, WRIT_NOTES_DIR: notesDir },
  stdio: ['pipe', 'pipe', 'inherit'],
});
const lines = createInterface({ input: child.stdout });
const pending = new Map();
let nextId = 1;

lines.on('line', (line) => {
  let msg;
  try {
    msg = JSON.parse(line);
  } catch {
    return;
  }
  if (msg.id !== undefined && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
});

function call(method, params) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${method}: no answer in 15 s`)), 15000);
    pending.set(id, (msg) => {
      clearTimeout(timer);
      resolve(msg);
    });
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  });
}

function notify(method, params) {
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method, params }) + '\n');
}

function summary(msg) {
  const text = msg.result?.content?.map((c) => c.text ?? '').join('') ?? JSON.stringify(msg.error ?? msg);
  return `${msg.result?.isError ? 'refused' : 'ok'} ${text.slice(0, 80).replace(/\s+/g, ' ')}`;
}

await call('initialize', {
  protocolVersion: '2025-06-18',
  capabilities: {},
  clientInfo: { name: CLIENT_NAME, version: '0.4.0' },
});
notify('notifications/initialized', {});

const calls = [
  ['list_notes', {}],
  ['search_notes', { query: 'compost' }],
  ['read_note', { path: 'Garden/Garden plan.md' }],
  ['note_backlinks', { path: 'Garden/Garden plan.md' }],
  ['read_note', { path: 'Garden/Seed order.md' }],
  ['note_tags', { path: 'Garden committee 10 Sep.md' }],
];
for (const [name, args] of calls) {
  const answer = await call('tools/call', { name, arguments: args });
  console.log(`${name}\t${summary(answer)}`);
  await new Promise((r) => setTimeout(r, 400));
}
child.stdin.end();
await new Promise((resolve) => child.on('exit', resolve));
