import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteVersion } from "../../src/services/tauri";
import type { SaveState } from "../../src/stores/global/save-status";
import { CURSOR_LINE_AT_START, HERO_NOTE, SEED_FILES, VERSIONED_NOTE } from "../backend/seed";
import { ENGAGED_MESSAGE, READY_MESSAGE, SCENE_MESSAGE, createSceneChannel } from "../scenes/channel";
import { SceneStateError } from "../scenes/errors";
import {
  KEY_DELAY_MAX_MS,
  KEY_DELAY_MIN_MS,
  LINE_END_DELAY_MS,
  TYPING_COOLDOWN_MS,
  createSceneRunner,
  type Random,
} from "../scenes/runner";
import {
  LOG_NOTE,
  MARKDOWN_NOTE,
  MARKDOWN_TYPED,
  SEARCH_QUERY,
  TODO_NOTE,
  TODO_TYPED,
  createScenes,
  findFirstLineContaining,
  findLastOpenItemLine,
  settle,
} from "../scenes/scenes";
import { SCENE_NAMES, type SceneApp, type SceneEditor, type SceneLayout, type SceneName, type ScenePlayer, type SceneState } from "../scenes/types";

const SCENE_BUDGET_MS = 7000;
const HIGHEST: Random = () => 0.9999;

interface Insert {
  note: string;
  char: string;
  at: number;
}

function createFakeApp(options: { saveState?: SaveState } = {}) {
  const texts = new Map<string, string>(Object.entries(SEED_FILES));
  const cursors = new Map<string, number>();
  const cursorLines = new Map<string, number>();
  const shownFromTop: string[] = [];
  const calls: string[] = [];
  const clearedAt: number[] = [];
  let tabs: string[] = [HERO_NOTE];
  const layouts = new Map<string, SceneLayout>();
  const inserts: Insert[] = [];
  const queries: string[] = [];
  const opened: string[] = [];
  const reveals: { note: string; line: number }[] = [];
  const restored: number[] = [];
  const selected: number[] = [];
  const versionList: NoteVersion[] = [
    { id: 3, at_ms: 3, bytes: 3 },
    { id: 2, at_ms: 2, bytes: 2 },
    { id: 1, at_ms: 1, bytes: 1 },
  ];
  const open = { settings: false, palette: false, versions: false, graph: false, graphApp: false, typing: false };

  const lineOffsets = (text: string) => {
    const starts = [0];
    for (let i = 0; i < text.length; i += 1) if (text[i] === "\n") starts.push(i + 1);
    return starts;
  };

  function editorFor(note: string): SceneEditor {
    const read = () => texts.get(note) ?? "";
    const lineEnd = (line: number) => {
      const text = read();
      const starts = lineOffsets(text);
      const next = starts[line];
      return next === undefined ? text.length : next - 1;
    };
    return {
      bufferId: `buffer:${note}`,
      path: `/Users/you/Notes/${note}`,
      text: read,
      replaceText: (text) => texts.set(note, text),
      showFromTop: () => shownFromTop.push(note),
      placeCursorAtLineStart: (line) => {
        cursorLines.set(note, line);
        cursors.set(note, lineOffsets(read())[line - 1]);
      },
      placeCursorAtLineEnd: (line) => cursors.set(note, lineEnd(line)),
      placeCursorAtEnd: () => cursors.set(note, read().length),
      insert(char) {
        const text = read();
        const at = cursors.get(note) ?? text.length;
        texts.set(note, text.slice(0, at) + char + text.slice(at));
        cursors.set(note, at + char.length);
        inserts.push({ note, char, at: Date.now() });
      },
      setTyping: (isTyping) => {
        open.typing = isTyping;
      },
    };
  }

  const app: SceneApp = {
    seedText: (note) => SEED_FILES[note],
    openNote: async (note) => {
      opened.push(note);
      calls.push(`openNote:${note}`);
      if (!tabs.includes(note)) tabs = [...tabs, note];
      return editorFor(note);
    },
    closeOtherTabs: async (editor) => {
      tabs = tabs.filter((note) => `buffer:${note}` === editor.bufferId);
    },
    openNoteAtLine: async (note, line) => {
      opened.push(note);
      reveals.push({ note, line });
      return editorFor(note);
    },
    setLayout: (editor, layout) => layouts.set(editor.bufferId, layout),
    restoreLayouts: () => {
      for (const id of layouts.keys()) layouts.set(id, "inline");
    },
    clearTyping: () => {
      clearedAt.push(Date.now());
      open.typing = false;
    },
    saveState: () => options.saveState ?? "saved",
    settings: {
      open: () => {
        open.settings = true;
      },
      close: () => {
        open.settings = false;
      },
    },
    apps: {
      setOn: async (id, isOn) => {
        if (id === "graph") open.graphApp = isOn;
      },
    },
    graph: {
      open: () => {
        open.graph = true;
      },
      close: () => {
        open.graph = false;
      },
    },
    palette: {
      open: () => {
        open.palette = true;
      },
      close: () => {
        open.palette = false;
      },
      isReady: () => open.palette,
      setQuery: (query) => queries.push(query),
    },
    versions: {
      open: () => {
        calls.push("versions.open");
        open.versions = true;
      },
      reset: async (note) => {
        calls.push(`versions.reset:${note}`);
      },
      close: () => {
        open.versions = false;
      },
      isLoaded: () => open.versions,
      list: () => versionList,
      select: async (id) => {
        selected.push(id);
      },
      restore: async (id) => {
        restored.push(id);
      },
    },
  };

  return { app, texts, layouts, inserts, queries, opened, reveals, restored, selected, open, cursorLines, shownFromTop, calls, clearedAt, tabs: () => tabs };
}

