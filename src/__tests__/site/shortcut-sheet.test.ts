import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { EDITOR_COMMAND_KEYS } from "../../editor/editor-command-keys";

// The site's keycap sheet and shortcut table read `site/src/data/shortcuts.json`
// rather than the app's `EDITOR_COMMAND_KEYS`, so a chord that changes here does
// not change the public page before the build carrying it is downloadable.
//
// The frozen copy has to be refreshed for the release that ships the new chords,
// and nothing in the repo moves at the moment a release is published: the deploy
// regenerates `release.json` from the GitHub API and the committed copy is left
// as it was. So the second test asks on either of two signals — the site's
// release data naming the version this tree builds, which needs someone to have
// committed it, or the version having moved past the one the sheet was frozen
// for, which `scripts/bump_version.py` does on its own at the next release.

function read<T>(path: string): T {
  return JSON.parse(readFileSync(resolve(process.cwd(), path), "utf8")) as T;
}

interface SiteShortcut {
  id: string;
  label: string;
  keybinding: string;
  aliases?: string[];
}

const sheet = read<{ frozenThrough: string; keys: SiteShortcut[] }>(
  "site/src/data/shortcuts.json",
);
const frozen = sheet.keys;
const release = read<{ version: string; published: boolean }>("site/src/data/release.json");
const appVersion = read<{ version: string }>("src-tauri/tauri.conf.json").version;

/** The plain JSON shape of the app's table, for a value-by-value comparison. */
const live = JSON.parse(JSON.stringify(EDITOR_COMMAND_KEYS)) as SiteShortcut[];

const releaseNamesThisTree = release.published === true && release.version === appVersion;
const sheetIsOlderThanTheApp = sheet.frozenThrough !== appVersion;
const theSiteOffersTheseChords = releaseNamesThisTree || sheetIsOlderThanTheApp;

describe("the site's frozen shortcut sheet", () => {
  it("names the same commands the app has, in the same order", () => {
    expect(frozen.map((entry) => entry.id)).toEqual(live.map((entry) => entry.id));
    expect(frozen.map((entry) => entry.label)).toEqual(live.map((entry) => entry.label));
  });

  it.skipIf(!theSiteOffersTheseChords)(
    "carries the chords of the release the site offers",
    () => {
      expect(
        frozen,
        `site/src/data/shortcuts.json holds the chords of v${sheet.frozenThrough} and ` +
          `this tree builds v${appVersion}. Refresh the file from EDITOR_COMMAND_KEYS ` +
          `and set frozenThrough to ${appVersion}.`,
      ).toEqual(live);
    },
  );
});
