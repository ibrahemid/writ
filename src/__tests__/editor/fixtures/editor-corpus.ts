// Deterministic documents for the editor's find and perf budgets, the same
// generators the editor perf harness (scripts/perf-editor) serves. Every
// generator writes exactly `size` UTF-8 bytes from a fixed seed, so two runs
// produce identical files and a changed number is a changed editor.

export type CorpusKind = "prose" | "single-line-json" | "markdown" | "log";

export interface CorpusSpec {
  name: string;
  kind: CorpusKind;
  size: number;
  /** Markdown only: bytes per section, which sets the heading count. */
  sectionBytes?: number;
  /** Log only: the line count; the size follows from it. */
  lines?: number;
}

export interface Corpus {
  bytes: Uint8Array;
  lines: number;
  headings: number;
  fences: number;
  links: number;
}

/** The word every generator plants about once per 100 KB. */
export const RARE_WORD = "zebrafish";

function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const WORDS = (
  "the of and to in is that it was for on are with as his they be at one have this from or had by " +
  "word but what some we can out other were all there when up use your how said an each she which do " +
  "their time if will way about many then them write would like so these her long make thing see him " +
  "two has look more day could go come did number sound no most people my over know water than call " +
  "first who may down side been now find any new work part take get place made live where after back " +
  "little only round man year came show every good me give our under name very through just form " +
  "sentence great think say help low line differ turn cause much mean before move right boy old too " +
  "same tell does set three want air well also play small end put home read hand port large spell add " +
  "even land here must big high such follow act why ask men change went light kind off need house " +
  "picture try us again animal point mother world near build self earth father café naïve résumé " +
  "committee garden schedule budget minutes agenda proposal quarterly migration renderer viewport"
).split(" ");

class Sink {
  readonly size: number;
  readonly bytes: Uint8Array;
  length = 0;
  lines = 0;
  private readonly encoder = new TextEncoder();

  constructor(size: number) {
    this.size = size;
    this.bytes = new Uint8Array(size);
  }

  get full(): boolean {
    return this.length >= this.size;
  }

  /** Appends `text`, truncated at the size on an ASCII byte; false once full. */
  push(text: string): boolean {
    if (this.full) return false;
    const encoded = this.encoder.encode(text);
    const room = this.size - this.length;
    if (encoded.length <= room) {
      this.bytes.set(encoded, this.length);
      this.length += encoded.length;
      for (let i = 0; i < encoded.length; i++) if (encoded[i] === 10) this.lines++;
      return true;
    }
    let cut = room;
    while (cut > 0 && encoded[cut - 1] >= 0x80) cut--;
    this.bytes.set(encoded.subarray(0, cut), this.length);
    for (let i = 0; i < cut; i++) if (encoded[i] === 10) this.lines++;
    this.length += cut;
    while (this.length < this.size) this.bytes[this.length++] = 32;
    return false;
  }
}

interface RarePlan {
  next: number;
}

function sentence(
  rand: () => number,
  minLength: number,
  maxLength: number,
  written: number,
  rareEvery: number,
  plan: RarePlan,
): string {
  const target = minLength + Math.floor(rand() * (maxLength - minLength));
  let out = "";
  while (out.length < target) {
    let word = WORDS[Math.floor(rand() * WORDS.length)];
    if (written + out.length >= plan.next) {
      word = RARE_WORD;
      plan.next += rareEvery;
    }
    out += out ? ` ${word}` : word[0].toUpperCase() + word.slice(1);
  }
  return `${out}.`;
}

function prose(size: number, seed: number): Corpus {
  const rand = mulberry32(seed);
  const sink = new Sink(size);
  const plan = { next: 50_000 };
  while (!sink.full) {
    const lines = 3 + Math.floor(rand() * 6);
    let paragraph = "";
    for (let i = 0; i < lines; i++) {
      paragraph += `${sentence(rand, 50, 110, sink.length + paragraph.length, 100_000, plan)}\n`;
    }
    if (!sink.push(`${paragraph}\n`)) break;
  }
  return { bytes: sink.bytes, lines: sink.lines + 1, headings: 0, fences: 0, links: 0 };
}

function singleLineJson(size: number, seed: number): Corpus {
  const rand = mulberry32(seed);
  const sink = new Sink(size);
  sink.push("[");
  for (let i = 0; !sink.full; i++) {
    const tagA = WORDS[Math.floor(rand() * WORDS.length)];
    const tagB = WORDS[Math.floor(rand() * WORDS.length)];
    const item = `{"id":${i},"name":"item-${i}","tags":["${tagA}","${tagB}"],"value":${rand().toFixed(6)},"ok":${rand() > 0.5}},`;
    if (!sink.push(item)) break;
  }
  return { bytes: sink.bytes, lines: 1, headings: 0, fences: 0, links: 0 };
}

const FENCE_LANGUAGES = ["js", "python", "rust", "sh", "json", "sql"];
const FENCE_BODIES: Record<string, string[]> = {
  js: ["const total = items.reduce((sum, item) => sum + item.value, 0);", "export function render(view) {", "  return view.state.doc.length > 0;", "}"],
  python: ["def total(items):", "    return sum(i.value for i in items)", "", "print(total(load()))"],
  rust: ["fn total(items: &[Item]) -> u64 {", "    items.iter().map(|i| i.value).sum()", "}"],
  sh: ["cargo test --workspace", "pnpm build", "ls -la ~/Writ | head"],
  json: ['{ "name": "writ", "version": "0.5.0", "private": true }'],
  sql: ["SELECT id, title FROM notes WHERE updated_at > ?1 ORDER BY updated_at DESC;"],
};

