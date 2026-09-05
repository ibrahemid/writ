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
    reason: "download",
    message: null,
    ...overrides,
  };
}

describe("NoteDownloading", () => {
  afterEach(cleanup);

  it("names the provider fetching the file", () => {
    const { getByText } = render(() => (
      <NoteDownloading download={download()} onDismiss={() => {}} />
    ));

    expect(getByText("Downloading from iCloud Drive…")).toBeTruthy();
  });

  it("says only that it is downloading when the provider is unknown", () => {
    const { getByText } = render(() => (
      <NoteDownloading
        download={download({ provider: null })}
        onDismiss={() => {}}
      />
    ));

    expect(getByText("Downloading…")).toBeTruthy();
  });

  it("cancels the wait from the Cancel button", () => {
    const onDismissWhileDownloading = vi.fn();
    const { getByRole } = render(() => (
      <NoteDownloading download={download()} onDismiss={onDismissWhileDownloading} />
    ));

    fireEvent.click(getByRole("button", { name: "Cancel" }));
    expect(onDismissWhileDownloading).toHaveBeenCalledTimes(1);
  });

  it("shows a failure with what the provider said", () => {
    const { getByText, getByRole } = render(() => (
      <NoteDownloading
        download={download({ state: "failed", message: "iCloud Drive is signed out" })}
        onDismiss={() => {}}
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
        onDismiss={() => {}}
      />
    ));

    expect(getByText("This file could not be downloaded.")).toBeTruthy();
    expect(queryByText("iCloud Drive is signed out")).toBeNull();
  });

  it("says the note did not open when that is what went wrong", () => {
    const { getByText, queryByText } = render(() => (
      <NoteDownloading
        download={download({ state: "failed", reason: "open" })}
        onDismiss={() => {}}
      />
    ));

    expect(getByText("The file downloaded but the note did not open. Open it again.")).toBeTruthy();
    expect(queryByText("This file could not be downloaded.")).toBeNull();
  });

  it("says the download is no longer being followed when nothing is listening", () => {
    const { getByText } = render(() => (
      <NoteDownloading
        download={download({ state: "failed", reason: "listener" })}
        onDismiss={() => {}}
      />
    ));

    expect(getByText("Writ lost track of this download. Open the note again.")).toBeTruthy();
  });

  it("says what to do after the wait ran out", () => {
    const onDismissAfterItStopped = vi.fn();
    const { getByText, getByRole } = render(() => (
      <NoteDownloading
        download={download({ state: "timed_out" })}
        onDismiss={onDismissAfterItStopped}
      />
    ));

    expect(
      getByText("Still waiting for iCloud Drive. Try again once the file has downloaded."),
    ).toBeTruthy();

    fireEvent.click(getByRole("button", { name: "Close" }));
    expect(onDismissAfterItStopped).toHaveBeenCalledTimes(1);
  });

  it("names no provider in the timed-out state when there is none", () => {
    const { getByText } = render(() => (
      <NoteDownloading
        download={download({ state: "timed_out", provider: null })}
        onDismiss={() => {}}
      />
    ));

    expect(getByText("Still waiting. Try again once the file has downloaded.")).toBeTruthy();
  });
});
