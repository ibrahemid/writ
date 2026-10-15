import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { writThemeSpec } from "../../components/Editor/cm-theme";

// WKWebView reads only the prefixed property and the production build adds it,
// but a development build serves the source as written, so every rule here
// carries both forms.

const ROOT = process.cwd();

function sheet(rel: string): string {
  return readFileSync(resolve(ROOT, rel), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
}

// Every rule whose selector list names `selector`, merged in source order.
function declarations(css: string, selector: string): Map<string, string> {
  const found = new Map<string, string>();
  let matched = false;
  for (const [, list, body] of css.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    if (!list.split(",").some((s) => s.trim() === selector)) continue;
    matched = true;
    for (const line of body.split(";")) {
      const [property, ...rest] = line.split(":");
      if (rest.length === 0) continue;
      found.set(property.trim(), rest.join(":").trim());
    }
  }
  if (!matched) throw new Error(`no rule for ${selector}`);
  return found;
}

function expectSelect(css: string, selector: string, value: "none" | "text") {
  const rule = declarations(css, selector);
  expect(rule.get("-webkit-user-select"), selector).toBe(value);
  expect(rule.get("user-select"), selector).toBe(value);
}

const GLOBAL = sheet("src/styles/global.css");

describe("chrome never selects", () => {
  it("turns selection off at the root every window, overlay and toast renders under", () => {
    expectSelect(GLOBAL, "body", "none");
  });

  it("keeps fields selectable", () => {
    for (const selector of ["input", "textarea", '[contenteditable]:not([contenteditable="false"])']) {
      expectSelect(GLOBAL, selector, "text");
    }
  });

  it("keeps the note selectable, a read-only one included", () => {
    const content = writThemeSpec[".cm-content"] as Record<string, string>;
    expect(content.WebkitUserSelect).toBe("text");
    expect(content.userSelect).toBe("text");
  });

  it("keeps the chat transcript selectable", () => {
    const chat = sheet("src/components/Chat/ChatPane.css");
    for (const selector of [
      ".chat-turn-text",
      ".chat-reply",
      ".chat-error-text",
      ".chat-proposal-path",
      ".chat-proposal-summary",
      ".chat-diff",
    ]) {
      expectSelect(chat, selector, "text");
    }
  });

  it("keeps the other text people copy selectable", () => {
    const surfaces: [string, string][] = [
      ["src/components/SettingsModal/apps/ClientCards.css", ".client-card-text"],
      ["src/components/SettingsModal/SettingsModal.css", ".settings-version"],
      ["src/components/ConfirmDialog/ConfirmDialog.css", ".confirm-message"],
      ["src/components/ConfirmDialog/ConfirmDialog.css", ".confirm-diff-text"],
      ["src/components/NoteHistory/NoteHistoryPanel.css", ".note-versions-diff"],
      ["src/components/FailureScreen/FailureScreen.css", ".failure-screen-text"],
      ["src/components/FailureScreen/FailureScreen.css", ".failure-screen-details"],
      ["src/components/Editor/NoteReadFailure.css", ".note-read-failure-line"],
      ["src/components/Editor/EditorBar.css", ".editor-bar-text"],
      ["src/components/Notifications/Toast.css", ".toast-text"],
      ["src/components/AiRewrite/AiRewriteOverlay.css", ".ai-overlay-pane-body"],
      ["src/components/ContextPack/ContextPackModal.css", ".context-pack-preview"],
      ["src/components/ContextPack/ContextPackModal.css", ".context-pack-path"],
      ["src/components/NotesMigrationReport/NotesMigrationReport.css", ".notes-report-text"],
      ["src/components/PromptFill/PromptFillModal.css", ".placeholders-preview-block"],
    ];
    for (const [file, selector] of surfaces) expectSelect(sheet(file), selector, "text");
  });

  it("leaves the preview document's own selection on", () => {
    for (const file of ["src-tauri/assets/preview-base.css", "src-tauri/assets/generated/preview-tokens.css"]) {
      expect(/user-select\s*:\s*none/.test(sheet(file)), file).toBe(false);
    }
    expect(declarations(sheet("src-tauri/assets/preview-base.css"), "::selection").get("background")).toBeDefined();
  });

  it("selects a field and the note in one colour", () => {
    expect(declarations(GLOBAL, "::selection").get("background")).toBe("var(--writ-selection)");
  });
});
