import { describe, it, expect, afterEach } from "vitest";
import { createMemoryStorage } from "../test-setup";

describe("memory Storage stub", () => {
  it("round-trips through the named Storage methods", () => {
    const store = createMemoryStorage();
    expect(store.getItem("missing")).toBeNull();
    store.setItem("a", "1");
    store.setItem("b", "2");
    expect(store.getItem("a")).toBe("1");
    expect(store.length).toBe(2);
    expect(store.key(0)).toBe("a");
    expect(store.key(9)).toBeNull();
    store.removeItem("a");
    expect(store.getItem("a")).toBeNull();
    store.clear();
    expect(store.length).toBe(0);
  });

  it("coerces non-string keys and values the way Storage does", () => {
    const store = createMemoryStorage();
    store.setItem("n", 7 as unknown as string);
    expect(store.getItem("n")).toBe("7");
  });

  it("exposes stored keys as properties", () => {
    const store = createMemoryStorage();
    store.setItem("theme", "warp-dark");
    expect(store["theme"]).toBe("warp-dark");
    expect("theme" in store).toBe(true);

    store["accent"] = "pine";
    expect(store.getItem("accent")).toBe("pine");
    expect(store.length).toBe(2);

    delete store["accent"];
    expect(store.getItem("accent")).toBeNull();
    expect("accent" in store).toBe(false);
    expect(store["accent"]).toBeUndefined();
  });

  it("keeps the Storage methods reachable when a key shares their name", () => {
    const store = createMemoryStorage();
    store.setItem("clear", "not-a-method");
    expect(typeof store.clear).toBe("function");
    expect(store.getItem("clear")).toBe("not-a-method");
  });
});

// Passes on jsdom's own Storage and on the stub the setup file installs when
// jsdom hands the suite a localStorage without getItem.
describe("the environment's localStorage", () => {
  afterEach(() => localStorage.clear());

  it("supports both the method and the property form", () => {
    localStorage.setItem("writ-probe", "1");
    expect(localStorage.getItem("writ-probe")).toBe("1");
    expect(localStorage["writ-probe"]).toBe("1");
    expect("writ-probe" in localStorage).toBe(true);

    localStorage["writ-probe-2"] = "2";
    expect(localStorage.getItem("writ-probe-2")).toBe("2");

    delete localStorage["writ-probe-2"];
    expect(localStorage.getItem("writ-probe-2")).toBeNull();
  });
});
