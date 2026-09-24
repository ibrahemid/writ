// The host's naming rules for files Writ mints, in writ_core::notes and
// writ_core::startup, for the page's in-memory folder.

const ILLEGAL_CHARS = new Set(["/", "\\", ":", "*", "?", '"', "<", ">", "|"]);
const MAX_STEM_CHARS = 200;

/** A clock field as chrono writes `%m`, `%d` or `%H`: two digits, zero padded. */
export function padTwo(n: number): string {
  return String(n).padStart(2, "0");
}

/** A local timestamp the way chrono writes `%Y-%m-%d %H.%M.%S`. */
export function formatDottedStamp(now: Date): string {
  return `${formatDateStem(now)} ${padTwo(now.getHours())}.${padTwo(now.getMinutes())}.${padTwo(now.getSeconds())}`;
}

/** notes::date_stem: the local date as `YYYY-MM-DD`. */
export function formatDateStem(now: Date): string {
  return `${now.getFullYear()}-${padTwo(now.getMonth() + 1)}-${padTwo(now.getDate())}`;
}

/** notes::note_file_stem: the title made a file stem, dated when it names nothing. */
export function deriveNoteFileStem(title: string, now: Date): string {
  const fallback = formatDateStem(now);
  if (!title.trim() || /^writ-[0-9]/.test(title.trim())) return fallback;
  return sanitizeTitle(title) ?? fallback;
}

/** writ_core::notes::minted_stem: `writ-<yymmdd>-<hhmm>` on the local clock. */
export function formatMintedStem(now: Date): string {
  return `writ-${padTwo(now.getFullYear() % 100)}${padTwo(now.getMonth() + 1)}${padTwo(now.getDate())}-${padTwo(now.getHours())}${padTwo(now.getMinutes())}`;
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
export function deriveFirstLineTitle(content: string): string | null {
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
export function dedupeStem(stem: string, taken: (candidate: string) => boolean): string {
  if (!taken(stem)) return stem;
  for (let n = 2; ; n += 1) {
    const candidate = `${stem}-${n}`;
    if (!taken(candidate)) return candidate;
  }
}

/** Subsequence match over the name, closer and earlier letters scoring higher. */
export function scoreFuzzyMatch(name: string, query: string): number | null {
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
