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

/** The tab open when the page loads: the hero capture's note. */
export const OPEN_AT_START = ["Garden committee 10 Sep.md"];

/** The hero capture's cursor line, so the page at rest matches the still. */
export const CURSOR_LINE_AT_START = 14;
