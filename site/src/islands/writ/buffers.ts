export type BufferLang = 'md' | 'ts' | 'html' | 'plain' | 'binary' | 'huge';

export interface BufferMeta {
  name: string;
  lang: BufferLang;
  label: string;
  dot: string;
  badge?: string;
  tok?: string;
}

export interface PaletteCommand {
  id: string;
  name: string;
  desc: string;
  kbd?: string;
}

export interface PaletteGroup {
  label: string;
  cmds: PaletteCommand[];
}

const join = (lines: string[]): string => lines.join('\n');

const reportMd = join([
  '# Payments service — migration analysis',
  '',
  'Generated for `svc-payments` at commit `a3f91c2`. Scope: move the settlement path off the legacy queue and onto the new event bus without dropping idempotency guarantees.',
  '',
  '## Summary',
  '',
  'The settlement worker is the only consumer still bound to the legacy `rabbit:settle` queue. Everything else already reads from the bus. Cutting it over removes the last broker dependency and lets us delete the dual-write shim in `ledger/append.ts`.',
  '',
  '## Cutover checklist',
  '',
  '- [x] Freeze the legacy queue for new settlements',
  '- [ ] Point the worker at the bus',
  '- [ ] Delete the dual-write shim',
  '',
  '## Settlement flow today',
  '',
  '```mermaid',
  'flowchart LR',
  '  A[Checkout] -->|charge.authorized| B(Event bus)',
  '  B --> C[Settlement worker]',
  '  C -->|legacy| D[(rabbit:settle)]',
  '  C --> E[Ledger append]',
  '  E --> F[(Postgres)]',
  '```',
  '',
  'Once the worker consumes settlement events directly from the bus, node `D` disappears.',
  '',
  '## Idempotency',
  '',
  'Each settlement is keyed by `(account_id, charge_id)`. The dedup window $w$ must cover the worst-case redelivery interval:',
  '',
  '$$w \\;\\ge\\; \\max_i\\, r_i \\,+\\, \\Delta_{clock}$$',
  '',
  '- [x] Move the idempotency key into the message body',
  '- [x] Read settlements from the bus',
  '- [ ] Delete the dual-write shim',
  '',
  '> With the bus we expect at-least-once delivery, so the worker must stay safe under replay.',
]);

const settleTs = join([
  '// settlement worker — reads the bus, dedups on the row, appends to the ledger',
  'import { bus } from "./transport/bus";',
  'import { ledger } from "./ledger/append";',
  'import { logger } from "./obs/log";',
  '',
  'export type SettleEvent = {',
  '  accountId: string;',
  '  chargeId: string;',
  '  amountMinor: number;',
  '  currency: "usd" | "eur" | "gbp";',
  '  idempotencyKey: string;',
  '};',
  '',
  'const DEDUP_HORIZON_MS = 5 * 60_000;',
  'const seen = new Map<string, number>();',
  '',
  'function settleKey(e: SettleEvent): string {',
  '  return `${e.accountId}:${e.chargeId}`;',
  '}',
  '',
  'function isDuplicate(key: string, now: number): boolean {',
  '  const at = seen.get(key);',
  '  if (at !== undefined && now - at < DEDUP_HORIZON_MS) return true;',
  '  seen.set(key, now);',
  '  return false;',
  '}',
  '',
  'export async function handleSettlement(e: SettleEvent, now = Date.now()): Promise<void> {',
  '  const key = settleKey(e);',
  '  if (isDuplicate(key, now)) {',
  '    logger.debug("skip duplicate settlement", { key });',
  '    return;',
  '  }',
  '  switch (e.currency) {',
  '    case "usd":',
  '    case "eur":',
  '    case "gbp":',
  '      await ledger.append({ ...e, key });',
  '      logger.info("settled", { key, amount: e.amountMinor });',
  '      break;',
  '    default:',
  '      logger.warn("unknown currency", { currency: e.currency });',
  '  }',
  '}',
]);

const schemaSql = join([
  '-- settlement schema — svc-payments',
  'create table settlements (',
  '  account_id   text        not null,',
  '  charge_id    text        not null,',
  '  amount_minor bigint      not null,',
  "  currency     text        not null check (currency in ('usd','eur','gbp')),",
  '  settled_at   timestamptz not null default now(),',
  '  primary key (account_id, charge_id)',
  ');',
  '',
  'create index settlements_settled_at_idx',
  '  on settlements (settled_at desc);',
  '',
  '-- backfill from the legacy ledger',
  'insert into settlements (account_id, charge_id, amount_minor, currency)',
  'select account_id, charge_id, amount_minor, currency',
  'from legacy.ledger_entries',
  "where kind = 'settlement'",
  'on conflict (account_id, charge_id) do nothing;',
]);

