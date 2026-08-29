import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { WorkspaceEntry } from "../../types/workspace";

const h = vi.hoisted(() => ({
  entries: new Map<string, WorkspaceEntry[]>(),
  root: "/notes" as string | null,
}));

vi.mock("../../stores/global/workspace", () => ({
  workspaceStore: {
    root: () => h.root,
    entriesFor: (path: string) => h.entries.get(path),
    loadDir: vi.fn(),
  },
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ tabs: { openFile: vi.fn(async () => undefined) } }),
}));

import FileTree from "../../components/Sidebar/FileTree";

function entry(name: string, isDir: boolean): WorkspaceEntry {
  return { name, path: `/notes/${name}`, is_dir: isDir };
}

afterEach(() => {
  h.entries = new Map();
  h.root = "/notes";
  cleanup();
});

describe("FileTree indent", () => {
  it("steps each level 16px past its parent", () => {
    h.entries.set("/notes", [entry("Drafts", true)]);
    h.entries.set("/notes/Drafts", [
      { name: "launch.md", path: "/notes/Drafts/launch.md", is_dir: false },
    ]);
    const { container } = render(() => <FileTree />);
    const folder = container.querySelector<HTMLElement>('[aria-level="1"]')!;
    expect(folder.style.paddingLeft).toBe("10px");

    fireEvent.click(folder);
    const child = container.querySelector<HTMLElement>('[aria-level="2"]')!;
    expect(child.style.paddingLeft).toBe("26px");
  });

  it("shows a folder open once expanded, and a note for a file", () => {
    h.entries.set("/notes", [entry("Drafts", true), entry("launch.md", false)]);
    const { container } = render(() => <FileTree />);
    const icons = () =>
      Array.from(container.querySelectorAll("use")).map((u) => u.getAttribute("href"));
    expect(icons()).toContain("#ph-folder");
    expect(icons()).toContain("#ph-file-text");
    expect(icons()).toContain("#ph-caret-right");

    fireEvent.click(container.querySelector<HTMLElement>('[aria-level="1"]')!);
    expect(icons()).toContain("#ph-folder-open");
    expect(icons()).toContain("#ph-caret-down");
  });
});
