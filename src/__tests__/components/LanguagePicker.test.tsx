import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import type { BufferDocument } from "../../types/buffer";

// Change Language: the list opens over the file in front, a row records the
// language for that file, and Escape leaves it as it was.

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = vi.fn();
  }
});

const h = vi.hoisted(() => ({
  activeTabs: [] as BufferDocument[],
  setLanguage: vi.fn(),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    activeTabs: () => h.activeTabs,
    setLanguage: h.setLanguage,
  },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({ tabs: { activeTabId: () => "a" } }),
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: vi.fn() } }),
}));

import LanguagePicker, {
  closeLanguagePicker,
  openLanguagePicker,
} from "../../components/Editor/LanguagePicker";

function doc(language: string | null): BufferDocument {
  return {
    id: "a",
    title: "snippet",
    filename: "a.txt",
    status: "active",
    language,
    source_path: "/files/snippet.txt",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

beforeEach(() => {
  h.activeTabs = [doc(null)];
  h.setLanguage.mockReset();
  h.setLanguage.mockResolvedValue(undefined);
});

afterEach(() => {
  closeLanguagePicker();
  cleanup();
});

describe("LanguagePicker", () => {
  it("is closed until Change Language opens it", async () => {
    const { container } = render(() => <LanguagePicker />);
    expect(container.querySelector('[aria-label="Change language"]')).toBeNull();

    openLanguagePicker();

    await waitFor(() => {
      const input = container.querySelector<HTMLInputElement>('input[aria-label="Language name"]');
      expect(input).not.toBeNull();
      expect(input!.placeholder).toBe("Type a language name");
    });
  });

  it("records the language picked for the file in front and closes", async () => {
    const { container } = render(() => <LanguagePicker />);
    openLanguagePicker();
    const input = await waitFor(() => {
      const found = container.querySelector<HTMLInputElement>('input[aria-label="Language name"]');
      expect(found).not.toBeNull();
      return found!;
    });

    fireEvent.input(input, { target: { value: "python" } });
    await waitFor(() => expect(container.textContent).toContain("Python"));
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(h.setLanguage).toHaveBeenCalledWith("a", "python"));
    expect(container.querySelector('input[aria-label="Language name"]')).toBeNull();
  });

  it("leaves the file as it was on Escape", async () => {
    const { container } = render(() => <LanguagePicker />);
    openLanguagePicker();
    const input = await waitFor(() => {
      const found = container.querySelector<HTMLInputElement>('input[aria-label="Language name"]');
      expect(found).not.toBeNull();
      return found!;
    });

    fireEvent.keyDown(input, { key: "Escape" });

    await waitFor(() =>
      expect(container.querySelector('input[aria-label="Language name"]')).toBeNull(),
    );
    expect(h.setLanguage).not.toHaveBeenCalled();
  });
});
