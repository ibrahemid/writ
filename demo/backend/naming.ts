// The host's naming rules for files Writ mints, in writ_core::notes and
// writ_core::startup, for the page's in-memory folder.

const ILLEGAL_CHARS = new Set(["/", "\\", ":", "*", "?", '"', "<", ">", "|"]);
const MAX_STEM_CHARS = 200;

/** writ_core::notes::minted_stem: `writ-<yymmdd>-<hhmm>` on the local clock. */
export function mintedStem(now: Date): string {
  const two = (n: number) => String(n).padStart(2, "0");
  return `writ-${two(now.getFullYear() % 100)}${two(now.getMonth() + 1)}${two(now.getDate())}-${two(now.getHours())}${two(now.getMinutes())}`;
}

function stripLineMarker(line: string): string {
  const heading = line.replace(/^#+/, "");
  if (heading.length !== line.length) return heading;
  for (const marker of ["- ", "* ", "+ ", "> "]) {
    if (line.startsWith(marker)) return line.slice(marker.length);
  }
  return line;
}

/** writ_core::startup::first_line_title: the name a first line gives a file, or null. */
export function firstLineTitle(content: string): string | null {
  const first = (content.split("\n")[0] ?? "").trim();
  if (first === "---") return null;
  const title = stripLineMarker(first).trim();
  if (!title || (title.startsWith("[[") && title.endsWith("]]"))) return null;
  if ([...title].every((c) => "#-*+>".includes(c))) return null;
  return title;
}

/** writ_core::notes::sanitize_title: a title made safe as a file stem, or null. */
export function sanitizeTitle(raw: string): string | null {
  const replaced = [...raw]
    .map((c) => (ILLEGAL_CHARS.has(c) || /\p{Cc}/u.test(c) ? " " : c))
    .join("");
  const trimmed = replaced
    .replace(/\s+/g, " ")
    .replace(/^[.\s]+/, "")
    .replace(/[.\s]+$/, "");
  const cut = [...trimmed].slice(0, MAX_STEM_CHARS).join("").replace(/[.\s]+$/, "");
  return cut || null;
}

/** The first of `stem`, `stem-2`, `stem-3`… that `taken` does not hold. */
export function dedupe(stem: string, taken: (candidate: string) => boolean): string {
  if (!taken(stem)) return stem;
  for (let n = 2; ; n += 1) {
    const candidate = `${stem}-${n}`;
    if (!taken(candidate)) return candidate;
  }
}

/** Subsequence match over the name, closer and earlier letters scoring higher. */
export function fuzzyScore(name: string, query: string): number | null {
  const hay = name.toLowerCase();
  const needle = query.toLowerCase().replace(/\s+/g, "");
  if (!needle) return null;
  let score = 0;
  let last = -1;
  for (const ch of needle) {
    const at = hay.indexOf(ch, last + 1);
    if (at === -1) return null;
    score += at === last + 1 ? 3 : 1;
    last = at;
  }
  if (hay.startsWith(needle)) score += 10;
  return score;
}
