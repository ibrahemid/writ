import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { Platform } from "../../lib/platform";

const h = vi.hoisted(() => ({ platform: "mac" as Platform }));

vi.mock("../../lib/platform", () => ({
  resolvePlatform: () => h.platform,
  detectPlatform: () => h.platform,
  IS_MAC: true,
}));

import TabItem from "../../components/Sidebar/TabItem";

afterEach(() => {
  h.platform = "mac";
  cleanup();
});

function renderRow(isActive: boolean) {
  return render(() => (
    <TabItem label="Meeting notes" icon="file-text" isActive={isActive} onClick={() => undefined} />
  ));
}

describe("TabItem platform overlay", () => {
  it("draws the Windows accent pill on the selected row", () => {
    h.platform = "win";
    const { container } = renderRow(true);
    expect(container.querySelector(".tab-item-pill")).not.toBeNull();
  });

  it("draws no pill on an unselected Windows row", () => {
    h.platform = "win";
    const { container } = renderRow(false);
    expect(container.querySelector(".tab-item-pill")).toBeNull();
  });

  it("draws no pill on Linux", () => {
    h.platform = "linux";
    const { container } = renderRow(true);
    expect(container.querySelector(".tab-item-pill")).toBeNull();
  });

  it("draws no pill on macOS", () => {
    const { container } = renderRow(true);
    expect(container.querySelector(".tab-item-pill")).toBeNull();
  });
});
