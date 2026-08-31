import { describe, it, expect } from "vitest";
import { resolveChromeLayout, resolveLightsSlot } from "../../lib/window-chrome";

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
  it("parks the macOS lights in the sidebar head while the sidebar is open", () => {
    expect(resolveLightsSlot("mac", true)).toBe("sidebar-head");
  });

  // A closed sidebar is width 0, overflow hidden and inert, so lights left
  // there would be both invisible and unclickable — with no native decorations
  // that leaves no way to hide the window.
  it("moves the macOS lights to the toolbar lead when the sidebar is closed", () => {
    expect(resolveLightsSlot("mac", false)).toBe("toolbar-lead");
  });

  it("draws no lights on the platforms that carry caption buttons", () => {
    expect(resolveLightsSlot("win", true)).toBe("none");
    expect(resolveLightsSlot("win", false)).toBe("none");
    expect(resolveLightsSlot("linux", true)).toBe("none");
    expect(resolveLightsSlot("linux", false)).toBe("none");
  });
});
