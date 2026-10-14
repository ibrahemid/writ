import { describe, it, expect, vi, afterEach } from "vitest";
import { render, fireEvent, cleanup } from "@solidjs/testing-library";
import type { SpellingEntry } from "../../editor/spelling";

const h = vi.hoisted(() => ({
  entries: [] as SpellingEntry[],
  reveal: vi.fn(),
  applyEntries: vi.fn(() => 0),
  showToast: vi.fn(),
}));

vi.mock("../../stores/global/spelling", () => ({
  spellingStore: {
    entries: () => h.entries,
    reveal: h.reveal,
    applyEntries: h.applyEntries,
    ignoreWord: vi.fn(async () => {}),
  },
  entryKey: (entry: SpellingEntry) => `${entry.from}:${entry.to}:${entry.word}`,
}));
vi.mock("../../stores/global/notifications", () => ({ showToast: h.showToast }));

import SpellingPreview, {
  openSpellingPreview,
  closeSpellingPreview,
} from "../../components/Editor/SpellingPreview";

function entry(word: string, fix: string, from: number): SpellingEntry {
  return {
    from,
    to: from + word.length,
    word,
    kind: "Spelling",
    message: "",
    suggestions: [fix],
    confident: true,
  } as SpellingEntry;
}

afterEach(() => {
  closeSpellingPreview();
  h.entries = [];
  vi.clearAllMocks();
  cleanup();
});

describe("SpellingPreview rows", () => {
  it("shows each word in the file from a button the keyboard reaches", () => {
    h.entries = [entry("teh", "the", 0), entry("recieve", "receive", 10)];
    render(() => <SpellingPreview />);
    openSpellingPreview();

    const reveals = [...document.querySelectorAll<HTMLButtonElement>(".spelling-preview-reveal")];
    expect(reveals.map((el) => el.tagName)).toEqual(["BUTTON", "BUTTON"]);
    expect(reveals[1].textContent).toContain("recieve");
    expect(reveals[1].textContent).toContain("receive");

    fireEvent.click(reveals[1]);
    expect(h.reveal).toHaveBeenCalledWith(h.entries[1]);
  });

  it("says how many words a fix changed", () => {
    h.entries = [entry("teh", "the", 0), entry("recieve", "receive", 10)];
    h.applyEntries.mockReturnValue(2);
    render(() => <SpellingPreview />);
    openSpellingPreview();

    const apply = [...document.querySelectorAll("button")].find((el) => el.textContent === "Apply")!;
    fireEvent.click(apply);

    expect(h.applyEntries).toHaveBeenCalledTimes(1);
    expect(h.showToast).toHaveBeenCalledWith("Fixed 2 words", "info");
  });

  it("says nothing when no word changed", () => {
    h.entries = [entry("teh", "the", 0)];
    h.applyEntries.mockReturnValue(0);
    render(() => <SpellingPreview />);
    openSpellingPreview();

    const apply = [...document.querySelectorAll("button")].find((el) => el.textContent === "Apply")!;
    fireEvent.click(apply);
    expect(h.showToast).not.toHaveBeenCalled();
  });
});
