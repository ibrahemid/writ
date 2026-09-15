import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// The picker in the composer is bound to `ai.chat.model`, which is an override
// for chat alone: picking the connection's own model clears it rather than
// writing the same id twice.

const mocks = vi.hoisted(() => ({
  config: vi.fn(),
  save: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: { config: mocks.config, save: mocks.save },
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: mocks.showToast,
}));

import ModelPicker, { pickerOptions } from "../../components/Chat/ModelPicker";

function config(provider: string, model: string, chatModel: string) {
  return { ai: { provider, model, chat: { enabled: true, model: chatModel } } };
}

beforeEach(() => {
  mocks.save.mockReset().mockResolvedValue(undefined);
  mocks.showToast.mockReset();
  mocks.config.mockReset().mockReturnValue(config("ollama", "qwen3:8b", ""));
});

afterEach(() => {
  cleanup();
});

describe("the model picker", () => {
  it("lists the connection's own model first", () => {
    expect(pickerOptions("qwen3:8b", ["phi4", "qwen3:8b", "llama3"])).toEqual([
      "qwen3:8b",
      "phi4",
      "llama3",
    ]);
    expect(pickerOptions("", ["phi4"])).toEqual(["phi4"]);
  });

  it("labels the connection's model and shows it as chosen", () => {
    const { container } = render(() => <ModelPicker live={["qwen3:8b", "phi4"]} />);

    const select = container.querySelector(".chat-model") as HTMLSelectElement;
    const options = Array.from(select.options).map((o) => [o.value, o.textContent]);
    expect(options).toEqual([
      ["", "qwen3:8b (default)"],
      ["phi4", "phi4"],
    ]);
    expect(select.value).toBe("");
  });

  it("writes the id when another model is picked", async () => {
    const { container } = render(() => <ModelPicker live={["qwen3:8b", "phi4"]} />);
    const select = container.querySelector(".chat-model") as HTMLSelectElement;

    select.value = "phi4";
    fireEvent.change(select);

    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    expect(mocks.save.mock.calls[0][0].ai.chat.model).toBe("phi4");
  });

  it("clears the override when the connection's own model is picked", async () => {
    mocks.config.mockReturnValue(config("ollama", "qwen3:8b", "phi4"));
    const { container } = render(() => <ModelPicker live={["qwen3:8b", "phi4"]} />);
    const select = container.querySelector(".chat-model") as HTMLSelectElement;
    expect(select.value).toBe("phi4");

    select.value = "";
    fireEvent.change(select);

    await waitFor(() => expect(mocks.save).toHaveBeenCalledTimes(1));
    expect(mocks.save.mock.calls[0][0].ai.chat.model).toBe("");
  });

  it("falls back to the curated list when nothing was fetched", () => {
    const { container } = render(() => <ModelPicker live={[]} />);
    const select = container.querySelector(".chat-model") as HTMLSelectElement;
    expect(select.options.length).toBeGreaterThan(0);
    expect(select.options[0].textContent).toBe("qwen3:8b (default)");
  });
});
