import { describe, it, expect } from "vitest";
import { resolveChromeLayout, resolveLightsSlot, toolbarLeadsLights } from "../../lib/window-chrome";

describe("resolveChromeLayout", () => {
  it("gives macOS no title bar row and no caption controls", () => {
    const layout = resolveChromeLayout("mac");
    expect(layout.titleBar).toBe(false);
    expect(layout.caption).toBe("none");
    expect(layout.headerBar).toBe(false);
  });

  it("hands the macOS drag region to the toolbar", () => {
    expect(resolveChromeLayout("mac").dragHost).toBe("toolbar");
  });

  it("gives Windows a title bar with the three caption buttons", () => {
    const layout = resolveChromeLayout("win");
    expect(layout.titleBar).toBe(true);
    expect(layout.caption).toBe("win");
    expect(layout.headerBar).toBe(false);
    expect(layout.dragHost).toBe("titlebar");
  });

  it("merges the GNOME header bar and leaves it one close control", () => {
    const layout = resolveChromeLayout("linux");
    expect(layout.titleBar).toBe(true);
    expect(layout.headerBar).toBe(true);
    expect(layout.caption).toBe("linux-close");
    expect(layout.dragHost).toBe("titlebar");
  });

  it("puts the compose control in the chrome only where the header bar merges", () => {
    expect(resolveChromeLayout("linux").composeInChrome).toBe(true);
    expect(resolveChromeLayout("mac").composeInChrome).toBe(false);
    expect(resolveChromeLayout("win").composeInChrome).toBe(false);
  });
});

describe("resolveLightsSlot", () => {
  // One host, not one per sidebar state: a slot that changed with the sidebar
  // handed the lights to a box that was mid-animation, which moved them.
  it("gives the macOS lights the window's leading edge", () => {
    expect(resolveLightsSlot("mac")).toBe("window-lead");
  });

  it("draws no lights on the platforms that carry caption buttons", () => {
    expect(resolveLightsSlot("win")).toBe("none");
    expect(resolveLightsSlot("linux")).toBe("none");
  });
});

describe("toolbarLeadsLights", () => {
  // A closed sidebar is width 0, overflow hidden and inert, so nothing spans
  // the inset the lights are pinned over except the toolbar itself.
  it("reserves the toolbar's lead only while the sidebar is closed", () => {
    expect(toolbarLeadsLights("mac", false)).toBe(true);
    expect(toolbarLeadsLights("mac", true)).toBe(false);
  });

  it("reserves nothing where the shell draws its own caption", () => {
    expect(toolbarLeadsLights("win", false)).toBe(false);
    expect(toolbarLeadsLights("linux", false)).toBe(false);
  });
});
