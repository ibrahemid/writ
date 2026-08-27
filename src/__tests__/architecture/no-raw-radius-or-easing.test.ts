import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

// Radii come from --writ-r-*, timing from var(--writ-motion). The allowlist is
// what the retokenisation has not reached yet: each unit removes its own files,
// and the last one leaves the list empty.

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");
const SKIP_DIRS = [
  resolve(SRC, "__tests__"),
  resolve(SRC, "styles/generated"),
  resolve(SRC, "styles/themes"),
];

export const RETOKENISE_ALLOWLIST: readonly string[] = [
  "src/components/ConfirmDialog/ConfirmDialog.css",
  "src/components/ContextMenu/ContextMenu.css",
  "src/components/Editor/StatusBar.css",
  "src/components/Editor/SpellingPreview.css",
  "src/components/Editor/TabBar.css",
  "src/components/Editor/cm-markdown-typography.css",
  "src/components/ErrorBoundary/ErrorBoundary.css",
  "src/components/Find/FindOverlay.css",
  "src/components/Notifications/Toast.css",
  "src/components/Palette/Palette.css",
  "src/components/Preview/LinkConfirm.css",
  "src/components/Preview/preview-chrome.css",
  "src/components/Preview/preview-layout-toggle.css",
  "src/components/PromptFill/PromptFillModal.css",
  "src/components/SettingsModal/SettingsModal.css",
  "src/components/ShortcutEditor/ShortcutEditor.css",
  "src/components/Sidebar/ActiveSection.css",
  "src/components/Sidebar/FileTree.css",
  "src/components/Sidebar/InboxSection.css",
  "src/components/Sidebar/SearchBar.css",
  "src/components/Sidebar/SearchResults.css",
  "src/components/Sidebar/Sidebar.css",
  "src/components/Sidebar/TabItem.css",
  "src/components/ThemeEditor/ThemeEditor.css",
  "src/components/TitleBar/TitleBar.css",
  "src/components/UpdateBanner/UpdateBanner.css",
  "src/styles/global.css",
];

const RAW_RADIUS = /border-radius\s*:[^;}]*\b\d+(?:\.\d+)?px/g;
const BARE_EASING = /(?:^|[\s,:(])(ease|ease-in|ease-out|ease-in-out|linear)(?=[\s,;)]|$)/gm;

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "node_modules" || SKIP_DIRS.includes(full)) continue;
      walk(full, files);
    } else if (/\.(css|ts|tsx)$/.test(entry) && !entry.endsWith(".d.ts")) {
      files.push(full);
    }
  }
  return files;
}

function withoutComments(text: string): string {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("\n")
    .map((line) => line.replace(/\/\/.*$/, ""))
    .join("\n");
}

function offendersFor(pattern: RegExp): string[] {
  const offenders: string[] = [];
  for (const file of walk(SRC)) {
    const rel = relative(REPO_ROOT, file);
    if (RETOKENISE_ALLOWLIST.includes(rel)) continue;
    const matches = withoutComments(readFileSync(file, "utf8")).match(pattern);
    if (matches) offenders.push(`${rel} -> ${matches.map((m) => m.trim()).join(", ")}`);
  }
  return offenders;
}

describe("no raw radius or easing outside the allowlist", () => {
  it("border-radius resolves through a --writ-r-* token", () => {
    const offenders = offendersFor(RAW_RADIUS);
    expect(offenders, `raw radii found:\n${offenders.join("\n")}`).toEqual([]);
  });

  it("transitions resolve through var(--writ-motion)", () => {
    const offenders = offendersFor(BARE_EASING);
    expect(offenders, `bare timing functions found:\n${offenders.join("\n")}`).toEqual([]);
  });

  it("allowlist entries all still exist", () => {
    for (const rel of RETOKENISE_ALLOWLIST) {
      expect(() => statSync(resolve(REPO_ROOT, rel)), `${rel} is gone`).not.toThrow();
    }
  });
});
