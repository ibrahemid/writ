import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { ensureSyntaxTree } from "@codemirror/language";
import { markdownInlineExtension } from "../../editor/markdown-typography";
import { MAX_NORMAL_LINE_LENGTH } from "../../editor/large-file";

// One pasted data URI is one line far over the per-line limit. Before, it put
// the whole note in a mode with no Markdown rendering (EP-09); now only that
// line is drawn plain.

describe("Markdown Inline with one very long line", () => {
  it("keeps rendering the headings and emphasis around the long line", () => {
    const blob = `![photo](data:image/png;base64,${"A".repeat(MAX_NORMAL_LINE_LENGTH * 2)})`;
    const doc = `# Trip notes\n\n${blob}\n\nSome **bold** words.\n`;
    const view = new EditorView({
      state: EditorState.create({
        doc,
        extensions: [
          markdown({ base: markdownLanguage }),
          markdownInlineExtension({
            resolve: () => new Promise(() => {}),
            download: () => new Promise(() => {}),
          }),
        ],
      }),
    });
    ensureSyntaxTree(view.state, view.state.doc.length, 30_000);
    view.dispatch({ selection: { anchor: doc.length } });

    expect(view.contentDOM.querySelector(".cm-line-md-h1")).not.toBeNull();
    expect(view.contentDOM.querySelector(".cm-md-strong, .cm-md-bold")).not.toBeNull();
    view.destroy();
  });
});
