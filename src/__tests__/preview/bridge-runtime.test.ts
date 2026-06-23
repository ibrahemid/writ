import { describe, it, expect, beforeEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// The bridge runtime is a same-origin script injected into the cross-origin
// preview iframe. It is authored as a standalone IIFE that reads its window /
// document / parent from free identifiers, so the test runs the exact shipped
// source against mocks — no separate module to drift out of sync.
const BRIDGE_SRC = readFileSync(
  resolve(process.cwd(), "src-tauri/assets/preview/bridge.js"),
  "utf8",
);

interface Posted {
  source?: string;
  dir?: string;
  type?: string;
  fraction?: number;
}

function makeScroller(scrollHeight: number, clientHeight: number) {
  return { scrollHeight, clientHeight, scrollTop: 0 };
}

function run(scroller: { scrollHeight: number; clientHeight: number; scrollTop: number }) {
  const handlers: Record<string, (ev: unknown) => void> = {};
  const posted: Posted[] = [];
  const win = {
    addEventListener(type: string, fn: (ev: unknown) => void) {
      handlers[type] = fn;
    },
  };
  const attrs: Record<string, string> = {};
  const style: Record<string, string> = {};
  const documentElement = {
    style,
    setAttribute(key: string, value: string) {
      attrs[key] = value;
    },
    removeAttribute(key: string) {
      delete attrs[key];
    },
  };
  const doc = { scrollingElement: scroller, documentElement };
  const parentWin = {
    postMessage(msg: Posted) {
      posted.push(msg);
    },
  };
  // eslint-disable-next-line @typescript-eslint/no-implied-eval
  const fn = new Function("window", "document", "self", "parent", BRIDGE_SRC);
  fn(win, doc, win, parentWin);
  return {
    handlers,
    posted,
    scroller,
    attrs,
    style,
    fireScroll: () => handlers.scroll?.({}),
    fireKey: (ev: Record<string, unknown>) => {
      let prevented = false;
      handlers.keydown?.({ ...ev, preventDefault: () => { prevented = true; } });
      return prevented;
    },
    fireWheel: (ev: Record<string, unknown>) => {
      let prevented = false;
      handlers.wheel?.({ ...ev, preventDefault: () => { prevented = true; } });
      return prevented;
    },
    sendDown: (msg: Posted) =>
      handlers.message?.({ data: { source: "writ-preview", dir: "down", ...msg } }),
  };
}

describe("preview bridge runtime", () => {
  let env: ReturnType<typeof run>;
  beforeEach(() => {
    // 1000px content in a 200px viewport → 800px scroll range.
    env = run(makeScroller(1000, 200));
  });

  it("announces readiness to the parent on load", () => {
    expect(env.posted.some((m) => m.type === "ready" && m.source === "writ-preview")).toBe(true);
  });

  it("tags outbound messages as upward so the parent can filter them", () => {
    const ready = env.posted.find((m) => m.type === "ready");
    expect(ready?.dir).toBe("up");
  });

  it("scrolls to a parent-driven fraction", () => {
    env.sendDown({ type: "scrollTo", fraction: 0.5 });
    expect(env.scroller.scrollTop).toBe(400); // 0.5 * (1000 - 200)
  });

  it("flips to the light reading palette on a setTheme message", () => {
    env.sendDown({ type: "setTheme", theme: "light" } as unknown as Posted);
    expect(env.attrs["data-writ-theme"]).toBe("light");
  });

  it("clears the light palette attribute when switched back to dark", () => {
    env.sendDown({ type: "setTheme", theme: "light" } as unknown as Posted);
    env.sendDown({ type: "setTheme", theme: "dark" } as unknown as Posted);
    expect(env.attrs["data-writ-theme"]).toBeUndefined();
  });

  it("posts the scroll fraction on a genuine user scroll", () => {
    env.scroller.scrollTop = 600;
    env.fireScroll();
    const scrolls = env.posted.filter((m) => m.type === "scroll");
    const scroll = scrolls[scrolls.length - 1];
    expect(scroll?.fraction).toBeCloseTo(600 / 800, 5);
    expect(scroll?.dir).toBe("up");
  });

  it("does not echo a programmatic scrollTo back to the parent", () => {
    env.sendDown({ type: "scrollTo", fraction: 0.25 });
    // The scroll event the programmatic set would raise must be swallowed.
    env.fireScroll();
    expect(env.posted.some((m) => m.type === "scroll")).toBe(false);
  });

  it("recognises the echo despite HiDPI sub-pixel landing", () => {
    env.sendDown({ type: "scrollTo", fraction: 0.25 }); // target 200
    env.scroller.scrollTop = 199.5; // device-pixel snap
    env.fireScroll();
    expect(env.posted.some((m) => m.type === "scroll")).toBe(false);
  });

  it("treats a coalesced settle at the latest target as an echo", () => {
    env.sendDown({ type: "scrollTo", fraction: 0.2 });
    env.sendDown({ type: "scrollTo", fraction: 0.5 });
    env.sendDown({ type: "scrollTo", fraction: 0.9 }); // box at 720
    env.fireScroll(); // single coalesced echo
    expect(env.posted.some((m) => m.type === "scroll")).toBe(false);
  });

  it("resumes posting after the suppressed echo is consumed", () => {
    env.sendDown({ type: "scrollTo", fraction: 0.25 });
    env.fireScroll(); // suppressed echo
    env.scroller.scrollTop = 800;
    env.fireScroll(); // genuine user scroll
    expect(env.posted.filter((m) => m.type === "scroll").length).toBe(1);
  });

  it("ignores a non-finite parent fraction", () => {
    env.scroller.scrollTop = 0;
    env.sendDown({ type: "scrollTo", fraction: Number.NaN });
    expect(env.scroller.scrollTop).toBe(0);
  });

  it("ignores messages that are not tagged from the parent", () => {
    env.scroller.scrollTop = 0;
    env.handlers.message?.({ data: { type: "scrollTo", fraction: 0.9 } });
    expect(env.scroller.scrollTop).toBe(0);
  });

  it("applies a parent-driven zoom factor to the document root", () => {
    env.sendDown({ type: "setZoom", factor: 2 } as unknown as Posted);
    expect(env.style.zoom).toBe("2");
  });

  it("ignores a non-positive or non-finite zoom factor", () => {
    env.sendDown({ type: "setZoom", factor: 1.5 } as unknown as Posted);
    env.sendDown({ type: "setZoom", factor: 0 } as unknown as Posted);
    env.sendDown({ type: "setZoom", factor: Number.NaN } as unknown as Posted);
    env.sendDown({ type: "setZoom", factor: -3 } as unknown as Posted);
    expect(env.style.zoom).toBe("1.5"); // last valid value held
  });

  it("forwards Cmd+= / Cmd+- / Cmd+0 over the preview as zoom intents", () => {
    expect(env.fireKey({ key: "=", metaKey: true })).toBe(true);
    expect(env.fireKey({ key: "-", metaKey: true })).toBe(true);
    expect(env.fireKey({ key: "0", metaKey: true })).toBe(true);
    const intents = env.posted.filter(
      (m) => m.type === "zoomStep" || m.type === "zoomReset",
    );
    expect(intents).toEqual([
      { source: "writ-preview", dir: "up", type: "zoomStep", direction: 1 },
      { source: "writ-preview", dir: "up", type: "zoomStep", direction: -1 },
      { source: "writ-preview", dir: "up", type: "zoomReset" },
    ]);
  });

  it("leaves an unmodified keystroke alone so typing in the preview is untouched", () => {
    const before = env.posted.length;
    expect(env.fireKey({ key: "=", metaKey: false })).toBe(false);
    expect(env.posted.length).toBe(before);
  });

  it("forwards Cmd/Ctrl + wheel as a zoom-wheel intent and swallows the scroll", () => {
    expect(env.fireWheel({ deltaY: -120, ctrlKey: true })).toBe(true);
    const wheel = env.posted.filter((m) => m.type === "zoomWheel");
    expect(wheel).toEqual([
      { source: "writ-preview", dir: "up", type: "zoomWheel", deltaY: -120 },
    ]);
  });

  it("leaves a plain wheel event to scroll the preview normally", () => {
    const before = env.posted.length;
    expect(env.fireWheel({ deltaY: 120 })).toBe(false);
    expect(env.posted.some((m) => m.type === "zoomWheel")).toBe(false);
    expect(env.posted.length).toBe(before);
  });
});
