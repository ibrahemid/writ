// The capture harness's notes folder, so the live window and the stills show
// the same files.
const FIXTURES = import.meta.glob<string>("../../scripts/capture/fixtures/home/Notes/**/*", {
  query: "?raw",
  import: "default",
  eager: true,
});

const PREFIX = "../../scripts/capture/fixtures/home/Notes/";

export const SEED_FILES: Record<string, string> = Object.fromEntries(
  Object.entries(FIXTURES).map(([path, content]) => [path.slice(PREFIX.length), content]),
);

export const HERO_NOTE = "Garden committee 10 Sep.md";

/** The tab open when the page loads: the hero capture's note. */
export const OPEN_AT_START = [HERO_NOTE];

/** The hero capture's cursor line, so the page at rest matches the still. */
export const CURSOR_LINE_AT_START = 14;

const VERSION_FIXTURES = import.meta.glob<string>("../../scripts/capture/fixtures/versions/*", {
  query: "?raw",
  import: "default",
  eager: true,
});

export const VERSIONED_NOTE = "Newsletter draft.md";

const HOUR_MS = 3_600_000;
const DAY_MS = 24 * HOUR_MS;
const NEWEST_VERSION_AGE_MS = 2 * HOUR_MS + 17 * 60_000;

export function computeSeededVersionAgeMs(index: number, count: number): number {
  const remaining = count - 1 - index;
  return remaining === 0 ? NEWEST_VERSION_AGE_MS : remaining * 2 * DAY_MS - 3 * HOUR_MS * index;
}

export interface SeededVersion {
  text: string;
  ageMs: number;
}

export const SEED_VERSIONS: readonly SeededVersion[] = Object.keys(VERSION_FIXTURES)
  .sort()
  .map((key, index, keys) => ({ text: VERSION_FIXTURES[key], ageMs: computeSeededVersionAgeMs(index, keys.length) }));
