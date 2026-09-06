import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { resolve, join } from "node:path";

// Surfaces that have moved onto the primitives. An inline <svg> is a second
// icon set, and a title= attribute is a second tooltip, so neither may come
// back to a migrated file. Later units add their own files here.

const REPO_ROOT = process.cwd();

const SIDEBAR_DIR = "src/components/Sidebar";
const RIGHT_PANEL_DIR = "src/components/RightPanel";
const RESIZER_DIR = "src/components/Resizer";

function componentsIn(dir: string): string[] {
  return readdirSync(resolve(REPO_ROOT, dir))
    .filter((entry) => entry.endsWith(".tsx"))
    .map((entry) => join(dir, entry))
    .sort();
}

const MIGRATED = [
  "src/components/Button/Button.tsx",
  "src/components/Kbd/Kbd.tsx",
  "src/components/Tooltip/Tooltip.tsx",
  "src/components/Toolbar/Toolbar.tsx",
  "src/components/AiRewrite/AiRewriteOverlay.tsx",
  "src/components/ConfirmDialog/ConfirmDialog.tsx",
  "src/components/ContextMenu/ContextMenu.tsx",
  "src/components/ErrorBoundary/ErrorBoundary.tsx",
  "src/components/Notifications/Toast.tsx",
  "src/components/Preview/LinkConfirm.tsx",
  "src/components/PromptFill/PromptFillModal.tsx",
  "src/components/SettingsModal/SettingsModal.tsx",
  "src/components/ShortcutEditor/ShortcutEditor.tsx",
  ...componentsIn(SIDEBAR_DIR),
  ...componentsIn(RIGHT_PANEL_DIR),
  ...componentsIn(RESIZER_DIR),
];

const ICON_OWNERS = ["src/components/Icon/Icon.tsx", "src/components/Icon/IconSprite.tsx"];

function read(rel: string): string {
  return readFileSync(resolve(REPO_ROOT, rel), "utf8");
}

describe("migrated surfaces use the primitives", () => {
  it("covers every sidebar component", () => {
    expect(MIGRATED).toContain("src/components/Sidebar/Sidebar.tsx");
    expect(MIGRATED).toContain("src/components/Sidebar/TabItem.tsx");
    expect(MIGRATED).toContain("src/components/Sidebar/FileTree.tsx");
  });

  it("covers every component of the panel beside the note", () => {
    expect(MIGRATED).toContain("src/components/RightPanel/RightPanel.tsx");
    expect(MIGRATED).toContain("src/components/RightPanel/BacklinksSection.tsx");
    expect(MIGRATED).toContain("src/components/RightPanel/OutlineSection.tsx");
    expect(MIGRATED).toContain("src/components/RightPanel/PropertiesSection.tsx");
    expect(MIGRATED).toContain("src/components/Resizer/EdgeResizer.tsx");
  });

  it("no inline svg icon remains", () => {
    const offenders = MIGRATED.filter((rel) => read(rel).includes("<svg"));
    expect(offenders, `inline <svg> found in: ${offenders.join(", ")}`).toEqual([]);
  });

  it("no title attribute stands in for a tooltip", () => {
    const offenders = [...MIGRATED, ...ICON_OWNERS].filter((rel) => /\btitle=/.test(read(rel)));
    expect(offenders, `title= found in: ${offenders.join(", ")}`).toEqual([]);
  });

  it("the sprite is the only place svg markup is authored", () => {
    expect(read("src/components/Icon/Icon.tsx")).toContain("<use href=");
    expect(read("src/components/Sidebar/SearchBar.tsx")).toContain('<Icon name="magnifying-glass"');
  });

  it("the sidebar's icon-only controls carry a name and a tip", () => {
    const cases = [
      ["src/components/Sidebar/TabItem.tsx", ["Close tab", "Restore tab"]],
      ["src/components/Sidebar/FilesSection.tsx", ["Close folder"]],
      ["src/components/Sidebar/InboxSection.tsx", ["Stop watching folder"]],
    ] as const;
    for (const [rel, names] of cases) {
      const source = read(rel);
      for (const name of names) {
        expect(source, rel).toContain(`aria-label="${name}"`);
        expect(source, rel).toContain(`<Tooltip label="${name}">`);
      }
    }
  });
});
