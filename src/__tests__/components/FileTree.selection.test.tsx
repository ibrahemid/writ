import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { render, cleanup } from "@solidjs/testing-library";
import type { WorkspaceEntry } from "../../types/workspace";

const h = vi.hoisted(() => ({
  entries: new Map<string, WorkspaceEntry[]>(),
  root: "/notes" as string | null,
  activeTabId: null as string | null,
  tabs: [] as { id: string; source_path: string | null }[],
}));

vi.mock("../../stores/global/workspace", () => ({
  workspaceStore: {
    root: () => h.root,
    entriesFor: (path: string) => h.entries.get(path),
    loadDir: vi.fn(),
  },
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => h.tabs },
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    tabs: { openFile: vi.fn(async () => undefined), activeTabId: () => h.activeTabId },
    sidebar: { selectedTag: () => null },
  }),
}));

import FileTree from "../../components/Sidebar/FileTree";

function entry(name: string): WorkspaceEntry {
  return { name, path: `/notes/${name}`, is_dir: false, conflict_copy: null };
}

afterEach(() => {
  h.entries = new Map();
  h.root = "/notes";
  h.activeTabId = null;
  h.tabs = [];
  cleanup();
});

describe("the folder tree marks the note the editor is showing", () => {
  it("selects the row whose file the active tab holds, and no other", () => {
    h.entries.set("/notes", [entry("launch.md"), entry("notes.md")]);
    h.activeTabId = "b1";
    h.tabs = [{ id: "b1", source_path: "/notes/launch.md" }];

    const { container } = render(() => <FileTree />);
    const rows = Array.from(container.querySelectorAll<HTMLElement>(".file-tree-item"));

    expect(rows[0].getAttribute("aria-selected")).toBe("true");
    expect(rows[0].classList.contains("is-selected")).toBe(true);
    expect(rows[1].getAttribute("aria-selected")).toBe("false");
    expect(rows[1].classList.contains("is-selected")).toBe(false);
  });

  it("marks nothing when the open note has no file behind it", () => {
    h.entries.set("/notes", [entry("launch.md")]);
    h.activeTabId = "b1";
    h.tabs = [{ id: "b1", source_path: null }];

    const { container } = render(() => <FileTree />);
    const row = container.querySelector<HTMLElement>(".file-tree-item")!;

    expect(row.getAttribute("aria-selected")).toBe("false");
    expect(row.classList.contains("is-selected")).toBe(false);
  });

  it("paints the selected row with the neutral fill and tints its icon", () => {
    const css = readFileSync(resolve(process.cwd(), "src/components/Sidebar/FileTree.css"), "utf8");
    expect(css).toMatch(
      /\.file-tree-item\.is-selected\s*\{[^}]*background:\s*var\(--writ-bg-selected\)/,
    );
    expect(css).toMatch(/\.file-tree-item\.is-selected\s*\{[^}]*font-weight:\s*500/);
    expect(css).toMatch(
      /\.file-tree-item\.is-selected\s*>\s*\.writ-icon\s*\{[^}]*color:\s*var\(--writ-accent\)/,
    );
  });
});
