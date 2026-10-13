import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  previewUrl,
  previewUrlPrefixLoaded,
  setPreviewUrlPrefix,
  loadPreviewUrlPrefix,
  BLANK_PREVIEW_SRC,
} from "../../stores/global/preview-url";

describe("previewUrl", () => {
  afterEach(() => {
    setPreviewUrlPrefix("writ-preview://");
  });

  it("builds the native scheme on macOS and Linux", () => {
    setPreviewUrlPrefix("writ-preview://");
    expect(previewUrlPrefixLoaded()).toBe(true);
    expect(previewUrl("document", "buf-1")).toBe("writ-preview://document/buf-1");
    expect(previewUrl("chrome", "blank")).toBe("writ-preview://chrome/blank");
  });

  it("builds the WebView2 workaround form on Windows", () => {
    // WebView2 serves a custom scheme as http://<scheme>.<host>, so document
    // and chrome become distinct http hosts.
    setPreviewUrlPrefix("http://writ-preview.");
    expect(previewUrl("document", "buf-1")).toBe("http://writ-preview.document/buf-1");
    expect(previewUrl("chrome", "blank")).toBe("http://writ-preview.chrome/blank");
    // A nested in-scope path (the note-asset route) keeps its segments.
    expect(previewUrl("document", "_note-asset/b/t/n/a.png")).toBe(
      "http://writ-preview.document/_note-asset/b/t/n/a.png",
    );
  });

  it("parks on the inert blank page until the base is known", () => {
    setPreviewUrlPrefix(null);
    expect(previewUrlPrefixLoaded()).toBe(false);
    expect(previewUrl("document", "buf-1")).toBe(BLANK_PREVIEW_SRC);
    expect(BLANK_PREVIEW_SRC).toBe("about:blank");
  });
});

describe("loadPreviewUrlPrefix", () => {
  let logSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    setPreviewUrlPrefix(null);
    logSpy = vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    logSpy.mockRestore();
    setPreviewUrlPrefix("writ-preview://");
  });

  it("takes the base the host gives", async () => {
    await loadPreviewUrlPrefix(() => Promise.resolve("http://writ-preview."));
    expect(previewUrl("document", "buf-1")).toBe("http://writ-preview.document/buf-1");
    expect(logSpy).not.toHaveBeenCalled();
  });

  it("stays parked and reports the failure when the host cannot answer", async () => {
    // A guessed scheme would be wrong on one platform or the other; on Windows
    // the native form is an external scheme to WebView2.
    await loadPreviewUrlPrefix(() => Promise.reject(new Error("ipc down")));
    expect(previewUrlPrefixLoaded()).toBe(false);
    expect(previewUrl("document", "buf-1")).toBe(BLANK_PREVIEW_SRC);
    expect(previewUrl("chrome", "blank")).toBe(BLANK_PREVIEW_SRC);
    expect(logSpy).toHaveBeenCalledTimes(1);
  });
});