function markdown(size: number, seed: number, sectionBytes: number): Corpus {
  const rand = mulberry32(seed);
  const sink = new Sink(size);
  const plan = { next: 50_000 };
  const pick = () => WORDS[Math.floor(rand() * WORDS.length)];
  let headings = 1;
  let fences = 0;
  let links = 0;
  sink.push("# Field notes\n\nA long working document.\n\n");
  for (let section = 1; !sink.full; section++) {
    const level = section % 10 === 0 ? 2 : 3;
    let text = `${"#".repeat(level)} Section ${section}: ${pick()} ${pick()}\n\n`;
    headings++;
    while (text.length < sectionBytes) {
      const roll = rand();
      if (roll < 0.45) {
        let paragraph = sentence(rand, 60, 160, sink.length + text.length, 100_000, plan);
        paragraph = paragraph.replace(/ (\w+) (\w+) /, " **$1** *$2* ");
        if (rand() < 0.5) {
          paragraph += ` See [the ${pick()} page](https://example.com/docs/${section}/${Math.floor(rand() * 1000)}).`;
          links++;
        }
        if (rand() < 0.3) {
          paragraph += ` Related: [[Note ${Math.floor(rand() * 500)}]].`;
          links++;
        }
        if (rand() < 0.3) paragraph += " Run `pnpm build` first.";
        text += `${paragraph}\n\n`;
      } else if (roll < 0.65) {
        const items = 2 + Math.floor(rand() * 4);
        for (let i = 0; i < items; i++) {
          const marker = rand() < 0.3 ? "- [ ] " : rand() < 0.5 ? "1. " : "- ";
          text += `${marker}${sentence(rand, 20, 60, sink.length + text.length, 100_000, plan)}\n`;
        }
        text += "\n";
      } else if (roll < 0.78) {
        const language = FENCE_LANGUAGES[Math.floor(rand() * FENCE_LANGUAGES.length)];
        text += `\`\`\`${language}\n${FENCE_BODIES[language].join("\n")}\n\`\`\`\n\n`;
        fences++;
      } else if (roll < 0.86) {
        text += `> ${sentence(rand, 40, 120, sink.length + text.length, 100_000, plan)}\n\n`;
      } else if (roll < 0.92) {
        text += "| Item | Owner | Due |\n|---|---|---|\n";
        for (let i = 0; i < 3; i++) text += `| ${pick()} | ${pick()} | 2026-10-${10 + i} |\n`;
        text += "\n";
      } else {
        text += `${sentence(rand, 80, 200, sink.length + text.length, 100_000, plan)} [link ${links}](https://example.org/${links}) and <https://writ.example/${section}>.\n\n`;
        links += 2;
      }
    }
    if (!sink.push(text)) break;
  }
  return { bytes: sink.bytes, lines: sink.lines + 1, headings, fences, links };
}

const LEVELS = ["INFO ", "INFO ", "INFO ", "DEBUG", "WARN ", "INFO ", "ERROR"];
const VERBS = ["GET", "GET", "POST", "PUT", "DELETE"];

function log(lines: number, seed: number): Corpus {
  const rand = mulberry32(seed);
  const parts: string[] = [];
  let t = Date.UTC(2026, 9, 3, 8, 0, 0);
  for (let i = 0; i < lines; i++) {
    t += Math.floor(rand() * 40);
    const level = LEVELS[Math.floor(rand() * LEVELS.length)];
    const stamp = new Date(t).toISOString();
    const worker = String(Math.floor(rand() * 16)).padStart(2, "0");
    const verb = VERBS[Math.floor(rand() * VERBS.length)];
    const id = Math.floor(rand() * 100000);
    const status = level === "ERROR" ? 500 : rand() < 0.05 ? 404 : 200;
    const request = Math.floor(rand() * 0xffffffff).toString(16).padStart(8, "0");
    const rare = i % 997 === 0 ? ` note=${RARE_WORD}` : "";
    parts.push(
      `${stamp} ${level} [worker-${worker}] ${verb} /api/v1/items/${id} status=${status} bytes=${Math.floor(rand() * 9000)} dur=${Math.floor(rand() * 300)}ms req=${request}${rare}\n`,
    );
  }
  const bytes = new TextEncoder().encode(parts.join(""));
  return { bytes, lines: lines + 1, headings: 0, fences: 0, links: 0 };
}

export function generateCorpus(spec: CorpusSpec, seed = 1337): Corpus {
  switch (spec.kind) {
    case "prose":
      return prose(spec.size, seed);
    case "single-line-json":
      return singleLineJson(spec.size, seed);
    case "markdown":
      return markdown(spec.size, seed, spec.sectionBytes ?? 10_000);
    case "log":
      return log(spec.lines ?? 200_000, seed);
  }
}

/** The generated document as text, the way the editor holds it. */
export function generateCorpusText(spec: CorpusSpec, seed = 1337): string {
  return new TextDecoder().decode(generateCorpus(spec, seed).bytes);
}
