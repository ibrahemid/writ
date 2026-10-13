import { describe, it, expect, vi, afterEach } from "vitest";
import { EditorState, Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { editorContextMenu, type EditorContextMenuDeps, type EditorMenuItem } from "../../editor/context-menu";

function lines(count: number, last = ""): string {
  const out: string[] = [];
  for (let i = 0; i < count; i++) out.push(`line ${i} {not a placeholder} and {{ spaced }}`);
  if (last) out.push(last);
  return out.join("\n");
}

let view: EditorView | null = null;

function menuFor(doc: string, looksForPlaceholders = true): EditorMenuItem[] {
  let shown: EditorMenuItem[] = [];
  const deps: EditorContextMenuDeps = {
    show: (_x, _y, items) => {
      shown = items;
    },
    spellingEntries: () => [],
    actions: {
      rewriteActions: [],
      runRewrite: vi.fn(),
      applySpelling: vi.fn(),
      addToDictionary: vi.fn(),
      openLink: vi.fn(),
      copyLink: vi.fn(),
      fillPlaceholders: vi.fn(),
      searchWorkspace: vi.fn(),
    },
    clipboard: { cut: vi.fn(), copy: vi.fn(), paste: vi.fn() },
    aiEnabled: () => false,
    editable: () => true,
    looksForPlaceholders: () => looksForPlaceholders,
  };
  view = new EditorView({
    state: EditorState.create({ doc, extensions: [editorContextMenu(deps)] }),
    parent: document.body,
  });
  view.contentDOM.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 5, clientY: 5 }));
  return shown;
}

const offersFill = (items: EditorMenuItem[]) => items.some((item) => item.label === "Fill placeholders…");

afterEach(() => {
  view?.destroy();
  view = null;
  vi.restoreAllMocks();
});

describe("the editor menu and {{placeholders}}", () => {
  it("offers Fill placeholders for one on the last line without reading the document whole", () => {
    const toString = vi.spyOn(Text.prototype, "toString");
    const items = menuFor(lines(3000, "ask {{topic}} here"));
    expect(offersFill(items)).toBe(true);
    expect(toString).not.toHaveBeenCalled();
  });

  it("does not offer it when no line holds one", () => {
    const toString = vi.spyOn(Text.prototype, "toString");
    const items = menuFor(lines(3000));
    expect(offersFill(items)).toBe(false);
    expect(toString).not.toHaveBeenCalled();
  });

  it("never reads a placeholder across a line break", () => {
    expect(offersFill(menuFor("{{to\npic}}"))).toBe(false);
  });

  it("does not look in a file the editor opened in a large tier", () => {
    expect(offersFill(menuFor("ask {{topic}} here", false))).toBe(false);
  });
});
