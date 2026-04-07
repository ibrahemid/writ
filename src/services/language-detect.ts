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
