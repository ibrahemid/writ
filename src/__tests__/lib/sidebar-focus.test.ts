import { describe, expect, it } from "vitest";
import { focusAfterSidebarChange } from "../../lib/sidebar-focus";

describe("focusAfterSidebarChange", () => {
  it("leaves the caret where it is when the sidebar opens", () => {
    expect(focusAfterSidebarChange(true)).toBe("keep");
  });

  it("returns focus to the editor when the sidebar closes", () => {
    expect(focusAfterSidebarChange(false)).toBe("editor");
  });

  it("leaves focus on a control outside the sidebar when it closes, the toolbar's search field clearing", () => {
    expect(focusAfterSidebarChange(false, true)).toBe("keep");
  });
});
