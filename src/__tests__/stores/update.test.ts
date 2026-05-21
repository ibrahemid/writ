import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  checkForUpdate: vi.fn().mockResolvedValue(undefined),
  downloadAndInstallUpdate: vi.fn().mockResolvedValue(undefined),
  dismissUpdate: vi.fn().mockResolvedValue(undefined),
  restartApp: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

import { updateStore } from "../../stores/update";
import * as api from "../../services/tauri";
import * as events from "../../services/events";

const mockedApi = vi.mocked(api);
const mockedEvents = vi.mocked(events);

describe("updateStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    updateStore.applyPhase({ status: "idle" });
  });

  it("mirrors the phase delivered by the event handler", () => {
    updateStore.applyPhase({ status: "available", version: "0.9.0" });
    expect(updateStore.phase()).toEqual({ status: "available", version: "0.9.0" });
  });

  it("tracks download progress as the phase updates", () => {
    updateStore.applyPhase({ status: "downloading", downloaded: 512, total: 2048 });
    expect(updateStore.phase()).toEqual({ status: "downloading", downloaded: 512, total: 2048 });
  });

  it("checkForUpdate sets checking optimistically and triggers the IPC", async () => {
    await updateStore.checkForUpdate();
    expect(mockedApi.checkForUpdate).toHaveBeenCalledOnce();
    expect(updateStore.phase()).toEqual({ status: "checking" });
  });

  it("checkForUpdate surfaces a graceful failure when the IPC rejects", async () => {
    mockedApi.checkForUpdate.mockRejectedValueOnce(new Error("ipc down"));
    await updateStore.checkForUpdate();
    expect(updateStore.phase()).toEqual({
      status: "failed",
      message: "Couldn't reach the update server.",
    });
  });

  it("install triggers download_and_install", async () => {
    await updateStore.install();
    expect(mockedApi.downloadAndInstallUpdate).toHaveBeenCalledOnce();
  });

  it("dismiss resets to idle and tells the backend", async () => {
    updateStore.applyPhase({ status: "available", version: "0.9.0" });
    await updateStore.dismiss();
    expect(updateStore.phase()).toEqual({ status: "idle" });
    expect(mockedApi.dismissUpdate).toHaveBeenCalledOnce();
  });

  it("restart triggers the relaunch IPC", async () => {
    await updateStore.restart();
    expect(mockedApi.restartApp).toHaveBeenCalledOnce();
  });

  it("subscribe registers an update:status listener", async () => {
    await updateStore.subscribe();
    expect(mockedEvents.onEvent).toHaveBeenCalledWith("update:status", expect.any(Function));
  });
});
