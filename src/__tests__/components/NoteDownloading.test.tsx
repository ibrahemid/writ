import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import NoteDownloading from "../../components/Editor/NoteDownloading";
import type { PendingDownload } from "../../stores/window/download-store";

function download(overrides: Partial<PendingDownload> = {}): PendingDownload {
  return {
    path: "/home/user/Writ/away.md",
    title: "away.md",
    provider: "iCloud Drive",
    state: "downloading",
    message: null,
    ...overrides,
  };
}

describe("NoteDownloading", () => {
  afterEach(cleanup);

  it("names the provider fetching the file", () => {
    const { getByText } = render(() => (
      <NoteDownloading download={download()} onCancel={() => {}} onClose={() => {}} />
    ));

    expect(getByText("Downloading from iCloud Drive…")).toBeTruthy();
  });

  it("says only that it is downloading when the provider is unknown", () => {
    const { getByText } = render(() => (
      <NoteDownloading
        download={download({ provider: null })}
        onCancel={() => {}}
        onClose={() => {}}
      />
    ));

    expect(getByText("Downloading…")).toBeTruthy();
  });

  it("cancels the wait from the Cancel button", () => {
    const onCancel = vi.fn();
    const { getByRole } = render(() => (
      <NoteDownloading download={download()} onCancel={onCancel} onClose={() => {}} />
    ));

    fireEvent.click(getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("shows a failure with what the provider said", () => {
    const { getByText, getByRole } = render(() => (
      <NoteDownloading
        download={download({ state: "failed", message: "iCloud Drive is signed out" })}
        onCancel={() => {}}
        onClose={() => {}}
      />
    ));

    expect(getByText("This file could not be downloaded.")).toBeTruthy();
    expect(getByText("iCloud Drive is signed out")).toBeTruthy();
    expect(getByRole("button", { name: "Close" })).toBeTruthy();
  });

  it("shows a failure with no detail when the provider gave no reason", () => {
    const { getByText, queryByText } = render(() => (
      <NoteDownloading
        download={download({ state: "failed" })}
        onCancel={() => {}}
        onClose={() => {}}
      />
    ));

    expect(getByText("This file could not be downloaded.")).toBeTruthy();
    expect(queryByText("iCloud Drive is signed out")).toBeNull();
  });

  it("says what to do after the wait ran out", () => {
    const onClose = vi.fn();
    const { getByText, getByRole } = render(() => (
      <NoteDownloading
        download={download({ state: "timed_out" })}
        onCancel={() => {}}
        onClose={onClose}
      />
    ));

    expect(
      getByText("Still waiting for iCloud Drive. Try again once the file has downloaded."),
    ).toBeTruthy();

    fireEvent.click(getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("names no provider in the timed-out state when there is none", () => {
    const { getByText } = render(() => (
      <NoteDownloading
        download={download({ state: "timed_out", provider: null })}
        onCancel={() => {}}
        onClose={() => {}}
      />
    ));

    expect(getByText("Still waiting. Try again once the file has downloaded.")).toBeTruthy();
  });
});
