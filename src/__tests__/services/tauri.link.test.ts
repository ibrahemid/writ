import { describe, it, expect, vi, beforeEach } from "vitest";

// The real IPC layer is driven end to end by standing in for the injected
// host bridge, so the test covers the argument names the Rust command
// actually receives.
const hostInvoke = vi.fn();
(globalThis as unknown as { window: Record<string, unknown> }).window.__TAURI_INTERNALS__ = {
  invoke: (cmd: string, args: unknown) => hostInvoke(cmd, args),
  transformCallback: (cb: unknown) => cb,
};

import { openExternalUrl, classifyExternalUrl } from "../../services/tauri";

// Normalization and the scheme allowlist live in Rust. If the service trimmed,
// lower-cased, or pre-filtered the string, the UI would become a second policy
// with its own bugs, and the two could disagree about what the user clicked.
describe("link service", () => {
  beforeEach(() => {
    hostInvoke.mockReset();
    hostInvoke.mockResolvedValue(undefined);
  });

  it("passes the raw string to open_external_url untouched", async () => {
    for (const raw of [
      "https://example.com",
      "  https://example.com  ",
      "JavaScript:alert(1)",
      "java\nscript:alert(1)",
      "mailto:a@example.com?subject=Hi There",
      "https://example.com/a%0Ab",
    ]) {
      hostInvoke.mockClear();
      await openExternalUrl(raw);
      expect(hostInvoke).toHaveBeenCalledWith("open_external_url", { url: raw });
    }
  });

  it("passes the raw string to classify_external_url untouched", async () => {
    hostInvoke.mockResolvedValue({ allowed: false, url: null, reason: "scheme", message: "no" });
    await classifyExternalUrl("  file:///etc/passwd ");
    expect(hostInvoke).toHaveBeenCalledWith("classify_external_url", {
      url: "  file:///etc/passwd ",
    });
  });

  it("returns the verdict as Rust sent it", async () => {
    const verdict = {
      allowed: true,
      url: "https://example.com/",
      reason: null,
      message: null,
    };
    hostInvoke.mockResolvedValue(verdict);
    await expect(classifyExternalUrl("https://example.com")).resolves.toEqual(verdict);
  });

  it("lets a refusal reject so the caller can surface it", async () => {
    hostInvoke.mockRejectedValue("Writ opens http, https, and mailto links only.");
    await expect(openExternalUrl("file:///etc/passwd")).rejects.toBe(
      "Writ opens http, https, and mailto links only.",
    );
  });
});
