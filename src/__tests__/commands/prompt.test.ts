import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  applyTransform: vi.fn(),
  promptScanPlaceholders: vi.fn(),
  promptFillPlaceholders: vi.fn(),
}));

vi.mock("../../services/clipboard", () => ({
  writeClipboardText: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

vi.mock("../../components/PromptFill/PromptFillModal", () => ({
  requestPlaceholderFill: vi.fn(),
}));

const editorMock = vi.hoisted(() => ({
  getActiveText: vi.fn(),
  applyEditToActiveBuffer: vi.fn(),
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({ editor: editorMock }),
  },
}));

import { registerPromptCommands } from "../../commands/prompt";
import { getCommand } from "../../commands/registry";
import * as tauriApi from "../../services/tauri";
import * as clipboard from "../../services/clipboard";
import { showToast } from "../../components/Notifications/Toast";
import { requestPlaceholderFill } from "../../components/PromptFill/PromptFillModal";

const mockedApi = vi.mocked(tauriApi);
const mockedClipboard = vi.mocked(clipboard);
const mockedToast = vi.mocked(showToast);
const mockedFill = vi.mocked(requestPlaceholderFill);

async function flush() {
  await new Promise<void>((r) => setTimeout(r, 0));
}

describe("registerPromptCommands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    registerPromptCommands();
  });

  it("registers both palette commands", () => {
    expect(getCommand("prompt.copyAsPrompt")).toBeDefined();
    expect(getCommand("prompt.fillPlaceholders")).toBeDefined();
  });

  describe("prompt.copyAsPrompt", () => {
    it("strips via the prepare_prompt transform and copies without mutating the buffer", async () => {
      editorMock.getActiveText.mockReturnValue({ text: "raw text", usedSelection: false });
      mockedApi.applyTransform.mockResolvedValue("stripped text\n");

      getCommand("prompt.copyAsPrompt")!.execute();
      await flush();

      expect(mockedApi.applyTransform).toHaveBeenCalledWith("prepare_prompt", "raw text");
      expect(mockedClipboard.writeClipboardText).toHaveBeenCalledWith("stripped text\n");
      expect(editorMock.applyEditToActiveBuffer).not.toHaveBeenCalled();
      expect(mockedToast).toHaveBeenCalledWith("Copied as prompt", "success");
    });

    it("uses the selection when one is present", async () => {
      editorMock.getActiveText.mockReturnValue({ text: "selected", usedSelection: true });
      mockedApi.applyTransform.mockResolvedValue("selected\n");

      getCommand("prompt.copyAsPrompt")!.execute();
      await flush();

      expect(editorMock.getActiveText).toHaveBeenCalledWith(true);
      expect(mockedApi.applyTransform).toHaveBeenCalledWith("prepare_prompt", "selected");
    });

    it("shows an error toast when the transform fails", async () => {
      editorMock.getActiveText.mockReturnValue({ text: "raw", usedSelection: false });
      mockedApi.applyTransform.mockRejectedValue(new Error("boom"));

      getCommand("prompt.copyAsPrompt")!.execute();
      await flush();

      expect(mockedClipboard.writeClipboardText).not.toHaveBeenCalled();
      expect(mockedToast).toHaveBeenCalledWith("Copy as prompt failed", "error");
    });

    it("does nothing when there is no active view", async () => {
      editorMock.getActiveText.mockReturnValue(null);

      getCommand("prompt.copyAsPrompt")!.execute();
      await flush();

      expect(mockedApi.applyTransform).not.toHaveBeenCalled();
      expect(mockedClipboard.writeClipboardText).not.toHaveBeenCalled();
    });
  });

  describe("prompt.fillPlaceholders", () => {
    it("scans, collects values, fills, and copies without mutating the buffer", async () => {
      editorMock.getActiveText.mockReturnValue({ text: "hi {{name}}", usedSelection: false });
      mockedApi.promptScanPlaceholders.mockResolvedValue(["name"]);
      mockedFill.mockResolvedValue({ name: "Writ" });
      mockedApi.promptFillPlaceholders.mockResolvedValue("hi Writ");

      getCommand("prompt.fillPlaceholders")!.execute();
      await flush();

      expect(mockedApi.promptScanPlaceholders).toHaveBeenCalledWith("hi {{name}}");
      expect(mockedFill).toHaveBeenCalledWith(["name"]);
      expect(mockedApi.promptFillPlaceholders).toHaveBeenCalledWith("hi {{name}}", { name: "Writ" });
      expect(mockedClipboard.writeClipboardText).toHaveBeenCalledWith("hi Writ");
      expect(editorMock.applyEditToActiveBuffer).not.toHaveBeenCalled();
      expect(mockedToast).toHaveBeenCalledWith("Filled prompt copied", "success");
    });

    it("does not fill or copy when the modal is cancelled", async () => {
      editorMock.getActiveText.mockReturnValue({ text: "hi {{name}}", usedSelection: false });
      mockedApi.promptScanPlaceholders.mockResolvedValue(["name"]);
      mockedFill.mockResolvedValue(null);

      getCommand("prompt.fillPlaceholders")!.execute();
      await flush();

      expect(mockedApi.promptFillPlaceholders).not.toHaveBeenCalled();
      expect(mockedClipboard.writeClipboardText).not.toHaveBeenCalled();
    });

    it("shows an error toast when filling fails", async () => {
      editorMock.getActiveText.mockReturnValue({ text: "{{a}}", usedSelection: false });
      mockedApi.promptScanPlaceholders.mockResolvedValue(["a"]);
      mockedFill.mockResolvedValue({ a: "1" });
      mockedApi.promptFillPlaceholders.mockRejectedValue(new Error("boom"));

      getCommand("prompt.fillPlaceholders")!.execute();
      await flush();

      expect(mockedClipboard.writeClipboardText).not.toHaveBeenCalled();
      expect(mockedToast).toHaveBeenCalledWith("Fill placeholders failed", "error");
    });
  });
});
