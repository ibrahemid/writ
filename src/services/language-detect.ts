const SHEBANG_MAP: Record<string, string> = {
  python: "python",
  python3: "python",
  node: "javascript",
  bash: "shell",
  sh: "shell",
  zsh: "shell",
  ruby: "ruby",
  perl: "perl",
  php: "php",
};

// Writ's own extension map. A name whose extension is not here is looked up in
// the editor's language table (src/editor/languages.ts) before it is called
// plain text.
const EXTENSION_MAP: Record<string, string> = {
  js: "javascript",
  jsx: "javascript",
  ts: "typescript",
  // TypeScript with JSX: the plain TypeScript grammar reads a tag as a type
  // assertion.
  tsx: "tsx",
  py: "python",
  rb: "ruby",
  rs: "rust",
  go: "go",
  java: "java",
  php: "php",
  html: "html",
  css: "css",
  json: "json",
  md: "markdown",
  yaml: "yaml",
  yml: "yaml",
  sql: "sql",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  toml: "toml",
  xml: "xml",
};

// Names that say plain text (ADR-041 §2). Nothing a file holds turns one into
// code; a person can still pick a language for it.
const PLAIN_TEXT_EXTENSIONS = new Set(["txt", "text"]);

const MIN_CONTENT_LENGTH = 30;
const SCORE_THRESHOLD = 3;
const WINNER_MARGIN = 2;

// Content-based detection scores the whole input through a stack of regexes,
// and `scoreJson` does a full `JSON.parse`. On a multi-megabyte buffer that is
// tens of milliseconds per call, fired every few keystrokes. The language of a
// file is determined by its opening structure (shebang, imports, headings,
// the first `{`), so scoring only the head is both faster and just as accurate.
export const MAX_DETECT_BYTES = 64 * 1024;

