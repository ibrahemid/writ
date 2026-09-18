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

describe("the settings area follows the interface text size", () => {
  it("letter-spaces no label, and shouts at none", () => {
    // The baseline's token table is explicit: --writ-ui-tracking is 0 and UI
    // labels are never tracked or upper-cased.
    const tracked = sheets
      .filter(([, css]) =>
        [...css.matchAll(/letter-spacing:\s*([^;]+);/g)].some(
          ([, value]) => !value.trim().startsWith("var("),
        ),
      )
      .map(([rel]) => rel);
    expect(tracked, `literal letter-spacing: ${tracked.join(", ")}`).toEqual([]);

    const shouted = sheets
      .filter(([, css]) => /text-transform:\s*uppercase/.test(css))
      .map(([rel]) => rel);
    expect(shouted, `uppercase labels: ${shouted.join(", ")}`).toEqual([]);
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

describe("the accent row's pitch survives the bigger hit box", () => {
  const css = readFileSync(
    resolve(process.cwd(), "src/components/SettingsModal/SettingsModal.css"),
    "utf8",
  );

  function rule(selector: string): string {
    const match = css.match(new RegExp(`^\\${selector}\\s*\\{([^}]*)\\}`, "m"));
    expect(match, `${selector} is declared`).toBeTruthy();
    return match![1];
  }

  function local(body: string, name: string): number {
    const found = body.match(new RegExp(`${name}:\\s*(\\d+)px`));
    expect(found, `${name} is declared`).toBeTruthy();
    return Number(found![1]);
  }

  // Before the hit box grew, the swatch was 16px and the gap 9px. The box is
  // 24px now, so the gap owes the row the 8px difference: the six swatches sit
  // exactly where they sat.
  const SWATCH_PITCH = 16 + 9;

  it("keeps swatch to swatch at the distance it was", () => {
    const row = rule(".settings-accents");
    const pitch = local(row, "--writ-accent-pitch");
    const box = local(row, "--writ-accent-box");

    expect(pitch).toBe(SWATCH_PITCH);
    expect(box).toBeGreaterThanOrEqual(24);
    expect(row).toMatch(/gap:\s*calc\(var\(--writ-accent-pitch[^)]*\) - var\(--writ-accent-box/);

    const swatch = rule(".settings-accent");
    expect(swatch).toMatch(/width:\s*var\(--writ-accent-box/);
    expect(swatch).toMatch(/height:\s*var\(--writ-accent-box/);
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
