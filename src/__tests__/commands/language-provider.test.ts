import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BufferDocument } from "../../types/buffer";
import type { PaletteResult } from "../../components/Palette/types";

const h = vi.hoisted(() => ({
  activeTabs: [] as BufferDocument[],
  activeTabId: null as string | null,
  setLanguage: vi.fn(),
  showToast: vi.fn(),
  logFailure: vi.fn(),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => h.activeTabs,
    setLanguage: h.setLanguage,
  },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({ tabs: { activeTabId: () => h.activeTabId } }),
  },
}));

vi.mock("../../components/Notifications/Toast", () => ({ showToast: h.showToast }));
vi.mock("../../lib/log", () => ({ logFailure: h.logFailure }));

import { createLanguageProvider } from "../../commands/providers/language-provider";
import { languageChoices } from "../../editor/languages";

function doc(over: Partial<BufferDocument> & { id: string }): BufferDocument {
  return {
    title: "notes",
    filename: `${over.id}.txt`,
    status: "active",
    language: null,
    source_path: `/files/${over.id}.txt`,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    encoding: "UTF-8",
    ...over,
  };
}

const signal = () => new AbortController().signal;

function query(q: string): PaletteResult[] {
  return createLanguageProvider().query(q, signal(), "all") as PaletteResult[];
}

async function flush() {
  for (let i = 0; i < 5; i++) await Promise.resolve();
}

beforeEach(() => {
  vi.clearAllMocks();
  h.activeTabs = [doc({ id: "a", source_path: "/files/snippet.txt" })];
  h.activeTabId = "a";
  h.setLanguage.mockResolvedValue(undefined);
});

describe("language provider", () => {
  it("offers nothing with no file in front", () => {
    h.activeTabId = null;
    expect(query("")).toEqual([]);
  });

  it("lists detection, plain text and every language on an empty query", () => {
    const rows = query("");
    expect(rows[0].label).toBe("Detect automatically");
    expect(rows[1].label).toBe("Plain text");
    expect(rows).toHaveLength(languageChoices().length + 2);
    expect(rows.slice(2).map((row) => row.label)).toEqual(languageChoices().map((c) => c.name));
  });

  it("fits every row under the cap", () => {
    expect(query("").length).toBeLessThanOrEqual(createLanguageProvider().cap);
  });

  it("puts an exact name first, then names that start with the query", () => {
    const labels = query("python").map((row) => row.label);
    expect(labels[0]).toBe("Python");
    expect(query("yam").map((row) => row.label)[0]).toBe("YAML");
    // Rows that rank the same keep the list's name order.
    expect(query("ya").map((row) => row.label).slice(0, 2)).toEqual(["Yacas", "YAML"]);
  });

  it("finds a language by an alias", () => {
    const labels = query("bash").map((row) => row.label);
    expect(labels[0]).toBe("Shell");
  });

  it("finds plain text and detection by the words people type", () => {
    expect(query("plain")[0].label).toBe("Plain text");
    expect(query("auto")[0].label).toBe("Detect automatically");
  });

  it("leaves out rows that do not match", () => {
    expect(query("zzzznothing")).toEqual([]);
  });

  it("marks the choice the file holds as current", () => {
    h.activeTabs = [doc({ id: "a", language: "python" })];
    const current = query("").filter((row) => row.detail === "Current");
    expect(current.map((row) => row.label)).toEqual(["Python"]);
  });

  it("marks detection as current when nothing was chosen", () => {
    const current = query("").filter((row) => row.detail === "Current");
    expect(current.map((row) => row.label)).toEqual(["Detect automatically"]);
  });

  it("marks detection as current for a stored id the editor does not know", () => {
    h.activeTabs = [doc({ id: "a", language: "no-such-language" })];
    const current = query("").filter((row) => row.detail === "Current");
    expect(current.map((row) => row.label)).toEqual(["Detect automatically"]);
  });

  it("records the language a row names for the file in front", async () => {
    query("python")[0].execute();
    await flush();
    expect(h.setLanguage).toHaveBeenCalledWith("a", "python");
  });

  it("records plain text and clears the choice", async () => {
    query("plain")[0].execute();
    query("auto")[0].execute();
    await flush();
    expect(h.setLanguage).toHaveBeenNthCalledWith(1, "a", "plaintext");
    expect(h.setLanguage).toHaveBeenNthCalledWith(2, "a", null);
  });

  it("says so when the choice could not be kept", async () => {
    h.setLanguage.mockRejectedValue(new Error("invalid language: the language id is empty"));
    query("python")[0].execute();
    await flush();
    // The tab's own name for the file: a text format's extension is dropped.
    expect(h.showToast).toHaveBeenCalledWith("Couldn't change the language of snippet", "error");
    expect(h.logFailure).toHaveBeenCalledTimes(1);
  });
});
