import type { Platform } from "./platform";

/** Which caption controls the shell draws, if any. */
export type CaptionKind = "none" | "win" | "linux-close";

/** Which element hosts the window drag region. */
export type DragHost = "titlebar" | "toolbar";

/** Where the macOS window lights render. */
export type LightsSlot = "sidebar-head" | "toolbar-lead" | "none";

export interface ChromeLayout {
  /** A chrome row above the body. macOS has none: the toolbar is the top row. */
  titleBar: boolean;
  caption: CaptionKind;
  /** The chrome row and the toolbar merge into one GNOME header bar stack. */
  headerBar: boolean;
  /** The compose control sits in the chrome rather than in the toolbar. */
  composeInChrome: boolean;
  dragHost: DragHost;
}

const LAYOUTS: Readonly<Record<Platform, ChromeLayout>> = {
  mac: {
    titleBar: false,
    caption: "none",
    headerBar: false,
    composeInChrome: false,
    dragHost: "toolbar",
  },
  win: {
    titleBar: true,
    caption: "win",
    headerBar: false,
    composeInChrome: false,
    dragHost: "titlebar",
  },
  linux: {
    titleBar: true,
    caption: "linux-close",
    headerBar: true,
    composeInChrome: true,
    dragHost: "titlebar",
  },
};

/**
 * The shell each platform draws. Pure and platform-injected so every branch is
 * reachable from a test on any host — the chrome is the one part of the UI a
 * Mac cannot exercise by running the app.
 */
export function resolveChromeLayout(platform: Platform): ChromeLayout {
  return LAYOUTS[platform];
}

/**
 * macOS draws its own lights, so they must stay reachable in both sidebar
 * states: a closed sidebar is zero-width, clipped and inert, and a window with
 * no native decorations would have no way left to hide itself.
 */
export function resolveLightsSlot(platform: Platform, sidebarOpen: boolean): LightsSlot {
  if (platform !== "mac") return "none";
  return sidebarOpen ? "sidebar-head" : "toolbar-lead";
}
