import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { BufferDocument } from "../../types/buffer";

const h = vi.hoisted(() => ({
  history: [] as Partial<BufferDocument>[],
  restoreFromHistory: vi.fn(),
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ tabs: { restoreFromHistory: h.restoreFromHistory } }),
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { historyList: () => h.history },
}));

import StartSurface, { recentFiles } from "../../components/Editor/StartSurface";
import { registerCommand, unregisterCommand } from "../../commands/registry";
import { keybindingSegments } from "../../lib/keybinding-format";

const COMMANDS = [
  { id: "note.new", keybinding: "CmdOrCtrl+N" },
  { id: "file.open", keybinding: "CmdOrCtrl+O" },
  { id: "note.today", keybinding: undefined },
  { id: "notes.quickOpen", keybinding: "CmdOrCtrl+Shift+O" },
  { id: "palette.open", keybinding: "Shift+Shift" },
];
const ran: string[] = [];

function closed(id: string, path: string | null, closedAt: string): Partial<BufferDocument> {
  return { id, title: path?.split("/").pop() ?? id, source_path: path, closed_at: closedAt };
}

function rows(container: HTMLElement): HTMLButtonElement[] {
  return [...container.querySelectorAll<HTMLButtonElement>(".start-surface-row")];
}

beforeEach(() => {
  for (const cmd of COMMANDS) {
    registerCommand({
      id: cmd.id,
      label: cmd.id,
      keybinding: cmd.keybinding,
      scope: "app",
      execute: () => {
        ran.push(cmd.id);
      },
    });
  }
});

afterEach(() => {
  for (const cmd of COMMANDS) unregisterCommand(cmd.id);
  ran.length = 0;
  h.history = [];
  h.restoreFromHistory.mockClear();
  cleanup();
});

describe("the start surface", () => {
  it("offers the next steps, each a button with its chord", () => {
    const { container } = render(() => <StartSurface />);
    const actions = rows(container);
    expect(actions.map((row) => row.querySelector(".start-surface-label")!.textContent)).toEqual([
      "New file",
      "Open file…",
      "Today's file",
      "Open file by name",
      "Command palette",
    ]);
    expect(actions.every((row) => row.tagName === "BUTTON")).toBe(true);

    const keys = actions.map((row) =>
      [...row.querySelectorAll(".kbd-key")].map((key) => key.textContent),
    );
    expect(keys[0]).toEqual(keybindingSegments("CmdOrCtrl+N"));
    expect(keys[2]).toEqual([]);
    expect(keys[4]).toEqual(keybindingSegments("Shift+Shift"));
  });

  it("runs the command a row names", () => {
    const { container } = render(() => <StartSurface />);
    for (const row of rows(container)) row.click();
    expect(ran).toEqual(COMMANDS.map((cmd) => cmd.id));
  });

  it("lists the files closed most recently and reopens the one picked", () => {
    h.history = [
      closed("a", "/w/older.md", "2026-10-01T10:00:00Z"),
      closed("b", "/w/newest.md", "2026-10-03T10:00:00Z"),
      closed("c", null, "2026-10-04T10:00:00Z"),
      closed("d", "/w/middle.txt", "2026-10-02T10:00:00Z"),
    ];
    const { container } = render(() => <StartSurface />);
    const recent = rows(container).slice(5);
    expect(recent.map((row) => row.textContent)).toEqual(["newest", "middle", "older"]);

    recent[1].click();
    expect(h.restoreFromHistory).toHaveBeenCalledWith("d");
  });

  it("keeps the recent list short", () => {
    const many = Array.from({ length: 9 }, (_, i) =>
      closed(`n${i}`, `/w/n${i}.md`, `2026-10-0${i + 1}T00:00:00Z`),
    ) as BufferDocument[];
    expect(recentFiles(many).map((doc) => doc.id)).toEqual(["n8", "n7", "n6", "n5", "n4"]);
  });

  it("takes the focus when nothing holds it, and moves through rows with the arrows", () => {
    (document.activeElement as HTMLElement | null)?.blur();
    h.history = [closed("b", "/w/newest.md", "2026-10-03T10:00:00Z")];
    const { container } = render(() => <StartSurface />);
    const all = rows(container);
    expect(document.activeElement).toBe(all[0]);

    fireEvent.keyDown(all[0], { key: "ArrowUp" });
    expect(document.activeElement).toBe(all[all.length - 1]);
    fireEvent.keyDown(all[all.length - 1], { key: "ArrowDown" });
    expect(document.activeElement).toBe(all[0]);
  });
});
