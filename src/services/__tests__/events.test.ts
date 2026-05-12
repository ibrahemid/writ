import { describe, it, expect, vi, beforeEach } from "vitest";

const mockEmit = vi.fn();
const mockListen = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  emit: (...args: unknown[]) => mockEmit(...args),
  listen: (...args: unknown[]) => mockListen(...args),
}));

import { emitFrontendReady, onEvent } from "../events";

beforeEach(() => {
  mockEmit.mockReset().mockResolvedValue(undefined);
  mockListen.mockReset().mockResolvedValue(() => {});
});

describe("emitFrontendReady", () => {
  it("emits the frontend-ready signal exactly once", async () => {
    await emitFrontendReady();
    expect(mockEmit).toHaveBeenCalledOnce();
    expect(mockEmit).toHaveBeenCalledWith("frontend-ready");
  });

  it("rejects when the underlying emit rejects", async () => {
    mockEmit.mockRejectedValueOnce(new Error("ipc down"));
    await expect(emitFrontendReady()).rejects.toThrow("ipc down");
  });
});

describe("onEvent", () => {
  it("subscribes to the writ:// event channel matching the kind", async () => {
    await onEvent("pending:opens", () => {});
    expect(mockListen).toHaveBeenCalledOnce();
    expect(mockListen).toHaveBeenCalledWith("writ://pending-opens", expect.any(Function));
  });

  it("forwards the payload to the handler", async () => {
    let captured: { paths: string[] } | undefined;
    let registered: ((event: { payload: unknown }) => void) | undefined;
    mockListen.mockImplementationOnce(
      (_name: string, handler: (event: { payload: unknown }) => void) => {
        registered = handler;
        return Promise.resolve(() => {});
      },
    );

    await onEvent("pending:opens", (payload) => {
      captured = payload;
    });

    registered?.({ payload: { paths: ["/tmp/a.md", "/tmp/b.md"] } });

    expect(captured).toEqual({ paths: ["/tmp/a.md", "/tmp/b.md"] });
  });
});
