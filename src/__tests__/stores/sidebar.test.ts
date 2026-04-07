import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  searchBuffers: vi.fn().mockResolvedValue([]),
}));

import { sidebarStore } from "../../stores/sidebar";
import { searchBuffers } from "../../services/tauri";

const mockedSearch = vi.mocked(searchBuffers);

describe("sidebarStore", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    sidebarStore.hide();
    sidebarStore.setSearchQuery("");
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe("visibility", () => {
    it("starts hidden", () => {
      expect(sidebarStore.isVisible()).toBe(false);
    });

    it("show makes it visible", () => {
      sidebarStore.show();
      expect(sidebarStore.isVisible()).toBe(true);
    });

    it("hide makes it hidden", () => {
      sidebarStore.show();
      sidebarStore.hide();
      expect(sidebarStore.isVisible()).toBe(false);
    });

    it("toggle flips visibility", () => {
      sidebarStore.toggle();
      expect(sidebarStore.isVisible()).toBe(true);

      sidebarStore.toggle();
      expect(sidebarStore.isVisible()).toBe(false);
    });
  });

  describe("search", () => {
    it("updates search query", () => {
      sidebarStore.setSearchQuery("hello");
      expect(sidebarStore.searchQuery()).toBe("hello");
    });

    it("clears search results when query is empty", () => {
      sidebarStore.setSearchQuery("");
      expect(sidebarStore.searchResultIds()).toEqual([]);
      expect(mockedSearch).not.toHaveBeenCalled();
    });

    it("clears search results when query is whitespace", () => {
      sidebarStore.setSearchQuery("   ");
      expect(sidebarStore.searchResultIds()).toEqual([]);
      expect(mockedSearch).not.toHaveBeenCalled();
    });

    it("debounces search calls", async () => {
      mockedSearch.mockResolvedValueOnce(["id-1", "id-2"]);

      sidebarStore.setSearchQuery("test");

      expect(mockedSearch).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(200);

      expect(mockedSearch).toHaveBeenCalledOnce();
      expect(mockedSearch).toHaveBeenCalledWith("test");
      expect(sidebarStore.searchResultIds()).toEqual(["id-1", "id-2"]);
    });

    it("resets results on search failure", async () => {
      mockedSearch.mockRejectedValueOnce(new Error("db error"));

      sidebarStore.setSearchQuery("fail");
      await vi.advanceTimersByTimeAsync(200);

      expect(sidebarStore.searchResultIds()).toEqual([]);
    });

    it("cancels previous search when query changes rapidly", async () => {
      sidebarStore.setSearchQuery("fir");
      await vi.advanceTimersByTimeAsync(100);

      mockedSearch.mockResolvedValueOnce(["final-result"]);
      sidebarStore.setSearchQuery("final");
      await vi.advanceTimersByTimeAsync(200);

      expect(mockedSearch).toHaveBeenCalledOnce();
      expect(mockedSearch).toHaveBeenCalledWith("final");
    });
  });
});
