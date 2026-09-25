import type { SaveState } from "../../src/stores/global/save-status";
import { CURSOR_LINE_AT_START, HERO_NOTE, VERSIONED_NOTE } from "../backend/seed";
import { ScenePaletteMissingError, SceneStateError, SceneTimeoutError } from "./errors";
import { checked, sleep, typeQuery, typeText, waitUntil, type Random } from "./runner";
import type { Scene, SceneApp, SceneName } from "./types";

export const TODO_NOTE = "To do.txt";
export const LOG_NOTE = "Server log.txt";
export const MARKDOWN_NOTE = "Sourdough notes.md";
export const TODO_TYPED = "\n- Take the recycling out on Tuesday";
export const MARKDOWN_TYPED = "\n## Saturday\n- 78% water\n- Cold proof\n- [ ] Buy rye\n";
export const SEARCH_QUERY = "compost";

const SAVE_WAIT_MS = 2000;
const PALETTE_WAIT_MS = 1000;
const VERSIONS_WAIT_MS = 2000;
const TODO_DONE_HEADING = "Done";

const isSaved = (state: SaveState) => state === "saved" || state === "clean";

export function findLastOpenItemLine(text: string): number {
  const lines = text.split("\n");
  const doneAt = lines.indexOf(TODO_DONE_HEADING);
  const openItems = doneAt === -1 ? lines : lines.slice(0, doneAt);
  for (let index = openItems.length - 1; index >= 0; index -= 1) {
    if (openItems[index].startsWith("- ")) return index + 1;
  }
  throw new SceneStateError("any-file", `${TODO_NOTE} lists no open item`);
}

export function findFirstLineContaining(text: string, term: string): number {
  const index = text.split("\n").findIndex((line) => line.includes(term));
  if (index === -1) throw new SceneStateError("search", `no line holds "${term}"`);
  return index + 1;
}

async function restoreSeedText(app: SceneApp, note: string, signal: AbortSignal) {
  const editor = await checked(app.openNote(note, signal), signal);
  const seed = app.seedText(note);
  if (editor.text() !== seed) editor.replaceText(seed);
  return { editor, seed };
}

export const settle: Scene = async (app, signal) => {
  app.clearTyping();
  app.settings.close();
  app.palette.close();
  app.versions.close();
  app.graph.close();
  app.restoreLayouts();
  await checked(app.apps.setOn("graph", false), signal);
};

export function createScenes(random: Random = Math.random): Readonly<Record<SceneName, Scene>> {
  return {
    hero: async (app, signal) => {
      const editor = await checked(app.openNote(HERO_NOTE, signal), signal);
      await checked(app.closeOtherTabs(editor), signal);
      editor.showFromTop();
      editor.placeCursorAtLineStart(CURSOR_LINE_AT_START);
    },

    "any-file": async (app, signal) => {
      const { editor, seed } = await restoreSeedText(app, TODO_NOTE, signal);
      editor.placeCursorAtLineEnd(findLastOpenItemLine(seed));
      await typeText(editor, TODO_TYPED, signal, random);
      await waitUntil(() => isSaved(app.saveState(editor)), SAVE_WAIT_MS, signal);
      await checked(app.openNote(LOG_NOTE, signal), signal);
    },

    markdown: async (app, signal) => {
      const { editor } = await restoreSeedText(app, MARKDOWN_NOTE, signal);
      editor.placeCursorAtEnd();
      await typeText(editor, MARKDOWN_TYPED, signal, random);
      await sleep(600, signal);
      app.setLayout(editor, "source");
      await sleep(1500, signal);
      app.setLayout(editor, "inline");
    },

    search: async (app, signal) => {
      app.palette.open();
      if (!(await waitUntil(() => app.palette.isReady(), PALETTE_WAIT_MS, signal))) throw new ScenePaletteMissingError();
      await sleep(400, signal);
      await typeQuery(app.palette, SEARCH_QUERY, signal, random);
      await sleep(1200, signal);
      app.palette.close();
      const line = findFirstLineContaining(app.seedText(HERO_NOTE), SEARCH_QUERY);
      await checked(app.openNoteAtLine(HERO_NOTE, line, signal), signal);
    },

    apps: async (app, signal) => {
      app.settings.open("apps");
      await sleep(800, signal);
      await checked(app.apps.setOn("graph", true), signal);
      await sleep(1000, signal);
      app.settings.close();
      app.graph.open();
    },

    versions: async (app, signal) => {
      await checked(app.versions.reset(VERSIONED_NOTE), signal);
      const editor = await checked(app.openNote(VERSIONED_NOTE, signal), signal);
      app.versions.open(editor.path);
      if (!(await waitUntil(() => app.versions.isLoaded(editor.path), VERSIONS_WAIT_MS, signal))) {
        throw new SceneTimeoutError(`the versions of ${VERSIONED_NOTE}`, VERSIONS_WAIT_MS);
      }
      await sleep(800, signal);
      const second = app.versions.list()[1];
      if (!second) throw new SceneStateError("versions", `${VERSIONED_NOTE} has fewer than two versions`);
      await checked(app.versions.select(second.id), signal);
      await sleep(1000, signal);
      await checked(app.versions.restore(second.id), signal);
      await sleep(600, signal);
      app.versions.close();
    },
  };
}
