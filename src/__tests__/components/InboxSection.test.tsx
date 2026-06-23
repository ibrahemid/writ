import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [path, setPath] = createSignal<string | null>(null);
  const [files, setFiles] = createSignal<
    { name: string; path: string; size_bytes: number }[]
  >([]);
  return { path, setPath, files, setFiles, openFile: vi.fn(), stopWatching: vi.fn() };
});

vi.mock("../../stores/global/inbox", () => ({
  inboxStore: { path: h.path, files: h.files, stopWatching: h.stopWatching },
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ tabs: { openFile: h.openFile } }),
}));

import InboxSection from "../../components/Sidebar/InboxSection";

afterEach(() => {
  h.setPath(null);
  h.setFiles([]);
  h.openFile.mockReset();
  h.stopWatching.mockReset();
  cleanup();
});

describe("InboxSection", () => {
  it("renders nothing when no folder is watched", () => {
    const { container } = render(() => <InboxSection />);
    expect(container.querySelector(".inbox-section")).toBeNull();
  });

  it("shows the folder name and the empty state when watched but no files", () => {
    h.setPath("/Users/me/Downloads/inbox");
    const { container } = render(() => <InboxSection />);
    expect(container.querySelector(".sidebar-section-title")!.textContent).toContain(
      "Inbox · inbox",
    );
    expect(container.querySelector(".inbox-empty")!.textContent).toBe("No files yet");
  });

  it("lists files with formatted sizes", () => {
    h.setPath("/inbox");
    h.setFiles([
      { name: "report.md", path: "/inbox/report.md", size_bytes: 1536 },
      { name: "notes.txt", path: "/inbox/notes.txt", size_bytes: 200 },
    ]);
    const { container } = render(() => <InboxSection />);
    const items = Array.from(container.querySelectorAll(".inbox-item"));
    expect(items).toHaveLength(2);
    expect(items[0].textContent).toContain("report.md");
    expect(items[0].textContent).toContain("1.5 KB");
    expect(items[1].textContent).toContain("200 B");
  });

  it("opens a file on click", () => {
    h.setPath("/inbox");
    h.setFiles([{ name: "a.md", path: "/inbox/a.md", size_bytes: 1 }]);
    const { container } = render(() => <InboxSection />);
    fireEvent.click(container.querySelector(".inbox-item")!);
    expect(h.openFile).toHaveBeenCalledWith("/inbox/a.md");
  });

  it("stops watching from the header action", () => {
    h.setPath("/inbox");
    const { container } = render(() => <InboxSection />);
    fireEvent.click(container.querySelector(".inbox-section-action")!);
    expect(h.stopWatching).toHaveBeenCalled();
  });
});
