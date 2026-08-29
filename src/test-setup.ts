// jsdom does not implement Range.getClientRects / getBoundingClientRect, which
// CodeMirror calls from its layout-measurement pass (scheduled on
// requestAnimationFrame). Without these, any test that drives an EditorView and
// lets a frame flush throws "getClientRects is not a function" as an unhandled
// error. Provide empty-rect stubs so the measure pass is a no-op under jsdom.

const EMPTY_RECT: DOMRect = {
  x: 0,
  y: 0,
  top: 0,
  left: 0,
  right: 0,
  bottom: 0,
  width: 0,
  height: 0,
  toJSON() {
    return {};
  },
};

const EMPTY_RECT_LIST: DOMRectList = {
  length: 0,
  item: () => null,
  [Symbol.iterator]: function* () {},
} as unknown as DOMRectList;

if (typeof Range !== "undefined" && !Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () => EMPTY_RECT_LIST;
  Range.prototype.getBoundingClientRect = () => EMPTY_RECT;
}

// jsdom 30 under vitest 4 can expose `window.localStorage` as a bare object with
// no Storage methods, so anything that persists through it fails with
// "localStorage.getItem is not a function". Install an in-memory Storage only
// when the environment's own one is unusable.

function createMemoryStorage(): Storage {
  const entries = new Map<string, string>();
  return {
    get length() {
      return entries.size;
    },
    key: (index: number) => Array.from(entries.keys())[index] ?? null,
    getItem: (key: string) => entries.get(String(key)) ?? null,
    setItem: (key: string, value: string) => {
      entries.set(String(key), String(value));
    },
    removeItem: (key: string) => {
      entries.delete(String(key));
    },
    clear: () => {
      entries.clear();
    },
  } as unknown as Storage;
}

const globalWithStorage = globalThis as typeof globalThis & {
  localStorage?: Storage;
};

if (typeof globalWithStorage.localStorage?.getItem !== "function") {
  Object.defineProperty(globalWithStorage, "localStorage", {
    value: createMemoryStorage(),
    configurable: true,
    writable: true,
  });
}
