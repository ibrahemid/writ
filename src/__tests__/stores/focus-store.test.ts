import { describe, it, expect } from "vitest";
import { createRoot } from "solid-js";
import { createFocusStore } from "../../stores/window/focus-store";

describe("focusStore", () => {
  it("defaults the active region to the editor", () => {
    createRoot((dispose) => {
      const store = createFocusStore();
      expect(store.activeRegion()).toBe("editor");
      dispose();
    });
  });

  it("tracks the active region through setActiveRegion", () => {
    createRoot((dispose) => {
      const store = createFocusStore();

      store.setActiveRegion("sidebar");
      expect(store.activeRegion()).toBe("sidebar");

      store.setActiveRegion("statusbar");
      expect(store.activeRegion()).toBe("statusbar");

      dispose();
    });
  });
});
