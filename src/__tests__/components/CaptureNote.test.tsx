import { describe, it, expect, vi } from "vitest";
import { createRoot } from "solid-js";
import { render, fireEvent } from "@solidjs/testing-library";
import CaptureNote from "../../components/CaptureNote/CaptureNote";
import { createCaptureNoteStore, type CaptureCalls } from "../../stores/capture/capture-note";

function setup(overrides: Partial<CaptureCalls> = {}) {
  const fake = {
    append: vi.fn(async () => ({ name: "2026-10-15.txt", added: true })),
    target: vi.fn(async () => "2026-10-15.txt"),
    close: vi.fn(async () => {}),
    ...overrides,
  };
  const note = createRoot(() => createCaptureNoteStore(fake));
  const view = render(() => <CaptureNote note={note} />);
  const field = view.getByLabelText("Quick capture") as HTMLTextAreaElement;
  return { fake, note, view, field };
}

const settle = () => new Promise<void>((done) => setTimeout(done, 0));

describe("CaptureNote", () => {
  it("takes the keyboard and names the file the line goes to", async () => {
    const { field, view } = setup();
    await settle();
    expect(document.activeElement).toBe(field);
    expect(view.container.textContent).toContain("adds it to 2026-10-15.txt");
  });

  it("adds the line on Escape and gives the keyboard back", async () => {
    const { fake, field } = setup();
    fireEvent.input(field, { target: { value: "call the bank" } });
    fireEvent.keyDown(field, { key: "Escape" });
    await settle();
    expect(fake.append).toHaveBeenCalledWith("call the bank");
    expect(fake.close).toHaveBeenCalledWith(true);
  });

  it("leaves Escape to an input method that is still composing", async () => {
    const { fake, field } = setup();
    fireEvent.input(field, { target: { value: "にほん" } });
    fireEvent.keyDown(field, { key: "Escape", isComposing: true });
    await settle();
    expect(fake.append).not.toHaveBeenCalled();
    expect(fake.close).not.toHaveBeenCalled();
  });

  it("hides without writing when the window loses the keyboard", async () => {
    const { fake, field } = setup();
    fireEvent.input(field, { target: { value: "half a thought" } });
    window.dispatchEvent(new Event("blur"));
    await settle();
    expect(fake.append).not.toHaveBeenCalled();
    expect(fake.close).toHaveBeenCalledWith(false);
  });

  it("says why a line was not added, and keeps it", async () => {
    const { field, view } = setup({
      append: vi.fn(async () => {
        throw "ERR_FILE_NOT_DOWNLOADED: evicted";
      }),
    });
    fireEvent.input(field, { target: { value: "call the bank" } });
    fireEvent.keyDown(field, { key: "Escape" });
    await settle();
    expect(view.getByRole("alert").textContent).toContain("has not finished downloading");
    expect(field.value).toBe("call the bank");
  });
});
