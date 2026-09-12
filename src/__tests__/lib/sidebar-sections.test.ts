import { describe, it, expect } from "vitest";
import {
  SIDEBAR_SECTIONS,
  isSidebarSectionId,
  knownSidebarSections,
  withSidebarSection,
} from "../../lib/sidebar-sections";

describe("sidebar sections", () => {
  it("lists the four sections in the order the sidebar draws them", () => {
    expect(SIDEBAR_SECTIONS).toEqual(["folder", "tags", "inbox", "recent"]);
  });

  it("knows its own ids and nothing else", () => {
    expect(isSidebarSectionId("tags")).toBe(true);
    expect(isSidebarSectionId("open")).toBe(false);
    expect(isSidebarSectionId(3)).toBe(false);
    expect(isSidebarSectionId(null)).toBe(false);
  });

  describe("knownSidebarSections", () => {
    it("keeps known ids once each, in sidebar order", () => {
      expect(knownSidebarSections(["recent", "tags", "recent"])).toEqual(["tags", "recent"]);
    });

    it("drops an id it does not know and anything that is not a list", () => {
      expect(knownSidebarSections(["open", "tags", 7, null])).toEqual(["tags"]);
      expect(knownSidebarSections(undefined)).toEqual([]);
      expect(knownSidebarSections("tags")).toEqual([]);
    });
  });

  describe("withSidebarSection", () => {
    it("adds a section in sidebar order", () => {
      expect(withSidebarSection(["recent"], "folder", true)).toEqual(["folder", "recent"]);
    });

    it("removes a section", () => {
      expect(withSidebarSection(["tags", "recent"], "tags", false)).toEqual(["recent"]);
    });

    it("hands the same list back when nothing changes", () => {
      const list = ["tags"] as const;
      expect(withSidebarSection(list, "tags", true)).toBe(list);
      expect(withSidebarSection(list, "inbox", false)).toBe(list);
    });
  });
});
