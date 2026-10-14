import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import type { BufferDocument } from "../../types/buffer";

// Reopen with Encoding: the list opens over the file in front, a row reads the
// file again in that encoding, and a tab with unsaved text is left alone.

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = vi.fn();
  }
});

const h = vi.hoisted(() => ({
  activeTabs: [] as BufferDocument[],
  reopenWithEncoding: vi.fn(),
  requestExternalReload: vi.fn(),
  dirty: false,
  toast: vi.fn(),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => h.activeTabs,
    reopenWithEncoding: h.reopenWithEncoding,
  },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({
      tabs: { activeTabId: () => "a" },
      editor: {
        isDirty: () => h.dirty,
        requestExternalReload: h.requestExternalReload,
      },
    }),
  },
}));

vi.mock("../../components/Notifications/Toast", () => ({ showToast: h.toast }));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: vi.fn() } }),
}));

import EncodingPicker, {
  closeEncodingPicker,
  openEncodingPicker,
} from "../../components/Editor/EncodingPicker";

function doc(encoding: string | null): BufferDocument {
  return {
    id: "a",
    title: "prices",
    filename: "a.txt",
    status: "active",
    language: null,
    source_path: "/files/prices.csv",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "crlf",
    encoding,
  };
}

async function openList(container: HTMLElement): Promise<HTMLInputElement> {
  openEncodingPicker();
  return waitFor(() => {
    const found = container.querySelector<HTMLInputElement>('input[aria-label="Encoding name"]');
    expect(found).not.toBeNull();
    return found!;
  });
}

beforeEach(() => {
  h.activeTabs = [doc("windows-1252")];
  h.dirty = false;
  h.reopenWithEncoding.mockReset();
  h.reopenWithEncoding.mockResolvedValue(undefined);
  h.requestExternalReload.mockReset();
  h.toast.mockReset();
});

afterEach(() => {
  closeEncodingPicker();
  cleanup();
});

describe("EncodingPicker", () => {
  it("marks the encoding the file is in now", async () => {
    const { container } = render(() => <EncodingPicker />);
    const input = await openList(container);
    fireEvent.input(input, { target: { value: "1252" } });
    await waitFor(() => expect(container.textContent).toContain("Western (Windows-1252)"));
    expect(container.textContent).toContain("Current");
  });

  it("reopens the file in front in the encoding picked and reloads its text", async () => {
    const { container } = render(() => <EncodingPicker />);
    const input = await openList(container);

    fireEvent.input(input, { target: { value: "latin-2" } });
    await waitFor(() => expect(container.textContent).toContain("Central European (ISO-8859-2)"));
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(h.reopenWithEncoding).toHaveBeenCalledWith("a", "ISO-8859-2"));
    await waitFor(() => expect(h.requestExternalReload).toHaveBeenCalledWith("a"));
    expect(container.querySelector('input[aria-label="Encoding name"]')).toBeNull();
  });

  it("says why when the file is not text in the encoding picked", async () => {
    h.reopenWithEncoding.mockRejectedValueOnce("ERR_READ_NOT_UTF8: the file is not UTF-8 text");
    const { container } = render(() => <EncodingPicker />);
    const input = await openList(container);

    fireEvent.input(input, { target: { value: "utf-8" } });
    await waitFor(() => expect(container.textContent).toContain("UTF-8"));
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(h.toast).toHaveBeenCalled());
    expect(h.toast.mock.calls[0][0]).toBe(
      "Couldn't reopen prices.csv as UTF-8: it is not UTF-8 text. Choose another encoding in the status bar.",
    );
    expect(h.requestExternalReload).not.toHaveBeenCalled();
  });

  it("leaves a file with unsaved text as it is", async () => {
    h.dirty = true;
    const { container } = render(() => <EncodingPicker />);
    const input = await openList(container);

    fireEvent.input(input, { target: { value: "utf-8" } });
    await waitFor(() => expect(container.textContent).toContain("UTF-8"));
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(h.toast).toHaveBeenCalled());
    expect(h.toast.mock.calls[0][0]).toBe(
      "Save prices.csv before reopening it in another encoding",
    );
    expect(h.reopenWithEncoding).not.toHaveBeenCalled();
  });

  it("lists nothing for a note with no file yet", async () => {
    h.activeTabs = [{ ...doc("UTF-8"), source_path: null }];
    const { container } = render(() => <EncodingPicker />);
    const input = await openList(container);
    fireEvent.input(input, { target: { value: "utf" } });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(container.textContent).not.toContain("UTF-16 LE");
  });

  it("lists nothing for a file with no encoding", async () => {
    h.activeTabs = [doc(null)];
    const { container } = render(() => <EncodingPicker />);
    const input = await openList(container);
    fireEvent.input(input, { target: { value: "utf" } });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(container.textContent).not.toContain("UTF-16 LE");
  });
});
