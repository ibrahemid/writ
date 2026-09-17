import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// The settings area's own sheets. The rules below are stated over an explicit
// file list rather than over every sheet under src/components, because the
// sidebar and editor areas carry the same defects and land in their own passes:
// Sidebar/**, Editor/StatusBar.css, Editor/FirstRunHint.css, Editor/TabBar.css
// and Editor/FindOverlay.css still hold hand-written rings and literal sizes.
const AREA = [
  "src/components/SettingsModal/SettingsModal.css",
  "src/components/ShortcutEditor/ShortcutEditor.css",
  "src/components/ThemeEditor/ThemeEditor.css",
  "src/components/NoteHistory/NoteHistoryPanel.css",
  "src/components/Activity/ActivityPanel.css",
  "src/components/NotesMigrationReport/NotesMigrationReport.css",
  "src/components/ConfirmDialog/ConfirmDialog.css",
  "src/components/Notifications/Toast.css",
  "src/components/UpdateBanner/UpdateBanner.css",
  "src/components/PromptFill/PromptFillModal.css",
  "src/components/AiRewrite/AiRewriteOverlay.css",
  "src/components/ErrorBoundary/ErrorBoundary.css",
] as const;

const sheets = AREA.map((rel) => [rel, readFileSync(resolve(process.cwd(), rel), "utf8")] as const);

describe("the settings area draws the host's focus ring", () => {
  it("writes no ring of its own", () => {
    const offenders = sheets
      .filter(([, css]) => /outline:\s*\d+px solid var\(--writ-accent\)/.test(css))
      .map(([rel]) => rel);
    expect(offenders, `hand-written focus rings: ${offenders.join(", ")}`).toEqual([]);
  });

  it("cancels the ring through the attribute, not in CSS", () => {
    // focus.css:34 turns data-writ-focus-silent into the only opt-out, so a
    // control that silences the ring in CSS silences it everywhere, including
    // the shells whose ring is the only keyboard state they draw.
    const offenders = sheets
      .filter(([, css]) => /outline:\s*none/.test(css))
      .map(([rel]) => rel);
    expect(offenders, `outline: none in CSS: ${offenders.join(", ")}`).toEqual([]);
  });
});

describe("the settings area follows the interface text size", () => {
  it("states no font size in pixels", () => {
    const offenders = sheets
      .filter(([, css]) => /font-size:\s*\d/.test(css))
      .map(([rel]) => rel);
    expect(offenders, `literal font sizes: ${offenders.join(", ")}`).toEqual([]);
  });

  it("treats a text-bearing box's height as a floor", () => {
    for (const [rel, selector] of [
      ["src/components/SettingsModal/SettingsModal.css", ".settings-nav-item"],
      ["src/components/SettingsModal/SettingsModal.css", ".settings-search-bar"],
      ["src/components/PromptFill/PromptFillModal.css", ".placeholders-input"],
      ["src/components/AiRewrite/AiRewriteOverlay.css", ".ai-overlay-input"],
    ] as const) {
      const css = readFileSync(resolve(process.cwd(), rel), "utf8");
      const body = css.match(new RegExp(`^\\${selector}\\s*\\{([^}]*)\\}`, "m"));
      expect(body, `${selector} is declared on its own line`).toBeTruthy();
      expect(body![1], `${selector} sets a fixed height`).not.toMatch(/(^|[\s;])height:\s*\d/);
      expect(body![1], `${selector} has no floor`).toMatch(/min-height:\s*\d+px/);
    }
  });

  it("clamps every modal in the area to the viewport", () => {
    for (const [rel, selector] of [
      ["src/components/ShortcutEditor/ShortcutEditor.css", ".shortcut-editor"],
      ["src/components/ThemeEditor/ThemeEditor.css", ".theme-editor"],
    ] as const) {
      const css = readFileSync(resolve(process.cwd(), rel), "utf8");
      const body = css.match(new RegExp(`^\\${selector}\\s*\\{([^}]*)\\}`, "m"))![1];
      expect(body, selector).toMatch(/width:\s*min\(\d+px,\s*calc\(100vw - 48px\)\)/);
    }
  });
});

describe("the two app-level banners share one layer", () => {
  it("spend the banner token rather than a bare number", () => {
    for (const rel of [
      "src/components/UpdateBanner/UpdateBanner.css",
      "src/components/NotesMigrationReport/NotesMigrationReport.css",
    ]) {
      const css = readFileSync(resolve(process.cwd(), rel), "utf8");
      expect(css, rel).toContain("z-index: var(--writ-z-banner)");
      expect(css, rel).not.toMatch(/z-index:\s*\d/);
    }
  });
});

describe("the download bar under reduced motion", () => {
  const css = readFileSync(
    resolve(process.cwd(), "src/components/UpdateBanner/UpdateBanner.css"),
    "utf8",
  );

  it("reads as unknown rather than as a transfer stuck at 40%", () => {
    // The 40% belongs to the sliding animation. Outside the motion query the
    // track fills, so a reduced-motion reader sees an unknown size, not a
    // stall.
    const resting = css.match(
      /^\.update-banner-track\.indeterminate \.update-banner-fill\s*\{([^}]*)\}/m,
    );
    expect(resting, "the resting fill is declared").toBeTruthy();
    expect(resting![1]).toMatch(/width:\s*100%/);

    const motion = css.slice(css.indexOf("@media (prefers-reduced-motion: no-preference)"));
    expect(motion).toMatch(/width:\s*40%/);
  });
});

describe("the area's buttons are the app's button", () => {
  it("leaves no bespoke button in the two banners", () => {
    for (const rel of [
      "src/components/UpdateBanner/UpdateBanner.tsx",
      "src/components/NotesMigrationReport/NotesMigrationReport.tsx",
    ]) {
      const tsx = readFileSync(resolve(process.cwd(), rel), "utf8");
      expect(tsx, rel).not.toMatch(/<button[\s>]/);
    }
  });
});
