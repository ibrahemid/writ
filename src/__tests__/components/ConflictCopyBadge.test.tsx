import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { ConflictCopyBadge } from "../../components/Sidebar/ConflictCopyBadge";

describe("ConflictCopyBadge", () => {
  afterEach(() => cleanup());

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
    expect(badge.getAttribute("title")).toContain("iCloud Drive");
  });

  it("marks the copy without a service name when the service is unknown", () => {
    const { container } = render(() => <ConflictCopyBadge kind="sync_client" provider={null} />);
    const badge = container.querySelector(".file-tree-copy-badge")!;
    expect(badge.textContent).toBe("Sync copy");
    expect(badge.getAttribute("title")).toContain("Your sync service");
  });

  it("marks a copy Writ kept", () => {
    const { container } = render(() => <ConflictCopyBadge kind="writ" provider={null} />);
    const badge = container.querySelector(".file-tree-copy-badge")!;
    expect(badge.textContent).toBe("Writ copy");
    expect(badge.getAttribute("title")).toContain("Writ kept this copy");
  });
});
