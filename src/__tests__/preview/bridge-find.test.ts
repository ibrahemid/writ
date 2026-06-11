import { describe, it, expect, beforeEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const BRIDGE_SRC = readFileSync(
  resolve(process.cwd(), "src-tauri/assets/preview/bridge.js"),
  "utf8",
);

interface FindResult {
  type?: string;
  current?: number;
  total?: number;
  capped?: boolean;
  ticks?: { fraction: number }[];
}

// In-preview find runs against the real rendered DOM, so this harness uses a
// genuine jsdom document (not the plain-object scroll mock) and drives the
// shipped bridge source against it.
function runWithDom(bodyHtml: string) {
  document.body.innerHTML = bodyHtml;
  const handlers: Record<string, (ev: unknown) => void> = {};
  const posted: FindResult[] = [];
  const win = {
    addEventListener(type: string, fn: (ev: unknown) => void) {
      handlers[type] = fn;
    },
  };
  const parentWin = { postMessage: (msg: FindResult) => posted.push(msg) };
  // eslint-disable-next-line @typescript-eslint/no-implied-eval
  const fn = new Function("window", "document", "self", "parent", BRIDGE_SRC);
  fn(win, document, win, parentWin);
  function down(msg: Record<string, unknown>) {
    handlers.message?.({ data: { source: "writ-preview", dir: "down", ...msg } });
  }
  return {
    posted,
    find: (term: Partial<{ query: string; caseSensitive: boolean; wholeWord: boolean; regexp: boolean }>) =>
      down({
        type: "find",
        query: "",
        caseSensitive: false,
        wholeWord: false,
        regexp: false,
        ...term,
      }),
    next: () => down({ type: "findNext" }),
    prev: () => down({ type: "findPrev" }),
    clear: () => down({ type: "findClear" }),
    lastResult: () => {
      const results = posted.filter((m) => m.type === "findResult");
      return results[results.length - 1] as FindResult | undefined;
    },
    marks: () => document.querySelectorAll("mark.writ-find"),
    currentMarks: () => document.querySelectorAll("mark.writ-find-current"),
  };
}

describe("preview bridge find", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("counts every match of a plain query", () => {
    const h = runWithDom("<p>hello world hello</p>");
    h.find({ query: "hello" });
    const r = h.lastResult();
    expect(r?.total).toBe(2);
    expect(r?.current).toBe(1);
    expect(h.marks().length).toBe(2);
  });

  it("marks the first match as current", () => {
    const h = runWithDom("<p>hello world hello</p>");
    h.find({ query: "hello" });
    expect(h.currentMarks().length).toBe(1);
    expect(h.currentMarks()[0].textContent).toBe("hello");
  });

  it("advances and wraps the current match with next / previous", () => {
    const h = runWithDom("<p>a hello b hello c hello</p>");
    h.find({ query: "hello" });
    expect(h.lastResult()?.current).toBe(1);
    h.next();
    expect(h.lastResult()?.current).toBe(2);
    h.next();
    expect(h.lastResult()?.current).toBe(3);
    h.next(); // wraps
    expect(h.lastResult()?.current).toBe(1);
    h.prev(); // wraps backward
    expect(h.lastResult()?.current).toBe(3);
  });

  it("honors case sensitivity", () => {
    const h = runWithDom("<p>hello Hello HELLO</p>");
    h.find({ query: "Hello", caseSensitive: true });
    expect(h.lastResult()?.total).toBe(1);
  });

  it("honors whole-word matching", () => {
    const h = runWithDom("<p>cat category cat scatter</p>");
    h.find({ query: "cat", wholeWord: true });
    expect(h.lastResult()?.total).toBe(2);
  });

  it("supports regular expressions", () => {
    const h = runWithDom("<p>hello hallo hullo</p>");
    h.find({ query: "h.llo", regexp: true });
    expect(h.lastResult()?.total).toBe(3);
  });

  it("reports zero and creates no marks for an empty query", () => {
    const h = runWithDom("<p>hello</p>");
    h.find({ query: "" });
    expect(h.lastResult()?.total).toBe(0);
    expect(h.marks().length).toBe(0);
  });

  it("reports zero for an invalid regular expression without throwing", () => {
    const h = runWithDom("<p>hello</p>");
    h.find({ query: "(", regexp: true });
    expect(h.lastResult()?.total).toBe(0);
  });

  it("matches across inline element boundaries", () => {
    const h = runWithDom("<p>he<strong>ll</strong>o world</p>");
    h.find({ query: "hello" });
    expect(h.lastResult()?.total).toBe(1);
    expect(h.marks().length).toBeGreaterThanOrEqual(1);
  });

  it("restores the original text when cleared", () => {
    const h = runWithDom("<p>hello world hello</p>");
    h.find({ query: "hello" });
    expect(h.marks().length).toBe(2);
    h.clear();
    expect(h.marks().length).toBe(0);
    expect(document.body.textContent).toBe("hello world hello");
  });

  it("does not match text inside script or style elements", () => {
    const h = runWithDom("<style>hello{}</style><p>hello</p><script>var hello=1;</script>");
    h.find({ query: "hello" });
    expect(h.lastResult()?.total).toBe(1);
  });
});
