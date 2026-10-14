import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { EditorView } from "@codemirror/view";
import { language as languageFacet } from "@codemirror/language";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";
import type { LayoutMode } from "../../../lib/preview-layout";

// A grammar loads the first time a file needs it. The text is on screen before
// it arrives, and the file is dressed in it once it does: a Markdown file in
// Inline mode gets its heading sizes without another keystroke.
//
// Its own file, so nothing else in the run has loaded the Markdown grammar.

const bufferContent = new Map<string, string>();

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => {}),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  },
}));

function markdownFile(id: string): BufferDocument {
  return {
    id,
    title: "plan.md",
    filename: "plan.md",
    status: "active",
    language: null,
    source_path: "/files/plan.md",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    encoding: "UTF-8",
  };
}

async function flushMicrotasks(n = 20) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

function HoldLayout(props: { bufferId: string; layout: LayoutMode }) {
  useWindow().layout.setLocal(props.bufferId, props.layout);
  return null;
}

afterEach(() => {
  cleanup();
});

// The first import of the Markdown grammar is the slow part of this file.
describe("EditorInstance: a grammar that arrives after the text", { timeout: 30_000 }, () => {
  it("shows the text at once and draws Inline Markdown when the grammar lands", async () => {
    bufferContent.set("G1", "# Heading\n\nBody text.\n");
    const EditorInstance = (await import("../EditorInstance")).default;
    const { container } = render(() => (
      <WindowProvider windowId={9801}>
        <HoldLayout bufferId="G1" layout={{ kind: "inline" }} />
        <EditorInstance buffer={markdownFile("G1")} />
      </WindowProvider>
    ));
    await flushMicrotasks();

    const dom = container.querySelector<HTMLElement>(".cm-editor");
    expect(dom, "the text waited for its grammar").not.toBeNull();
    const view = EditorView.findFromDOM(dom!)!;
    expect(view.state.doc.toString()).toBe("# Heading\n\nBody text.\n");

    await vi.waitFor(
      () => {
        expect(view.state.facet(languageFacet)?.name).toBe("markdown");
        expect(container.querySelector(".cm-line-md-h1")).not.toBeNull();
      },
      { timeout: 25_000, interval: 20 },
    );
  });
});
