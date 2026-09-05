import { describe, it, expect, afterEach, vi } from "vitest";
import { render, fireEvent, cleanup, screen } from "@solidjs/testing-library";
import LinkAmbiguityPicker, {
  hideLinkPicker,
  showLinkCandidates,
  showMissingNote,
} from "../../components/Editor/LinkAmbiguityPicker";

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

    fireEvent.keyDown(document, { key: "Escape" });
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
});
