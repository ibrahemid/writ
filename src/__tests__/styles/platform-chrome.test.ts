import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// The window chrome is the one surface a Mac cannot exercise by running the
// app, so the platform layer is asserted from the stylesheets themselves. It is
// one selector root by contract: :root[data-platform="…"], written once at boot.

const ROOT = process.cwd();

function sheet(rel: string): string {
  return readFileSync(resolve(ROOT, rel), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
}

function declarations(css: string, selector: string): Map<string, string> {
  for (const [, list, body] of css.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    if (!list.split(",").some((s) => s.trim() === selector)) continue;
    const found = new Map<string, string>();
    for (const line of body.split(";")) {
      const [property, ...rest] = line.split(":");
      if (rest.length === 0) continue;
      found.set(property.trim(), rest.join(":").trim());
    }
    return found;
  }
  throw new Error(`no rule for ${selector}`);
}

const TITLEBAR = sheet("src/components/TitleBar/TitleBar.css");
const FOCUS = sheet("src/styles/focus.css");
const APP = sheet("src/App.css");
const SIDEBAR = sheet("src/components/Sidebar/Sidebar.css");
const TOOLBAR = sheet("src/components/Toolbar/Toolbar.css");
const THEME = sheet("src/styles/generated/theme.css");

describe("the platform layer keys off one selector root", () => {
  it("scopes every platform-specific rule to the root attribute", () => {
    const scoped = [...TITLEBAR.matchAll(/([^{}]+)\{/g)]
      .map((m) => m[1].trim())
      .filter((s) => s.includes("data-platform"));
    expect(scoped.length).toBeGreaterThan(0);
    expect(scoped.every((s) => s.startsWith(':root[data-platform="'))).toBe(true);
  });

  it("styles nothing through the titlebar-<platform> class, which is a handle", () => {
    for (const css of [TITLEBAR, FOCUS, APP, SIDEBAR]) {
      expect(/\.titlebar-(mac|win|linux)\b/.test(css)).toBe(false);
    }
  });
});

describe("Windows caption row", () => {
  it("is 32px with the documented 16px lead", () => {
    const bar = declarations(TITLEBAR, ':root[data-platform="win"] .titlebar');
    expect(bar.get("height")).toBe("32px");
    expect(bar.get("padding-left")).toBe("var(--writ-space-5)");
  });

  it("keeps a 46x32 caption button", () => {
    const cap = declarations(TITLEBAR, ".winctrl");
    expect(cap.get("width")).toBe("46px");
    expect(cap.get("height")).toBe("32px");
  });

  it("holds the drag strip to the documented 48px minimum", () => {
    const drag = declarations(TITLEBAR, ".titlebar-drag");
    expect(drag.get("flex")).toBe("1");
    expect(drag.get("min-width")).toBe("48px");
  });

  it("paints close hover from the close tokens, not a danger alias", () => {
    const close = declarations(TITLEBAR, ".winctrl-close:hover");
    expect(close.get("background")).toBe("var(--writ-win-close-bg)");
    expect(close.get("color")).toBe("var(--writ-win-close-fg)");
    expect(TITLEBAR).not.toContain("winctrl-danger");
  });

  it("strokes the content layer under the caption row", () => {
    const body = declarations(APP, ':root[data-platform="win"] .app-body');
    expect(body.get("border-top-left-radius")).toBe("var(--writ-r-window)");
    expect(body.get("box-shadow")).toBe("inset 1px 1px 0 var(--writ-win-layer-stroke)");
  });
});

describe("GNOME header bar", () => {
  it("is one 47px row laid out 1fr auto 1fr on the header-bar ground", () => {
    const bar = declarations(TITLEBAR, ':root[data-platform="linux"] .headerbar');
    expect(bar.get("height")).toBe("47px");
    expect(bar.get("grid-template-columns")).toBe("1fr auto 1fr");
    expect(bar.get("padding")).toBe("6px 7px 7px");
    expect(bar.get("background")).toBe("var(--writ-lin-headerbar-bg)");
  });

  it("centres the title at the GNOME metric", () => {
    const title = declarations(TITLEBAR, ".headerbar-title");
    expect(title.get("font-size")).toBe("14.67px");
    expect(title.get("line-height")).toBe("20.5px");
    expect(title.get("font-weight")).toBe("700");
  });

  it("draws the close control as a 24px disc in a 34px box", () => {
    expect(declarations(TITLEBAR, ".gnomectrl").get("width")).toBe("34px");
    const disc = declarations(TITLEBAR, ".gnomectrl::before");
    expect(disc.get("width")).toBe("24px");
    expect(disc.get("border-radius")).toBe("var(--writ-r-pill)");
    expect(disc.get("background")).toBe("color-mix(in srgb, currentColor 10%, transparent)");
    expect(declarations(TITLEBAR, ".gnomectrl:hover::before").get("background")).toBe(
      "color-mix(in srgb, currentColor 15%, transparent)",
    );
  });

  it("paints the client-side-decoration shadow the compositor will not", () => {
    expect(declarations(APP, ':root[data-platform="linux"] .app-container').get("box-shadow")).toBe(
      "var(--writ-shadow-csd)",
    );
  });

  // The window is transparent and the frame fills it, so without room around
  // the frame the shadow paints past the webview edge and is clipped away.
  it("insets the frame far enough for the shadow to land", () => {
    const frame = declarations(APP, ':root[data-platform="linux"] .app-container');
    expect(frame.get("margin")).toBe("var(--writ-frame-shadow-inset)");
    expect(THEME).toContain("--writ-frame-shadow-inset: 19px;");
  });

  // A maximized window meets the screen edge: no shadow to make room for, and
  // no corners to round. Both follow the one class the frame already carries.
  it("drops the inset, the radius and the shadow together when maximized", () => {
    const maximized = declarations(
      APP,
      ':root[data-platform="linux"] .app-container.is-maximized',
    );
    expect(maximized.get("margin")).toBe("0");
    expect(maximized.get("border-radius")).toBe("0");
    expect(maximized.get("box-shadow")).toBe("none");
    expect(
      declarations(APP, ':root[data-platform="win"] .app-container.is-maximized').get(
        "border-radius",
      ),
    ).toBe("0");
  });

  it("leaves the mac and Windows frames flush against the window", () => {
    const rules = [...APP.matchAll(/([^{}]+)\{([^}]*)\}/g)].map(([, list, body]) => ({
      selector: list.trim(),
      body,
    }));
    for (const rule of rules) {
      if (rule.selector.includes('data-platform="linux"')) continue;
      expect(rule.body, rule.selector).not.toMatch(/(^|;)\s*margin\s*:/);
      expect(rule.body, rule.selector).not.toContain("--writ-frame-shadow-inset");
    }
    // Declared for the one shell that needs it, so the other two cannot read it.
    expect(THEME.match(/--writ-frame-shadow-inset:/g)).toHaveLength(1);
  });
});

describe("macOS lights", () => {
  it("sits the head at the toolbar's height, inset where the system draws them", () => {
    const head = declarations(SIDEBAR, ".sidebar-head");
    expect(head.get("height")).toBe("44px");
    expect(head.get("padding")).toBe("0 20px");
  });

  it("spaces the 12px lights by 8px", () => {
    expect(declarations(TITLEBAR, ".window-lights").get("gap")).toBe("var(--writ-space-3)");
    expect(declarations(TITLEBAR, ".maclight").get("width")).toBe("12px");
  });

  // The lights used to change parent with the sidebar state, so the first
  // frame of the width animation moved them. The layer is parented to neither
  // pane and positioned against the window, which is what holds them still.
  it("pins the layer to the window's leading edge over both panes", () => {
    const layer = declarations(TITLEBAR, ".window-lights-layer");
    expect(layer.get("position")).toBe("absolute");
    expect(layer.get("top")).toBe("0");
    expect(layer.get("left")).toBe("0");
    expect(layer.get("height")).toBe(declarations(SIDEBAR, ".sidebar-head").get("height"));
    expect(layer.get("padding-left")).toBe("20px");
    expect(layer.get("z-index")).toBe("var(--writ-z-chrome)");
    expect(layer.get("pointer-events")).toBe("none");
    expect(declarations(TITLEBAR, ".window-lights").get("pointer-events")).toBe("auto");
  });

  // Reserved on the same motion token as the sidebar width, so the leading
  // control tracks the sidebar instead of crossing under the pinned lights.
  it("eases the toolbar's reservation on the sidebar's own motion token", () => {
    const bar = declarations(TOOLBAR, ':root[data-platform="mac"] .writ-toolbar');
    expect(bar.get("transition")).toBe("padding-left var(--writ-motion)");
    expect(declarations(SIDEBAR, ".sidebar").get("transition")).toBe("width var(--writ-motion)");
    const lead = declarations(TOOLBAR, ':root[data-platform="mac"] .writ-toolbar.leads-lights');
    expect(lead.get("padding-left")).toBe("var(--writ-window-lights-lead, 84px)");
  });

  it("takes the amber from the baseline value the host paints", () => {
    expect(THEME).toContain("--writ-traffic-minimize: #FEBC2E;");
  });
});

describe("focus rings follow the host", () => {
  // The rule stays at one selector: a platform-scoped :focus-visible would beat
  // CodeMirror's own `outline: none` and stroke the editor's scroller.
  it("draws every ring from one unscoped rule", () => {
    const ring = declarations(FOCUS, ":focus-visible");
    expect(ring.get("outline")).toBe("var(--writ-focus-outline, 2px solid var(--writ-accent))");
    expect(ring.get("outline-offset")).toBe("var(--writ-focus-offset, 2px)");
    expect(FOCUS).not.toMatch(/data-platform="[a-z]+"\] :focus-visible/);
  });

  // CodeMirror clears `outline` on .cm-content and nothing else, and the
  // focus-silent opt-out can only take back what the rule draws through
  // `outline`. Fluent's second ring lives on `.winctrl:focus-visible`.
  it("draws the ring through outline alone", () => {
    expect(declarations(FOCUS, ":focus-visible").has("box-shadow")).toBe(false);
    expect(FOCUS).not.toContain("--writ-focus-shadow");
  });

  it("keeps the offset accent ring as the default", () => {
    const base = declarations(FOCUS, ":root");
    expect(base.get("--writ-focus-outline")).toBe("2px solid var(--writ-accent)");
    expect(base.get("--writ-focus-offset")).toBe("2px");
  });

  it("gives Windows the tighter Fluent ring", () => {
    const ring = declarations(FOCUS, ':root[data-platform="win"]');
    expect(ring.get("--writ-focus-outline")).toBe("2px solid var(--writ-win-focus-outer)");
    expect(ring.get("--writ-focus-offset")).toBe("1px");
  });

  // A caption control runs to the window edge, so the ring is drawn inward, but
  // the stacking order is Fluent's: the contrast stroke outermost, a 1px band of
  // the opposite polarity inside it. Both tokens swap with the theme, so drawing
  // them the other way round hides whichever one carries the contrast.
  it("stacks the caption ring contrast-outermost", () => {
    const ring = declarations(TITLEBAR, ".winctrl:focus-visible");
    const stroke = ring.get("outline")!.match(/^(\d+)px solid var\((--writ-win-focus-\w+)\)$/);
    const band = ring.get("box-shadow")!.match(/^inset 0 0 0 (\d+)px var\((--writ-win-focus-\w+)\)$/);
    expect(stroke, ring.get("outline")).not.toBeNull();
    expect(band, ring.get("box-shadow")).not.toBeNull();

    const width = Number(stroke![1]);
    const depth = Math.abs(Number(ring.get("outline-offset")!.replace("px", "")));
    const spread = Number(band![1]);

    // Depths measured inward from the button's own edge. The outline is drawn
    // outward from an edge `depth` inside the box; the inset shadow fills from
    // the edge to `spread` and is painted under the outline.
    expect([depth - width, depth]).toEqual([0, 2]);
    expect([depth, spread]).toEqual([2, 3]);
    expect(stroke![2]).toBe("--writ-win-focus-outer");
    expect(band![2]).toBe("--writ-win-focus-inner");
  });

  it("draws the GNOME ring inside the control at half the accent", () => {
    const ring = declarations(FOCUS, ':root[data-platform="linux"]');
    expect(ring.get("--writ-focus-outline")).toBe(
      "2px solid color-mix(in srgb, var(--writ-accent) 50%, transparent)",
    );
    expect(ring.get("--writ-focus-offset")).toBe("-2px");
  });
});

describe("window frame", () => {
  it("takes its radius from the platform token", () => {
    expect(declarations(APP, ".app-container").get("border-radius")).toBe("var(--writ-r-window)");
    expect(THEME).toContain("--writ-r-window: 10px;");
    expect(THEME).toContain("--writ-r-window: 8px;");
    expect(THEME).toContain("--writ-r-window: 15px;");
  });

  it("meets the screen edge square once maximized", () => {
    expect(
      declarations(APP, ':root[data-platform="win"] .app-container.is-maximized').get(
        "border-radius",
      ),
    ).toBe("0");
  });

  it("draws its own border only on macOS", () => {
    expect(declarations(APP, ':root[data-platform="mac"] .app-container').get("border")).toBe(
      "1px solid var(--writ-border-soft)",
    );
    expect(declarations(APP, ".app-container").has("border")).toBe(false);
  });
});
