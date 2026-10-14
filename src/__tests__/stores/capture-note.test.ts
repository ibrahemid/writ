import { describe, it, expect, vi } from "vitest";
import { createRoot } from "solid-js";
import {
  captureFailureMessage,
  createCaptureNoteStore,
  type CaptureCalls,
} from "../../stores/capture/capture-note";

function calls(overrides: Partial<CaptureCalls> = {}) {
  return {
    append: vi.fn(async () => ({ name: "2026-10-15.txt", added: true })),
    target: vi.fn(async () => "2026-10-15.txt"),
    close: vi.fn(async () => {}),
    ...overrides,
  };
}

function note(fake: CaptureCalls) {
  return createRoot(() => createCaptureNoteStore(fake));
}

describe("the capture note", () => {
  it("adds the text, empties the field, then gives the keyboard back", async () => {
    const order: string[] = [];
    const fake = calls({
      append: vi.fn(async (text: string) => {
        order.push(`append ${text}`);
        return { name: "2026-10-15.txt", added: true };
      }),
      close: vi.fn(async (returnFocus: boolean) => {
        order.push(`close ${returnFocus}`);
      }),
    });
    const capture = note(fake);
    capture.setText("buy milk");
    await capture.submit();
    expect(order).toEqual(["append buy milk", "close true"]);
    expect(capture.text()).toBe("");
  });

  it("writes nothing for a blank note and still gives the keyboard back", async () => {
    const fake = calls();
    const capture = note(fake);
    capture.setText("  \n ");
    await capture.submit();
    expect(fake.append).not.toHaveBeenCalled();
    expect(fake.close).toHaveBeenCalledWith(true);
  });

  it("keeps the window, the text and the reason when the entry is not written", async () => {
    const fake = calls({
      append: vi.fn(async () => {
        throw "ERR_READ_ONLY_DESTINATION: read-only";
      }),
    });
    const capture = note(fake);
    await capture.refreshTarget();
    capture.setText("buy milk");
    await capture.submit();
    expect(fake.close).not.toHaveBeenCalled();
    expect(capture.text()).toBe("buy milk");
    expect(capture.problem()).toBe(
      "Could not add this to 2026-10-15.txt: this file is read-only, so nothing was written.",
    );
  });

  it("hides on a click elsewhere without writing, and keeps the text", async () => {
    const fake = calls();
    const capture = note(fake);
    capture.setText("half a thought");
    await capture.dismiss();
    expect(fake.append).not.toHaveBeenCalled();
    expect(fake.close).toHaveBeenCalledWith(false);
    expect(capture.text()).toBe("half a thought");
  });

  it("does not hide while an entry is being written", async () => {
    let finish: () => void = () => {};
    const fake = calls({
      append: vi.fn(
        () =>
          new Promise<{ name: string; added: boolean }>((done) => {
            finish = () => done({ name: "2026-10-15.txt", added: true });
          }),
      ),
    });
    const capture = note(fake);
    capture.setText("buy milk");
    const pending = capture.submit();
    await capture.dismiss();
    expect(fake.close).not.toHaveBeenCalled();
    finish();
    await pending;
    expect(fake.close).toHaveBeenCalledTimes(1);
    expect(fake.close).toHaveBeenCalledWith(true);
  });
});

describe("what a note that was not written says", () => {
  it("names a file that kept changing and how to try again", () => {
    expect(captureFailureMessage("ERR_FILE_CHANGED_ON_DISK: changed", "2026-10-15.txt")).toBe(
      "2026-10-15.txt kept changing while this was being added. Press Escape to try again.",
    );
  });

  it("shows a sentence from the app as it is", () => {
    expect(captureFailureMessage("2026-10-15.txt is not a text file, so nothing was written.", "")).toBe(
      "2026-10-15.txt is not a text file, so nothing was written.",
    );
  });

  it("names the characters an encoding cannot hold", () => {
    expect(
      captureFailureMessage("ERR_ENCODING_LOSSY: windows-1252 has no bytes for U+4E2D", ""),
    ).toBe("Could not add this to today's file: this file is Windows-1252, which cannot hold “中”.");
  });
});
