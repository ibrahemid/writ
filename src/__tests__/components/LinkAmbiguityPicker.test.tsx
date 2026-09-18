import { describe, it, expect, afterEach, vi } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { render, fireEvent, cleanup, screen } from "@solidjs/testing-library";
import LinkAmbiguityPicker, {
  hideLinkPicker,
  showLinkCandidates,
  showMissingNote,
} from "../../components/Editor/LinkAmbiguityPicker";

const CSS = readFileSync(
  resolve(process.cwd(), "src/components/Editor/LinkAmbiguityPicker.css"),
  "utf8",
);

const rule = (selector: string): string => {
  const match = CSS.match(
    new RegExp(`${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`),
  );
  expect(match, `${selector} is declared`).toBeTruthy();
  return match![1]!;
};

afterEach(() => {
  hideLinkPicker();
  cleanup();
});

describe("LinkAmbiguityPicker", () => {
  it("lists every note the target could mean and opens the one picked", async () => {
    const onPick = vi.fn();
    render(() => <LinkAmbiguityPicker />);
    showLinkCandidates("Note", ["/notes/a/Note.md", "/notes/b/Note.md"], onPick);

    expect(await screen.findByText(/More than one note is called/)).toBeTruthy();
    const rows = document.querySelectorAll(".link-picker-row");
    expect(rows.length).toBe(2);
    expect(rows[0].textContent).toContain("Note.md");
    expect(rows[0].textContent).toContain("/notes/a");
    expect(rows[1].textContent).toContain("/notes/b");

    fireEvent.click(rows[1]);
    expect(onPick).toHaveBeenCalledWith("/notes/b/Note.md");
    expect(document.querySelector(".link-picker")).toBeNull();
  });

  it("offers to create the note when the target names none", async () => {
    const onCreate = vi.fn();
    render(() => <LinkAmbiguityPicker />);
    showMissingNote("New", onCreate);

    expect(await screen.findByText(/No note is called/)).toBeTruthy();
    const create = screen.getByText("Create note");
    fireEvent.click(create);
    expect(onCreate).toHaveBeenCalledTimes(1);
    expect(document.querySelector(".link-picker")).toBeNull();
  });

  it("closes on Escape without picking anything", async () => {
    const onPick = vi.fn();
    render(() => <LinkAmbiguityPicker />);
    showLinkCandidates("Note", ["/notes/a/Note.md"], onPick);
    await screen.findByText(/More than one note is called/);

    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(onPick).not.toHaveBeenCalled();
    expect(document.querySelector(".link-picker")).toBeNull();
  });

  it("takes focus when it appears", async () => {
    render(() => <LinkAmbiguityPicker />);
    showMissingNote("New", vi.fn());
    await screen.findByText("Create note");
    await Promise.resolve();
    expect(document.activeElement?.textContent).toContain("Create note");
  });

  it("keeps Tab inside the dialog it declares itself modal for", async () => {
    render(() => <LinkAmbiguityPicker />);
    showLinkCandidates("Note", ["/notes/a/Note.md", "/notes/b/Note.md"], vi.fn());
    await screen.findByText(/More than one note is called/);

    const panel = document.querySelector(".link-picker")!;
    for (let i = 0; i < 3; i += 1) {
      fireEvent.keyDown(document.activeElement ?? document.body, { key: "Tab" });
      expect(panel.contains(document.activeElement)).toBe(true);
    }
  });

  it("sits on the modal layer with the modal sheet's radius and shadow", () => {
    expect(rule(".link-picker-scrim")).toMatch(/z-index:\s*var\(--writ-z-modal\)/);
    expect(rule(".link-picker")).toMatch(/border-radius:\s*var\(--writ-r-modal\)/);
    expect(rule(".link-picker")).toMatch(/box-shadow:\s*var\(--writ-shadow-modal\)/);
  });

  it("selects a row with the neutral fill, and never cancels its focus ring", () => {
    expect(CSS).not.toMatch(/outline:\s*none/);
    expect(rule(".link-picker-row:hover")).toMatch(/background:\s*var\(--writ-bg-hover\)/);
    expect(rule(".link-picker-row:focus-visible")).toMatch(
      /background:\s*var\(--writ-bg-selected\)/,
    );
    expect(rule(".link-picker-row:focus-visible")).toMatch(/font-weight:\s*500/);
    expect(CSS).not.toMatch(/background:\s*var\(--writ-accent\)/);
  });
});
