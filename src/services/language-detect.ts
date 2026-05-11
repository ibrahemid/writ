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

const EXTENSION_MAP: Record<string, string> = {
  js: "javascript",
  jsx: "javascript",
  ts: "typescript",
  tsx: "typescript",
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
  toml: "toml",
  xml: "xml",
};

const MIN_CONTENT_LENGTH = 30;
const SCORE_THRESHOLD = 3;
const WINNER_MARGIN = 2;

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

export function detectFromContent(text: string): string | null {
  if (!text || text.trim().length < MIN_CONTENT_LENGTH) return null;

  const scores: Array<[string, number]> = [
    ["json", scoreJson(text)],
    ["rust", scoreRust(text)],
    ["typescript", scoreTypescript(text)],
    ["python", scorePython(text)],
    ["markdown", scoreMarkdown(text)],
    ["html", scoreHtml(text)],
    ["shell", scoreShell(text)],
    ["javascript", scoreJavascript(text)],
  ];

  scores.sort((a, b) => b[1] - a[1]);
  const [topLang, topScore] = scores[0];
  const secondScore = scores[1][1];

  if (topScore < SCORE_THRESHOLD) return null;
  if (topScore - secondScore < WINNER_MARGIN) return null;
  return topLang;
}

export function detectLanguage(content: string, filename?: string): string | null {
  if (filename) {
    const ext = filename.split(".").pop()?.toLowerCase();
    if (ext && EXTENSION_MAP[ext]) return EXTENSION_MAP[ext];
  }

  const firstLine = content.split("\n")[0] || "";

  if (firstLine.startsWith("#!")) {
    const parts = firstLine.split("/");
    const interpreter = parts[parts.length - 1].split(" ")[0];
    if (SHEBANG_MAP[interpreter]) return SHEBANG_MAP[interpreter];
  }

  if (content.includes("```")) {
    const match = content.match(/```(\w+)/);
    if (match) return match[1].toLowerCase();
  }

  return null;
}