function createHarness(options: { random?: Random; saveState?: SaveState } = {}) {
  const fake = createFakeApp({ saveState: options.saveState });
  const reports: [SceneName, SceneState][] = [];
  const failures: [SceneName, unknown][] = [];
  const player = createSceneRunner({
    app: fake.app,
    scenes: createScenes(options.random ?? HIGHEST),
    settle,
    report: (name, state) => reports.push([name, state]),
    reportFailure: (name, error) => failures.push([name, error]),
  });
  return { ...fake, player, reports, failures };
}

async function playToEnd(player: ScenePlayer, name: SceneName): Promise<number> {
  const start = Date.now();
  const finished = player.play(name);
  await vi.runAllTimersAsync();
  await finished;
  return Date.now() - start;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("the scenes", () => {
  it("has a scene for every name the page can post", () => {
    expect(Object.keys(createScenes()).sort()).toEqual([...SCENE_NAMES].sort());
  });

  it("finds the fixture lines the scenes aim at from the seed text", () => {
    expect(findLastOpenItemLine(SEED_FILES[TODO_NOTE])).toBe(9);
    expect(findFirstLineContaining(SEED_FILES[HERO_NOTE], SEARCH_QUERY)).toBe(13);
    expect(() => findLastOpenItemLine("Done\n- paid")).toThrow(SceneStateError);
    expect(() => findFirstLineContaining("nothing here", SEARCH_QUERY)).toThrow(SceneStateError);
  });

  it.each(SCENE_NAMES)("plays %s inside the budget at the slowest cadence", async (name) => {
    const { player, reports } = createHarness({ random: HIGHEST, saveState: "dirty" });
    const elapsed = await playToEnd(player, name);
    expect(reports).toEqual([[name, "done"]]);
    expect(elapsed).toBeLessThan(SCENE_BUDGET_MS);
  });

  it("types at 35 to 70 ms a key and 250 ms before a line break", async () => {
    let step = 0;
    const wandering: Random = () => [0, 0.5, 0.9999, 0.25][step++ % 4];
    const { player, inserts } = createHarness({ random: wandering });
    await playToEnd(player, "markdown");
    expect(inserts.map((insert) => insert.char).join("")).toBe(MARKDOWN_TYPED);
    const gaps = inserts.slice(1).map((insert, index) => ({ char: insert.char, gap: insert.at - inserts[index].at }));
    for (const { char, gap } of gaps) {
      if (char === "\n") expect(gap).toBe(LINE_END_DELAY_MS);
      else {
        expect(gap).toBeGreaterThanOrEqual(KEY_DELAY_MIN_MS);
        expect(gap).toBeLessThanOrEqual(KEY_DELAY_MAX_MS);
      }
    }
  });

  it("shows the caret only while it types", async () => {
    const { player, open } = createHarness();
    const finished = player.play("markdown");
    await vi.advanceTimersByTimeAsync(300);
    expect(open.typing).toBe(true);
    await vi.runAllTimersAsync();
    await finished;
    expect(open.typing).toBe(false);
  });

  it("puts the resting note back on its cursor line for hero", async () => {
    const { player, opened, reports, cursorLines, shownFromTop } = createHarness();
    await playToEnd(player, "hero");
    expect(opened).toEqual([HERO_NOTE]);
    expect(shownFromTop).toEqual([HERO_NOTE]);
    expect(reports).toEqual([["hero", "done"]]);
    expect(cursorLines.get(HERO_NOTE)).toBe(CURSOR_LINE_AT_START);
  });

  it("closes every tab but the resting note when hero follows another scene", async () => {
    const { player, tabs } = createHarness();
    await playToEnd(player, "any-file");
    expect(tabs()).toEqual([HERO_NOTE, TODO_NOTE, LOG_NOTE]);
    await playToEnd(player, "hero");
    expect(tabs()).toEqual([HERO_NOTE]);
  });

  it("adds one line to the to-do list on every replay and rests on the log", async () => {
    const { player, texts, opened } = createHarness();
    const seedLines = SEED_FILES[TODO_NOTE].split("\n");
    await playToEnd(player, "any-file");
    const once = texts.get(TODO_NOTE)!.split("\n");
    expect(once).toHaveLength(seedLines.length + 1);
    expect(once[9]).toBe(TODO_TYPED.trim());
    await playToEnd(player, "any-file");
    expect(texts.get(TODO_NOTE)!.split("\n")).toEqual(once);
    expect(opened[opened.length - 1]).toBe(LOG_NOTE);
  });

  it("types the markdown at the end of the seed text, shows source, and rests inline", async () => {
    const { player, texts, layouts } = createHarness();
    texts.set(MARKDOWN_NOTE, "changed by an earlier run");
    await playToEnd(player, "markdown");
    expect(texts.get(MARKDOWN_NOTE)).toBe(SEED_FILES[MARKDOWN_NOTE] + MARKDOWN_TYPED);
    expect(layouts.get(`buffer:${MARKDOWN_NOTE}`)).toBe("inline");
  });

  it("types the query a letter at a time and opens the first hit on its line", async () => {
    const { player, queries, reveals, open } = createHarness();
    await playToEnd(player, "search");
    expect(queries).toEqual([..."compost"].map((_, index) => SEARCH_QUERY.slice(0, index + 1)));
    expect(open.palette).toBe(false);
    expect(reveals).toEqual([{ note: HERO_NOTE, line: 13 }]);
  });

  it("switches the graph on inside settings and rests on the drawing", async () => {
    const { player, open } = createHarness();
    await playToEnd(player, "apps");
    expect(open).toMatchObject({ settings: false, graphApp: true, graph: true });
  });

  it("selects and restores the second version, then closes the list", async () => {
    const { player, selected, restored, opened, open, calls } = createHarness();
    await playToEnd(player, "versions");
    expect(opened).toEqual([VERSIONED_NOTE]);
    expect(calls).toEqual([`versions.reset:${VERSIONED_NOTE}`, `openNote:${VERSIONED_NOTE}`, "versions.open"]);
    expect(selected).toEqual([2]);
    expect(restored).toEqual([2]);
    expect(open.versions).toBe(false);
  });

  it("settles to a window with nothing open and the graph switched off", async () => {
    const { app, open, layouts } = createFakeApp();
    Object.assign(open, { settings: true, palette: true, versions: true, graph: true, graphApp: true, typing: true });
    layouts.set("buffer:a.md", "source");
    const finished = settle(app, new AbortController().signal);
    await vi.runAllTimersAsync();
    await finished;
    expect(open).toEqual({ settings: false, palette: false, versions: false, graph: false, graphApp: false, typing: false });
    expect(layouts.get("buffer:a.md")).toBe("inline");
  });

  it("cancels a running scene when the next one arrives, and the first goes quiet", async () => {
    const { player, reports, inserts } = createHarness();
    const first = player.play("markdown");
    await vi.advanceTimersByTimeAsync(500);
    const typedBeforeCancel = inserts.length;
    expect(typedBeforeCancel).toBeGreaterThan(0);
    const second = player.play("apps");
    await vi.runAllTimersAsync();
    await Promise.all([first, second]);
    expect(reports).toEqual([
      ["markdown", "cancelled"],
      ["apps", "done"],
    ]);
    expect(inserts).toHaveLength(typedBeforeCancel);
  });

  it("plays only the newest of several scenes posted in a burst", async () => {
    const { player, reports } = createHarness();
    const runs = [player.play("markdown"), player.play("search"), player.play("versions")];
    await vi.runAllTimersAsync();
    await Promise.all(runs);
    expect(reports).toEqual([
      ["markdown", "cancelled"],
      ["search", "cancelled"],
      ["versions", "done"],
    ]);
  });

  it("reports a scene that fails as cancelled and names the failure", async () => {
    const { player, reports, failures, app } = createHarness();
    app.versions.list = () => [];
    await playToEnd(player, "versions");
    expect(reports).toEqual([["versions", "cancelled"]]);
    expect(failures[0][1]).toBeInstanceOf(SceneStateError);
  });
});

describe("the wait after a scene cancelled while typing", () => {
  const typedChars = [...MARKDOWN_TYPED].length;

  async function advanceUntilTyped(inserts: Insert[]): Promise<void> {
    while (inserts.length < typedChars) await vi.advanceTimersByTimeAsync(5);
  }

  it.each(["any-file", "hero"] as const)("holds %s's settle until the cooldown after a cancel mid-typing", async (next) => {
    const { player, inserts, clearedAt, reports } = createHarness();
    const first = player.play("markdown");
    await vi.advanceTimersByTimeAsync(500);
    expect(inserts.length).toBeGreaterThan(0);
    expect(inserts.length).toBeLessThan(typedChars);
    const cancelAt = Date.now();
    const second = player.play(next);
    await vi.advanceTimersByTimeAsync(TYPING_COOLDOWN_MS - 1);
    expect(clearedAt).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(clearedAt).toHaveLength(2);
    expect(clearedAt[1]).toBe(cancelAt + TYPING_COOLDOWN_MS);
    await vi.runAllTimersAsync();
    await Promise.all([first, second]);
    expect(reports).toEqual([
      ["markdown", "cancelled"],
      [next, "done"],
    ]);
  });

  it("holds the cooldown when the typing stopped just before the cancel", async () => {
    const { player, inserts, clearedAt } = createHarness();
    const first = player.play("markdown");
    await advanceUntilTyped(inserts);
    await vi.advanceTimersByTimeAsync(20);
    const cancelAt = Date.now();
    const second = player.play("search");
    await vi.runAllTimersAsync();
    await Promise.all([first, second]);
    expect(clearedAt[1]).toBe(cancelAt + TYPING_COOLDOWN_MS);
  });

  it("keeps the first deadline when another scene arrives during the cooldown", async () => {
    const { player, clearedAt, reports } = createHarness();
    const runs = [player.play("markdown")];
    await vi.advanceTimersByTimeAsync(500);
    const cancelAt = Date.now();
    runs.push(player.play("search"));
    await vi.advanceTimersByTimeAsync(100);
    runs.push(player.play("versions"));
    await vi.runAllTimersAsync();
    await Promise.all(runs);
    expect(clearedAt).toEqual([expect.any(Number), cancelAt + TYPING_COOLDOWN_MS]);
    expect(reports).toEqual([
      ["markdown", "cancelled"],
      ["search", "cancelled"],
      ["versions", "done"],
    ]);
  });

  it("starts the next scene at once after a clean finish", async () => {
    const { player, clearedAt } = createHarness();
    await playToEnd(player, "markdown");
    const playAt = Date.now();
    await playToEnd(player, "any-file");
    expect(clearedAt[1]).toBe(playAt);
  });

  it("starts the next scene at once when the cancelled scene typed longer ago than the cooldown", async () => {
    const { player, inserts, clearedAt, reports } = createHarness();
    const first = player.play("markdown");
    await advanceUntilTyped(inserts);
    await vi.advanceTimersByTimeAsync(700);
    const cancelAt = Date.now();
    const second = player.play("any-file");
    await vi.runAllTimersAsync();
    await Promise.all([first, second]);
    expect(clearedAt[1]).toBe(cancelAt);
    expect(reports).toEqual([
      ["markdown", "cancelled"],
      ["any-file", "done"],
    ]);
  });
});

describe("the scene channel", () => {
  const ORIGIN = "https://writ.example";

  function createChannel() {
    const posted: unknown[] = [];
    const parent = { postMessage: (message: unknown) => posted.push(message) };
    const played: SceneName[] = [];
    let cancels = 0;
    const player: ScenePlayer = {
      play: async (name) => {
        played.push(name);
      },
      cancel: () => {
        cancels += 1;
      },
    };
    const channel = createSceneChannel({ parent, origin: ORIGIN });
    const post = (data: unknown, overrides: { origin?: string; source?: unknown } = {}) =>
      channel.receive({ data, origin: overrides.origin ?? ORIGIN, source: "source" in overrides ? overrides.source : parent });
    return { channel, posted, played, player, post, cancels: () => cancels };
  }

  it("drops messages from another origin, another window, of another type, or naming no scene", () => {
    const { channel, played, player, post } = createChannel();
    channel.ready(player);
    post({ type: SCENE_MESSAGE, name: "search" }, { origin: "https://elsewhere.example" });
    post({ type: SCENE_MESSAGE, name: "search" }, { source: {} });
    post({ type: SCENE_MESSAGE, name: "search" }, { source: null });
    post({ type: "writ-demo-other", name: "search" });
    post({ type: SCENE_MESSAGE, name: "credits" });
    post({ type: SCENE_MESSAGE, name: "search", state: "done" });
    post("search");
    post(null);
    expect(played).toEqual([]);
    post({ type: SCENE_MESSAGE, name: "search" });
    expect(played).toEqual(["search"]);
  });

  it("holds only the newest scene posted before ready and plays it once ready", () => {
    const { channel, played, player, post, posted } = createChannel();
    post({ type: SCENE_MESSAGE, name: "markdown" });
    post({ type: SCENE_MESSAGE, name: "apps" });
    expect(played).toEqual([]);
    channel.ready(player);
    expect(posted).toEqual([{ type: READY_MESSAGE }]);
    expect(played).toEqual(["apps"]);
  });

  it("tells the page once when the visitor takes over, cancels the scene, and ignores the rest", () => {
    const { channel, played, player, post, posted, cancels } = createChannel();
    channel.ready(player);
    channel.engage();
    channel.engage();
    expect(posted.filter((message) => (message as { type: string }).type === ENGAGED_MESSAGE)).toHaveLength(1);
    expect(cancels()).toBe(1);
    post({ type: SCENE_MESSAGE, name: "search" });
    expect(played).toEqual([]);
  });

  it("forgets a scene held before ready once the visitor takes over", () => {
    const { channel, played, player, post } = createChannel();
    post({ type: SCENE_MESSAGE, name: "versions" });
    channel.engage();
    channel.ready(player);
    expect(played).toEqual([]);
  });

  it("acks each scene with its name and state", () => {
    const { channel, posted } = createChannel();
    channel.report("apps", "done");
    expect(posted).toEqual([{ type: SCENE_MESSAGE, name: "apps", state: "done" }]);
  });
});