const gatewayLog = join([
  '2026-06-25T14:02:11Z INFO  bus connected partition=3 lag=0',
  '2026-06-25T14:02:11Z INFO  req settled account=acct_8842 charge=ch_91f amount=4200 status=ok',
  '2026-06-25T14:02:12Z WARN  redelivery seen key=acct_8842:ch_91f within horizon',
  '2026-06-25T14:02:12Z INFO  req settled account=acct_2210 charge=ch_77a amount=1599 status=ok',
  '2026-06-25T14:02:13Z ERROR append failed table=settlements code=23505 duplicate_key',
  '2026-06-25T14:02:13Z INFO  retry scheduled key=acct_2210:ch_77a in=200ms',
  '2026-06-25T14:02:13Z INFO  req settled account=acct_5531 charge=ch_3a2 amount=8800 status=ok',
  '2026-06-25T14:02:14Z DEBUG dedup map size=1284 horizon_ms=300000',
  '2026-06-25T14:02:14Z INFO  req settled account=acct_9001 charge=ch_b40 amount=300 status=ok',
  '2026-06-25T14:02:15Z WARN  unknown currency currency=chf dropped=true',
  '2026-06-25T14:02:15Z INFO  bus lag=0 throughput=512/s',
]);

const stagingEnv = join([
  '# svc-payments — staging',
  'DATABASE_URL=postgres://localhost:5432/payments',
  'BUS_BROKERS=localhost:9092',
  'FEATURE_SETTLEMENT_BUS=true',
  'DEDUP_HORIZON_MS=300000',
  'LOG_LEVEL=debug',
]);

const configYaml = join([
  'service: svc-payments',
  'settlement:',
  '  transport: bus',
  '  dedup_horizon_ms: 300000',
  '  currencies: [usd, eur, gbp]',
  'observability:',
  '  level: info',
  '  exporter: otlp',
]);

const releaseHtml = join([
  '<section style="font-family:system-ui; max-width:540px">',
  '  <h1 style="margin:0 0 8px">Writ 1.0 is here</h1>',
  '  <p style="color:#5a5a6a; line-height:1.5">Render Markdown, HTML, Mermaid and KaTeX the',
  '  moment you open a file — fully offline, with every scratch searchable.</p>',
  '  <p><a href="#" style="color:#3b5bdb">Get it for macOS, Windows or Linux →</a></p>',
  '</section>',
]);

const writ1559 = join([
  '# scratch — retry policy',
  '',
  'Agent proposed the backoff for `handleSettlement`:',
  '',
  '- max attempts: **5**',
  '- backoff: exponential, base `200ms`',
  '- jitter: full',
  '',
  '> paste back into the worker once confirmed',
]);

const writ4471 = join([
  '# scratch — auth flow',
  '',
  '```mermaid',
  'sequenceDiagram',
  '  Client->>Gateway: POST /token',
  '  Gateway->>Auth: validate',
  '  Auth-->>Gateway: access + refresh',
  '  Gateway-->>Client: 200 (access, refresh)',
  '```',
]);

const writ3182 = join([
  '# scratch — gradient step',
  '',
  'The update rule:',
  '',
  '$$\\theta_{t+1} = \\theta_t - \\eta\\,\\nabla_\\theta J(\\theta_t)$$',
  '',
  'with learning rate $\\eta = 0.01$ and batch size $n = 32$.',
]);

function buildBigLog(): string {
  const lines: string[] = [];
  const accts = ['acct_8842', 'acct_2210', 'acct_5531', 'acct_9001', 'acct_4417', 'acct_7780'];
  const chs = ['ch_91f', 'ch_77a', 'ch_3a2', 'ch_b40', 'ch_0d1', 'ch_e52'];
  for (let i = 0; i < 1400; i++) {
    const s = 11 + (i % 47);
    const a = accts[i % accts.length];
    const c = chs[i % chs.length];
    const amt = 100 + ((i * 37) % 9000);
    lines.push(
      '2026-06-25T14:' +
        String(2 + (i % 57)).padStart(2, '0') +
        ':' +
        String(s).padStart(2, '0') +
        'Z INFO  req settled account=' +
        a +
        ' charge=' +
        c +
        ' amount=' +
        amt +
        ' status=ok',
    );
  }
  return lines.join('\n');
}

export const BUFFERS: Record<string, BufferMeta> = {
  'report.md': { name: 'report.md', lang: 'md', label: 'Markdown', dot: 'var(--accent)' },
  'settle.ts': { name: 'settle.ts', lang: 'ts', label: 'TypeScript', dot: 'var(--sx-fn)' },
  'schema.sql': { name: 'schema.sql', lang: 'plain', label: 'Plain Text', dot: 'var(--sx-type)' },
  'gateway.log': { name: 'gateway.log', lang: 'plain', label: 'Plain Text', dot: 'var(--muted)' },
  'icon-256.png': {
    name: 'icon-256.png',
    lang: 'binary',
    label: 'Binary',
    dot: 'var(--warn)',
    badge: 'Binary · read-only',
    tok: '—',
  },
  'gateway-week.log': {
    name: 'gateway-week.log',
    lang: 'huge',
    label: 'Plain Text',
    dot: 'var(--muted)',
    badge: 'Large file · syntax off',
  },
  'staging.env': { name: 'staging.env', lang: 'plain', label: 'Plain Text', dot: 'var(--sx-num)' },
  'release-email.html': {
    name: 'release-email.html',
    lang: 'html',
    label: 'HTML',
    dot: 'var(--sx-kw)',
  },
  'config.yaml': { name: 'config.yaml', lang: 'plain', label: 'Plain Text', dot: 'var(--sx-type)' },
  'writ-1559': { name: 'writ-1559', lang: 'md', label: 'Markdown', dot: 'var(--accent)' },
  'writ-4471': { name: 'writ-4471', lang: 'md', label: 'Markdown', dot: 'var(--accent)' },
  'writ-3182': { name: 'writ-3182', lang: 'md', label: 'Markdown', dot: 'var(--accent)' },
};

