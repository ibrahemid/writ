// Maps an internal language id (the lowercase keys registered in
// editor/builtins.ts and produced by services/language-detect.ts) to a
// human-readable label for the status bar. Unknown ids are title-cased so a
// newly registered language still reads sensibly without a map entry.
const LABELS: Record<string, string> = {
  javascript: "JavaScript",
  typescript: "TypeScript",
  python: "Python",
  rust: "Rust",
  json: "JSON",
  html: "HTML",
  css: "CSS",
  markdown: "Markdown",
  php: "PHP",
  yaml: "YAML",
  sql: "SQL",
  toml: "TOML",
  xml: "XML",
  shell: "Shell",
  go: "Go",
  java: "Java",
  ruby: "Ruby",
  perl: "Perl",
};

const PLAIN_TEXT_LABEL = "Plain Text";

export function languageLabel(id: string | null | undefined): string {
  if (!id) return PLAIN_TEXT_LABEL;
  const known = LABELS[id];
  if (known) return known;
  return id.charAt(0).toUpperCase() + id.slice(1);
}