function scoreRust(text: string): number {
  let s = 0;
  if (/\bfn\s+\w+\s*\(/.test(text)) s += 3;
  if (/\bpub\s+(fn|struct|enum|mod)\b/.test(text)) s += 2;
  if (/\buse\s+[\w:]+(::\{?[\w,\s]+\}?)?\s*;/.test(text)) s += 2;
  if (/\blet\s+(mut\s+)?\w+\s*[:=]/.test(text)) s += 1;
  if (/\bimpl\s+(<[^>]+>\s+)?\w+/.test(text)) s += 2;
  if (/\b(struct|enum|trait)\s+\w+/.test(text)) s += 2;
  if (/->\s*[&\w<]/.test(text)) s += 1;
  if (/&str|String|Vec<|Option<|Result<|Box</.test(text)) s += 2;
  if (/::\w/.test(text)) s += 1;
  if (/\bmatch\s+\w+\s*\{/.test(text)) s += 1;
  return s;
}

function scoreTypescript(text: string): number {
  let s = 0;
  if (/\binterface\s+\w+\s*\{/.test(text)) s += 3;
  if (/:\s*(string|number|boolean|void|never|any|unknown|Array<|Promise<)\b/.test(text)) s += 3;
  if (/\btype\s+\w+\s*=/.test(text)) s += 2;
  if (/\bimport\s+(\{[^}]+\}|\w+|\*\s+as\s+\w+)\s+from\s+["']/.test(text)) s += 1;
  if (/\bexport\s+(default\s+)?(function|class|const|let|interface|type|enum)\b/.test(text)) s += 1;
  if (/\bas\s+const\b/.test(text)) s += 1;
  if (/\benum\s+\w+\s*\{/.test(text)) s += 2;
  return s;
}

function scoreJavascript(text: string): number {
  let s = 0;
  if (/\b(const|let|var)\s+\w+\s*=/.test(text)) s += 1;
  if (/\bfunction\s+\w+\s*\(/.test(text)) s += 2;
  if (/\bimport\s+(\{[^}]+\}|\w+|\*\s+as\s+\w+)\s+from\s+["']/.test(text)) s += 2;
  if (/\bmodule\.exports\s*=/.test(text)) s += 2;
  if (/\b(async|await)\b/.test(text)) s += 1;
  if (/=>\s*[\w{(]/.test(text)) s += 1;
  if (/console\.(log|warn|error)\(/.test(text)) s += 1;
  return s;
}

function scorePython(text: string): number {
  let s = 0;
  if (/^def\s+\w+\s*\(/m.test(text)) s += 3;
  if (/^from\s+[\w.]+\s+import\b/m.test(text)) s += 3;
  if (/^import\s+\w+/m.test(text)) s += 2;
  if (/^class\s+\w+(\([^)]*\))?\s*:/m.test(text)) s += 2;
  if (/^\s+(if|for|while|elif|else|try|except|with)\b.*:\s*$/m.test(text)) s += 1;
  if (/\b(self|None|True|False)\b/.test(text)) s += 1;
  if (/__init__|__main__|__name__/.test(text)) s += 2;
  if (/\bprint\s*\(/.test(text)) s += 1;
  return s;
}

function scoreJson(text: string): number {
  const trimmed = text.trim();
  if (!trimmed) return 0;
  const first = trimmed[0];
  const last = trimmed[trimmed.length - 1];
  if (!((first === "{" && last === "}") || (first === "[" && last === "]"))) return 0;
  try {
    JSON.parse(trimmed);
    return 10;
  } catch {
    return 0;
  }
}

function scoreMarkdown(text: string): number {
  let s = 0;
  if (/^#{1,6}\s+\S/m.test(text)) s += 3;
  if (/^[-*+]\s+\S/m.test(text)) s += 2;
  if (/\[[^\]]+\]\([^)]+\)/.test(text)) s += 2;
  if (/^```\w*$/m.test(text)) s += 3;
  if (/^>\s+\S/m.test(text)) s += 1;
  if (/^\d+\.\s+\S/m.test(text)) s += 1;
  if (/\*\*[^*]+\*\*|__[^_]+__/.test(text)) s += 1;
  return s;
}

function scoreHtml(text: string): number {
  let s = 0;
  if (/<!DOCTYPE\s+html/i.test(text)) s += 5;
  if (/<html[\s>]/i.test(text)) s += 3;
  if (/<\/?(head|body|div|span|p|h[1-6]|a|img|script|style)[\s>]/i.test(text)) s += 2;
  return s;
}

function scoreShell(text: string): number {
  let s = 0;
  if (/^#!.*\b(bash|sh|zsh)\b/m.test(text)) s += 5;
  if (/\bif\s+\[\s+.+\s+\]\s*;\s*then\b/.test(text)) s += 2;
  if (/\b(then|fi|elif|esac)\b/.test(text)) s += 1;
  if (/\$\{?\w+\}?/.test(text)) s += 1;
  if (/^\s*(echo|export|source)\s+/m.test(text)) s += 1;
  return s;
}

// The order breaks no ties (a tie is no answer); it is the order the scores are
// listed in when they are compared.
const CONTENT_SCORERS: ReadonlyArray<readonly [string, (text: string) => number]> = [
  ["json", scoreJson],
  ["rust", scoreRust],
  ["typescript", scoreTypescript],
  ["python", scorePython],
  ["markdown", scoreMarkdown],
  ["html", scoreHtml],
  ["shell", scoreShell],
  ["javascript", scoreJavascript],
];

/** Every id the name map, the shebang map and the content scorers can answer. */
export const DETECTABLE_LANGUAGE_IDS: readonly string[] = [
  ...new Set([
    ...Object.values(EXTENSION_MAP),
    ...Object.values(SHEBANG_MAP),
    ...CONTENT_SCORERS.map(([id]) => id),
  ]),
];

export function detectFromContent(text: string): string | null {
  if (!text) return null;
  const head = text.length > MAX_DETECT_BYTES ? text.slice(0, MAX_DETECT_BYTES) : text;
  if (head.trim().length < MIN_CONTENT_LENGTH) return null;

  const scores: Array<[string, number]> = CONTENT_SCORERS.map(([id, score]) => [id, score(head)]);

  scores.sort((a, b) => b[1] - a[1]);
  const [topLang, topScore] = scores[0];
  const secondScore = scores[1][1];

  if (topScore < SCORE_THRESHOLD) return null;
  if (topScore - secondScore < WINNER_MARGIN) return null;
  return topLang;
}

/**
 * The extension a file name ends with, lower case, or null when it has none.
 *
 * A leading dot starts a name rather than an extension: `.bashrc` has none,
 * `.bashrc.sh` has `sh`.
 */
export function extensionOf(name: string): string | null {
  const base = name.slice(Math.max(name.lastIndexOf("/"), name.lastIndexOf("\\")) + 1);
  const dot = base.lastIndexOf(".");
  if (dot <= 0 || dot === base.length - 1) return null;
  return base.slice(dot + 1).toLowerCase();
}

/** Whether a name says plain text (`.txt`, `.text`). */
export function namesPlainText(name: string): boolean {
  const ext = extensionOf(name);
  return ext !== null && PLAIN_TEXT_EXTENSIONS.has(ext);
}

/**
 * Whether the text may say what language a file is in: only when its name
 * says nothing, because it has no extension. A name with an extension decides,
 * whatever the text holds or comes to hold.
 */
export function textMayNameLanguage(name: string | undefined): boolean {
  return !name || extensionOf(name) === null;
}

// The first line alone, without splitting the whole document into lines.
function firstLineOf(content: string): string {
  const end = content.indexOf("\n");
  const line = end === -1 ? content : content.slice(0, end);
  return line.endsWith("\r") ? line.slice(0, -1) : line;
}

function languageFromShebang(line: string): string | null {
  if (!line.startsWith("#!")) return null;
  const parts = line.split("/");
  const interpreter = parts[parts.length - 1].split(" ")[0];
  return SHEBANG_MAP[interpreter] ?? null;
}

/**
 * The language a file is in, from its name and, for a name with no extension,
 * its first line.
 *
 * A fenced block names the language of the fence only. A name that says plain
 * text answers null. A name whose extension this map does not list answers
 * null here, and the editor then asks its own table.
 */
export function detectLanguage(content: string, filename?: string): string | null {
  if (filename) {
    const ext = extensionOf(filename);
    if (ext !== null) {
      if (PLAIN_TEXT_EXTENSIONS.has(ext)) return null;
      return EXTENSION_MAP[ext] ?? null;
    }
  }
  return languageFromShebang(firstLineOf(content));
}
