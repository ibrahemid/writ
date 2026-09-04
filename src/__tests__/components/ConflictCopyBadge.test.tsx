import { describe, it, expect, afterEach, beforeEach, vi } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { ConflictCopyBadge } from "../../components/Sidebar/ConflictCopyBadge";

const HOVER_DELAY_MS = 500;

/** The tip is what the badge says beyond its two words, so it has to be hovered out. */
function tipTextAfterHover(container: HTMLElement): string {
  fireEvent.pointerEnter(container.firstElementChild!);
  vi.advanceTimersByTime(HOVER_DELAY_MS);
  return document.querySelector('[role="tooltip"]')!.textContent ?? "";
}

describe("ConflictCopyBadge", () => {
  beforeEach(() => vi.useFakeTimers());

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("renders nothing for an ordinary file", () => {
    const { container } = render(() => <ConflictCopyBadge kind={null} provider="iCloud Drive" />);
    expect(container.querySelector(".file-tree-copy-badge")).toBeNull();
  });

  it("marks a copy a sync service kept and names the service", () => {
    const { container } = render(() => (
      <ConflictCopyBadge kind="sync_client" provider="iCloud Drive" />
    ));
    const badge = container.querySelector(".file-tree-copy-badge")!;
    expect(badge.textContent).toBe("Sync copy");
    expect(tipTextAfterHover(container)).toContain("iCloud Drive");
  });

  it("marks the copy without a service name when the service is unknown", () => {
    const { container } = render(() => <ConflictCopyBadge kind="sync_client" provider={null} />);
    const badge = container.querySelector(".file-tree-copy-badge")!;
    expect(badge.textContent).toBe("Sync copy");
    expect(tipTextAfterHover(container)).toContain("Your sync service");
  });

  it("marks a copy Writ kept", () => {
    const { container } = render(() => <ConflictCopyBadge kind="writ" provider={null} />);
    const badge = container.querySelector(".file-tree-copy-badge")!;
    expect(badge.textContent).toBe("Writ copy");
    expect(tipTextAfterHover(container)).toContain("Writ kept this copy");
  });
});
