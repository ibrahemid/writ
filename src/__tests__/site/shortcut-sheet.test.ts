import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { EDITOR_COMMAND_KEYS } from "../../editor/editor-command-keys";

// The site's keycap sheet and shortcut table read `site/src/data/shortcuts.json`
// rather than the app's `EDITOR_COMMAND_KEYS`, so a chord that changes here does
// not change the public page before the build carrying it is downloadable.
//
// The frozen copy has to be refreshed for the release that ships the new chords.
// That is what the second test is: it starts asking once the site's release data
// names the version this tree builds.

function read<T>(path: string): T {
  return JSON.parse(readFileSync(resolve(process.cwd(), path), "utf8")) as T;
}

interface SiteShortcut {
  id: string;
  label: string;
  keybinding: string;
  aliases?: string[];
}

const frozen = read<SiteShortcut[]>("site/src/data/shortcuts.json");
const release = read<{ version: string; published: boolean }>("site/src/data/release.json");
const appVersion = read<{ version: string }>("src-tauri/tauri.conf.json").version;

/** The plain JSON shape of the app's table, for a value-by-value comparison. */
const live = JSON.parse(JSON.stringify(EDITOR_COMMAND_KEYS)) as SiteShortcut[];

const releaseShipsThisTree = release.published === true && release.version === appVersion;

describe("the site's frozen shortcut sheet", () => {
  it("names the same commands the app has, in the same order", () => {
    expect(frozen.map((entry) => entry.id)).toEqual(live.map((entry) => entry.id));
    expect(frozen.map((entry) => entry.label)).toEqual(live.map((entry) => entry.label));
  });

  it.skipIf(!releaseShipsThisTree)(
    "carries the chords of the release the site offers",
    () => {
      expect(
        frozen,
        `site/src/data/shortcuts.json still holds the chords of an earlier build. ` +
          `The site offers v${release.version}, which is what this tree builds, so ` +
          `refresh the file from EDITOR_COMMAND_KEYS.`,
      ).toEqual(live);
    },
  );
});
