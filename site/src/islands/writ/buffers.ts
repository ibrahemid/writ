import type { TransformId } from './transforms';

export type BufferLang = 'md' | 'ts' | 'html' | 'plain' | 'binary' | 'huge';

export interface BufferMeta {
  name: string;
  lang: BufferLang;
  badge?: string;
  tok?: string;
}

// Maps a demo buffer's language tag to the app's registry language id (null =
// plain text). Binary and huge buffers carry no grammar in the app either.
export function cmLangId(lang: BufferLang): string | null {
  switch (lang) {
    case 'md':
      return 'markdown';
    case 'ts':
      return 'typescript';
    case 'html':
      return 'html';
    default:
      return null;
  }
}

const join = (lines: string[]): string => lines.join('\n');

const meetingNotesMd = join([
  '# Team sync, Tuesday',
  '',
  'Present: Dana, Omar, Lucy. Twenty minutes, mostly the launch date.',
  '',
  '## Decisions',
  '',
  '- Launch moves to the 14th. Dana calls the printer today.',
  '- One location for the photo shoot instead of two.',
  '',
  '## Actions',
  '',
  '- [x] Send Dana the venue quote',
  '- [ ] Confirm the caterer headcount by Friday',
  '- [ ] Ask Lucy for the updated slides',
  '- [ ] Book the van for the 13th',
  '',
  '## Open questions',
  '',
  'Omar wants the printing budget split out from the general line. Nobody could remember what the deposit was, so it goes on next week\'s list.',
  '',
  '> Next sync Tuesday, same time, same room.',
]);

const tripPackingMd = join([
  '# Lisbon, five days',
  '',
  'One carry-on. Laundry at the flat on Wednesday, so pack for three days and repeat.',
  '',
  '## Bag',
  '',
  '- [x] Passport and the printed booking',
  '- [ ] Plug adapter, type F',
  '- [ ] Charger and the short cable',
  '- [ ] Two shirts, one jumper, one pair of shoes',
  '- [ ] Sunscreen, the small bottle',
  '',
  '## Before leaving',
  '',
  '- [ ] Water the plants',
  '- [ ] Bins out Thursday night',
  '- [ ] Spare key to Sam',
]);

const recipeMd = join([
  '# Tomato and bread soup',
  '',
  'Serves four. Forty minutes, most of it waiting.',
  '',
  '## What you need',
  '',
  '- 800 g ripe tomatoes, or two tins',
  '- 250 g stale bread, crusts on',
  '- 3 cloves of garlic',
  '- A handful of basil',
  '- Olive oil, salt, pepper',
  '',
  '## What to do',
  '',
  '1. Warm the oil and cook the garlic gently until it smells sweet. Do not let it brown.',
  '2. Add the tomatoes and a cup of water. Simmer 15 minutes.',
  '3. Tear the bread in, stir, take the pan off the heat and leave it 10 minutes.',
  '4. Beat it with a spoon until it thickens. Basil, oil and salt at the end.',
  '',
  'Better the next day. Reheat it slowly and add water.',
]);

