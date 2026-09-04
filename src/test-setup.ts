import { configure } from "@solidjs/testing-library";

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

export function createMemoryStorage(): Storage {
  const entries = new Map<string, string>();
  const api = {
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
  };

  // A real Storage also exposes every stored key as a property, so
  // `store.k = "v"`, `store.k`, `"k" in store` and `delete store.k` behave like
  // the setItem/getItem/removeItem calls. The Proxy keeps that half of the
  // contract; the named methods above always win over a stored key.
  return new Proxy(api, {
    get(target, prop, receiver) {
      if (typeof prop === "string" && !(prop in target)) {
        return entries.get(prop);
      }
      return Reflect.get(target, prop, receiver);
    },
    set(target, prop, value, receiver) {
      if (typeof prop === "string" && !(prop in target)) {
        entries.set(prop, String(value));
        return true;
      }
      return Reflect.set(target, prop, value, receiver);
    },
    has(target, prop) {
      return (typeof prop === "string" && entries.has(prop)) || Reflect.has(target, prop);
    },
    deleteProperty(target, prop) {
      if (typeof prop === "string" && entries.has(prop)) {
        entries.delete(prop);
        return true;
      }
      return Reflect.deleteProperty(target, prop);
    },
    ownKeys() {
      return Array.from(entries.keys());
    },
    getOwnPropertyDescriptor(target, prop) {
      if (typeof prop === "string" && entries.has(prop)) {
        return {
          value: entries.get(prop),
          writable: true,
          enumerable: true,
          configurable: true,
        };
      }
      return Reflect.getOwnPropertyDescriptor(target, prop);
    },
  }) as unknown as Storage;
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

// `waitFor` keeps its own budget, which is one second by default and has
// nothing to do with the test timeout. A machine running several suites at
// once takes longer than that to mount an editor and land a digest, and the
// failure it produces is a passing test that failed, which is worse than a
// slow one. The budget matches the mount project's testTimeout so a test that
// is genuinely stuck still fails, on the timeout that means that.
configure({ asyncUtilTimeout: 15_000 });
