import { describe, it, expect, vi, beforeEach } from "vitest";
import { EditorSelection, EditorState } from "@codemirror/state";

const h = vi.hoisted(() => ({
  answerOpenContext: vi.fn(),
  active: null as unknown,
  logFailure: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({ answerOpenContext: h.answerOpenContext }));
vi.mock("../../lib/log", () => ({ logFailure: h.logFailure }));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => h.active },
}));

import { answerContextAsk } from "../../stores/global/open-context";

function windowShowing(id: string | null, doc: string, from: number, to: number, dirty: boolean) {
  const state = EditorState.create({ doc, selection: EditorSelection.single(from, to) });
  return {
    editor: {
      currentBufferId: () => id,
      getView: () => ({ state }),
      isDirty: (asked: string) => dirty && asked === id,
    },
  };
}

describe("answering a program that asks which file is open", () => {
  beforeEach(() => {
    h.answerOpenContext.mockReset();
    h.logFailure.mockReset();
  });

  it("hands Rust the tab in front and its selection under the ask's token", async () => {
    h.active = windowShowing("tab-7", "# Plan\nlaunch the site\n", 7, 22, true);
    h.answerOpenContext.mockResolvedValue(undefined);

    await answerContextAsk("t-1");

    expect(h.answerOpenContext).toHaveBeenCalledWith("t-1", {
      buffer_id: "tab-7",
      selections: [
        { text: "launch the site", from: { line: 2, column: 1 }, to: { line: 2, column: 16 } },
      ],
      cursor: { line: 2, column: 16 },
      unsaved: true,
      truncated: false,
    });
  });

  it("answers that nothing is open when no window is", async () => {
    h.active = null;
    h.answerOpenContext.mockResolvedValue(undefined);

    await answerContextAsk("t-2");

    expect(h.answerOpenContext).toHaveBeenCalledWith("t-2", {
      buffer_id: null,
      selections: [],
      cursor: null,
      unsaved: false,
      truncated: false,
    });
  });

  it("logs an answer Rust would not take, without the selection", async () => {
    h.active = windowShowing("tab-7", "secret text", 0, 6, false);
    h.answerOpenContext.mockRejectedValue(new Error("no program is waiting for this answer"));

    await answerContextAsk("t-3");

    expect(h.logFailure).toHaveBeenCalledTimes(1);
    expect(String(h.logFailure.mock.calls[0][0])).not.toContain("secret");
  });
});