const draftEmailMd = join([
  '# Draft, reply to the landlord',
  '',
  '**Subject:** Radiator in the back room',
  '',
  'Hi Peter,',
  '',
  'The radiator in the back room has not warmed up since the weekend. The others are fine, so I do not think it is the boiler. I bled it on Sunday and nothing came out.',
  '',
  'Can you send someone this week? I am in Thursday and Friday after 4pm, and any time Saturday.',
  '',
  'Thanks',
  '',
  '---',
  '',
  'Second paragraph stays short on purpose. Send it in the morning.',
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

const readingListMd = join([
  '# Reading list',
  '',
  '- The book Lucy lent me. Bought a copy, still on the shelf.',
  '- Two long pieces on the Lisbon earthquake, saved to the folder.',
  '- The article Omar sent about printing costs. Skimmed, worth a second pass.',
  '- Something about sourdough that turned out to be an advert.',
]);

const newsletterHtml = join([
  '<section style="font-family:system-ui; max-width:540px">',
  '  <h1 style="margin:0 0 8px">Allotment newsletter, March</h1>',
  '  <p style="color:#5a5a6a; line-height:1.5">Plot inspections are on the 12th. The water is back',
  '  on from the first weekend, and three plots at the top end are free.</p>',
  '  <p><a href="#" style="color:#3b5bdb">Put your name down for a plot →</a></p>',
  '</section>',
]);

const notesMd = join([
  '# scratch, notes',
  '',
  'Quick pass over teh packing list before Friday.',
  '',
  '- recieve the tickets from Dana, they are not in the folder yet',
  '- keep the receipts seperate from the travel notes',
  '- check the enviroment tab in the camera settings before packing it',
]);

const writ1559 = join([
  '# scratch, from the call',
  '',
  'Numbers Dana read out, before I lose them:',
  '',
  '- deposit **450**, paid in January',
  '- balance due 14 days before',
  '- full refund up to 30 days out',
  '',
  '> check this against the email',
]);

const writ4471 = join([
  '# scratch, moving day',
  '',
  '```mermaid',
  'flowchart TD',
  '  A[Keys at 9] --> B[Van loaded]',
  '  B --> C{Lift working?}',
  '  C -->|yes| D[Boxes up first]',
  '  C -->|no| E[Furniture first, up the stairs]',
  '  D --> F[Beds built by 6]',
  '  E --> F',
  '```',
]);

const writ3182 = join([
  '# scratch, the loan',
  '',
  'Monthly payment on a repayment loan:',
  '',
  '$$M = P\\,\\frac{r(1+r)^n}{(1+r)^n - 1}$$',
  '',
  'with $P = 18000$, a monthly rate $r = 0.0042$ and $n = 60$ payments.',
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

// Demo seed. Keys are buffer ids and `name` is the label the window renders, so
// the two can differ: `report.md` is the id WritWindow.tsx opens on mount and
// falls back to, while the tab reads meeting-notes.md.
export const BUFFERS: Record<string, BufferMeta> = {
  'report.md': { name: 'meeting-notes.md', lang: 'md' },
  'trip-packing.md': { name: 'trip-packing.md', lang: 'md' },
  'recipe.md': { name: 'recipe.md', lang: 'md' },
  'draft-email.md': { name: 'draft-email.md', lang: 'md' },
  'notes.md': { name: 'notes.md', lang: 'md' },
  'reading-list.md': { name: 'reading-list.md', lang: 'md' },
  'newsletter.html': { name: 'newsletter.html', lang: 'html' },
  'settle.ts': { name: 'settle.ts', lang: 'ts' },
  'icon-256.png': {
    name: 'icon-256.png',
    lang: 'binary',
    badge: 'Binary · read-only',
    tok: '—',
  },
  'gateway-week.log': {
    name: 'gateway-week.log',
    lang: 'huge',
    badge: 'Large file · syntax off',
  },
  'writ-1559': { name: 'writ-1559', lang: 'md' },
  'writ-4471': { name: 'writ-4471', lang: 'md' },
  'writ-3182': { name: 'writ-3182', lang: 'md' },
};

export const DEFAULT_CONTENTS: Record<string, string> = {
  'report.md': meetingNotesMd,
  'trip-packing.md': tripPackingMd,
  'recipe.md': recipeMd,
  'draft-email.md': draftEmailMd,
  'notes.md': notesMd,
  'reading-list.md': readingListMd,
  'newsletter.html': newsletterHtml,
  'settle.ts': settleTs,
  'icon-256.png': '',
  'gateway-week.log': buildBigLog(),
  'writ-1559': writ1559,
  'writ-4471': writ4471,
  'writ-3182': writ3182,
};

export const FMT: Record<'md' | 'html' | 'mermaid' | 'math', string> = {
  md: 'report.md',
  html: 'newsletter.html',
  mermaid: 'writ-4471',
  math: 'writ-3182',
};

export const OPEN_FILES = ['report.md', 'trip-packing.md', 'recipe.md', 'draft-email.md'];

export const HISTORY: { id: string; when: string }[] = [
  { id: 'notes.md', when: '1m' },
  { id: 'reading-list.md', when: '4m' },
  { id: 'newsletter.html', when: '7m' },
  { id: 'writ-4471', when: '11m' },
  { id: 'writ-3182', when: '16m' },
  { id: 'settle.ts', when: '24m' },
  { id: 'writ-1559', when: '31m' },
];

// The nine text transforms, with the app's plain labels and descriptions and in
// the app's palette order (kept in step with the transforms the app registers).
export const TEXT_TRANSFORMS: { id: TransformId; name: string; desc: string }[] = [
  { id: 'trim', name: 'Trim leading spaces', desc: 'Remove spaces and tabs from the start of each line.' },
  { id: 'trimtrailing', name: 'Trim trailing spaces', desc: 'Remove spaces and tabs from the end of each line.' },
  { id: 'normalize', name: 'Collapse repeated spaces', desc: 'Collapse repeated spaces and tabs inside a line down to one space.' },
  { id: 'quotes', name: 'Straighten quotes', desc: 'Replace curly quotes with straight ones.' },
  { id: 'dedent', name: 'Remove shared indentation', desc: 'Remove the indentation shared by every non-blank line.' },
  { id: 'finalnl', name: 'End with one newline', desc: 'End the text with exactly one trailing newline.' },
  { id: 'punct', name: 'Fix spacing before punctuation', desc: 'Remove stray spaces before commas, periods, and other punctuation.' },
  { id: 'prompt', name: 'Prepare as prompt', desc: 'Remove frontmatter and HTML comments outside code, then trim trailing whitespace.' },
  { id: 'tidy', name: 'Tidy whitespace', desc: 'Trim trailing spaces, remove shared indentation, collapse repeated spaces, and end with one newline.' },
];