export const DEFAULT_CONTENTS: Record<string, string> = {
  'report.md': reportMd,
  'settle.ts': settleTs,
  'schema.sql': schemaSql,
  'gateway.log': gatewayLog,
  'icon-256.png': '',
  'gateway-week.log': buildBigLog(),
  'staging.env': stagingEnv,
  'release-email.html': releaseHtml,
  'config.yaml': configYaml,
  'writ-1559': writ1559,
  'writ-4471': writ4471,
  'writ-3182': writ3182,
};

export const FMT: Record<'md' | 'html' | 'mermaid' | 'math', string> = {
  md: 'report.md',
  html: 'release-email.html',
  mermaid: 'writ-4471',
  math: 'writ-3182',
};

export const OPEN_FILES = ['report.md', 'settle.ts', 'schema.sql', 'gateway.log'];

export const HISTORY: { id: string; when: string }[] = [
  { id: 'staging.env', when: '1m' },
  { id: 'release-email.html', when: '2m' },
  { id: 'config.yaml', when: '6m' },
  { id: 'writ-1559', when: '9m' },
  { id: 'writ-4471', when: '12m' },
  { id: 'writ-3182', when: '14m' },
];

export const GROUPS: PaletteGroup[] = [
  {
    label: 'FORMAT',
    cmds: [
      { id: 'fmt-bold', name: 'Bold', desc: 'Wrap the selection in **.', kbd: '⌘B' },
      { id: 'fmt-italic', name: 'Italic', desc: 'Wrap the selection in *.', kbd: '⌘I' },
      { id: 'fmt-strike', name: 'Strikethrough', desc: 'Wrap the selection in ~~.', kbd: '⌘⇧X' },
      { id: 'fmt-code', name: 'Inline code', desc: 'Wrap the selection in backticks.', kbd: '⌘E' },
      { id: 'fmt-link', name: 'Link', desc: 'Turn the selection into a Markdown link.', kbd: '⌘K' },
    ],
  },
  {
    label: 'TRANSFORM',
    cmds: [
      { id: 'trim', name: 'Trim Leading Whitespace', desc: 'Remove leading spaces and tabs from each line.' },
      { id: 'dedent', name: 'Dedent', desc: 'Remove shared leading indentation.' },
      { id: 'finalnl', name: 'Ensure Final Newline', desc: 'End with exactly one trailing newline.' },
      { id: 'prompt', name: 'Prepare as Prompt', desc: 'Strip frontmatter and comments outside code fences.' },
      { id: 'tidy', name: 'Tidy Whitespace', desc: 'Trim, dedent, collapse blank runs, final newline.' },
      { id: 'normalize', name: 'Normalize Whitespace', desc: 'Collapse repeated spaces and tabs to one.' },
      { id: 'punct', name: 'Fix Punctuation Spacing', desc: 'Remove stray spaces before punctuation.' },
      { id: 'quotes', name: 'Smart → Straight Quotes', desc: 'Replace curly quotes with ASCII quotes.' },
    ],
  },
  {
    label: 'TABS & FILES',
    cmds: [
      { id: 'newtab', name: 'New Tab', desc: 'Open a fresh scratch buffer.', kbd: '⌘T' },
      { id: 'closetab', name: 'Close Tab', desc: 'Close the active buffer.', kbd: '⌘W' },
      { id: 'switchtab', name: 'Switch Tab', desc: 'Jump to the next open buffer.', kbd: '⌘]' },
      { id: 'renametab', name: 'Rename Tab', desc: 'Rename the active buffer inline.' },
    ],
  },
  {
    label: 'VIEW',
    cmds: [
      { id: 'togglesidebar', name: 'Toggle Sidebar', desc: 'Show or hide the buffer list.', kbd: '⌘S' },
      { id: 'find', name: 'Find', desc: 'Focus the search field.', kbd: '⌘F' },
      { id: 'zoomin', name: 'Zoom In', desc: 'Increase editor scale.', kbd: '⌘+' },
      { id: 'zoomout', name: 'Zoom Out', desc: 'Decrease editor scale.', kbd: '⌘−' },
      { id: 'zoomreset', name: 'Reset Zoom', desc: 'Return to 100%.', kbd: '⌘0' },
    ],
  },
  {
    label: 'ACTIONS',
    cmds: [
      { id: 'search', name: 'Search', desc: 'Full-text search across every buffer.' },
      { id: 'watchinbox', name: 'Watch Inbox', desc: 'Auto-open files dropped into the inbox folder.' },
      { id: 'copyprompt', name: 'Copy as Prompt', desc: 'Copy the buffer, cleaned, to the clipboard.' },
    ],
  },
];
