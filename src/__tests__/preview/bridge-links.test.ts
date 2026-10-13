import { describe, it, expect, beforeEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// Runs the exact shipped bridge source against mocks, like the other bridge
// suites — the runtime is a standalone IIFE reading its window / document /
// parent from free identifiers.
const BRIDGE_SRC = readFileSync(
  resolve(process.cwd(), "src-tauri/assets/preview/bridge.js"),
  "utf8",
);

interface Posted {
  source?: string;
  dir?: string;
  type?: string;
  href?: string;
  x?: number;
  y?: number;
}

interface FakeNode {
  nodeType: number;
  tagName?: string;
  localName?: string;
  parentNode: FakeNode | null;
  getAttribute?: (name: string) => string | null;
  getAttributeNS?: (ns: string, name: string) => string | null;
}

const XLINK_NS = "http://www.w3.org/1999/xlink";

// An <a> inside inline SVG: lowercase tag name, destination in href or in the
// legacy xlink:href.
function svgAnchor(href: string | null, xlinkHref: string | null = null): FakeNode {
  return {
    nodeType: 1,
    tagName: "a",
    localName: "a",
    parentNode: null,
    getAttribute: (name: string) => (name === "href" ? href : null),
    getAttributeNS: (ns: string, name: string) =>
      ns === XLINK_NS && name === "href" ? xlinkHref : null,
  };
}

function area(href: string | null): FakeNode {
  return {
    nodeType: 1,
    tagName: "AREA",
    localName: "area",
    parentNode: null,
    getAttribute: (name: string) => (name === "href" ? href : null),
  };
}

function anchor(href: string | null, parent: FakeNode | null = null): FakeNode {
  return {
    nodeType: 1,
    tagName: "A",
    parentNode: parent,
    getAttribute: (name: string) => (name === "href" ? href : null),
  };
}

function textNode(parent: FakeNode | null): FakeNode {
  return { nodeType: 3, parentNode: parent };
}

function run() {
  const handlers: Record<string, (ev: unknown) => void> = {};
  const posted: Posted[] = [];
  const win = {
    addEventListener(type: string, fn: (ev: unknown) => void) {
      handlers[type] = fn;
    },
  };
  const documentElement = {
    style: {} as Record<string, string>,
    setAttribute() {},
    removeAttribute() {},
  };
  const doc = {
    scrollingElement: { scrollHeight: 1000, clientHeight: 200, scrollTop: 0 },
    documentElement,
  };
  const parentWin = {
    postMessage(msg: Posted) {
      posted.push(msg);
    },
  };
  // eslint-disable-next-line @typescript-eslint/no-implied-eval
  const fn = new Function("window", "document", "self", "parent", BRIDGE_SRC);
  fn(win, doc, win, parentWin);
  return {
    posted,
    click: (ev: Record<string, unknown>) => {
      let prevented = false;
      handlers.click?.({
        button: 0,
        clientX: 0,
        clientY: 0,
        defaultPrevented: false,
        ...ev,
        preventDefault: () => {
          prevented = true;
        },
      });
      return prevented;
    },
    links: () => posted.filter((m) => m.type === "link:open"),
  };
}

describe("preview bridge links", () => {
  let env: ReturnType<typeof run>;
  beforeEach(() => {
    env = run();
  });

  it("reports an anchor click upward and stops the frame navigating", () => {
    const prevented = env.click({
      target: anchor("https://example.com/docs"),
      clientX: 120,
      clientY: 64,
    });
    expect(prevented).toBe(true);
    expect(env.links()).toHaveLength(1);
    expect(env.links()[0]).toMatchObject({
      source: "writ-preview",
      dir: "up",
      href: "https://example.com/docs",
      x: 120,
      y: 64,
    });
  });

  it("reports the raw href, leaving the policy to Rust", () => {
    for (const href of ["javascript:alert(1)", "file:///etc/passwd", "./notes.md"]) {
      const local = run();
      expect(local.click({ target: anchor(href) })).toBe(true);
      expect(local.links()[0]?.href).toBe(href);
    }
  });

  it("finds the anchor from a click on its content", () => {
    const a = anchor("https://example.com/");
    env.click({ target: textNode(a) });
    expect(env.links()[0]?.href).toBe("https://example.com/");
  });

  it("leaves a same-document fragment to the frame", () => {
    expect(env.click({ target: anchor("#section") })).toBe(false);
    expect(env.links()).toHaveLength(0);
  });

  it("ignores a click that is not on an anchor", () => {
    expect(env.click({ target: textNode(null) })).toBe(false);
    expect(env.links()).toHaveLength(0);
  });

  it("ignores an anchor with no href", () => {
    expect(env.click({ target: anchor(null) })).toBe(false);
    expect(env.click({ target: anchor("") })).toBe(false);
    expect(env.links()).toHaveLength(0);
  });

  it("reports a click on an SVG link and stops the frame navigating", () => {
    expect(env.click({ target: svgAnchor("https://example.com/svg") })).toBe(true);
    expect(env.links()[0]?.href).toBe("https://example.com/svg");
  });

  it("reads an SVG link's legacy xlink:href", () => {
    expect(env.click({ target: svgAnchor(null, "https://example.com/xlink") })).toBe(true);
    expect(env.links()[0]?.href).toBe("https://example.com/xlink");
  });

  it("finds an SVG link from a click on the shape inside it", () => {
    const link = svgAnchor("https://example.com/shape");
    const shape: FakeNode = { nodeType: 1, tagName: "rect", localName: "rect", parentNode: link };
    expect(env.click({ target: shape })).toBe(true);
    expect(env.links()[0]?.href).toBe("https://example.com/shape");
  });

  it("reports a click on an image-map area and stops the frame navigating", () => {
    expect(env.click({ target: area("https://example.com/area") })).toBe(true);
    expect(env.links()[0]?.href).toBe("https://example.com/area");
  });

  it("ignores a non-primary button and an already-handled click", () => {
    expect(env.click({ target: anchor("https://example.com/"), button: 1 })).toBe(false);
    expect(
      env.click({ target: anchor("https://example.com/"), defaultPrevented: true }),
    ).toBe(false);
    expect(env.links()).toHaveLength(0);
  });
});
