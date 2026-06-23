import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [count, setCount] = createSignal<number | null>(null);
  return { count, setCount, requestMock: vi.fn(), resetMock: vi.fn() };
});
const { setCount, requestMock, resetMock } = h;

vi.mock("../../stores/global/prompt-estimate", () => ({
  promptEstimateStore: { count: h.count, request: h.requestMock, reset: h.resetMock },
}));
vi.mock("../../stores/global/token-estimate", () => ({
  formatTokenCount: (n: number) => String(n),
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: vi.fn() } }),
}));

import PromptFillModal, {
  requestPlaceholderFill,
} from "../../components/PromptFill/PromptFillModal";

function mountShell() {
  while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
  const appRoot = document.createElement("div");
  appRoot.id = "app";
  document.body.appendChild(appRoot);
  return appRoot;
}

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

afterEach(() => {
  setCount(null);
  requestMock.mockReset();
  cleanup();
});

describe("PromptFillModal workbench meta", () => {
  it("shows the variable count and requests an estimate from the template", async () => {
    render(() => <PromptFillModal />, { container: mountShell() });
    const promise = requestPlaceholderFill(["name", "endpoint"], "Hi {{name}} at {{endpoint}}");
    await flush();

    const meta = document.querySelector(".placeholders-meta")!;
    expect(meta.textContent).toContain("2 variables");
    expect(requestMock).toHaveBeenCalledWith("Hi {{name}} at {{endpoint}}", {
      name: "",
      endpoint: "",
    });

    document.querySelector<HTMLButtonElement>(".placeholders-cancel")!.click();
    await promise;
  });

  it("singularizes a single variable", async () => {
    render(() => <PromptFillModal />, { container: mountShell() });
    const promise = requestPlaceholderFill(["only"], "{{only}}");
    await flush();
    expect(document.querySelector(".placeholders-meta")!.textContent).toContain("1 variable");
    expect(document.querySelector(".placeholders-meta")!.textContent).not.toContain("variables");
    document.querySelector<HTMLButtonElement>(".placeholders-cancel")!.click();
    await promise;
  });

  it("renders the token estimate once the store resolves a count", async () => {
    render(() => <PromptFillModal />, { container: mountShell() });
    const promise = requestPlaceholderFill(["name"], "Hi {{name}}");
    await flush();
    expect(document.querySelector(".placeholders-meta-tokens")).toBeNull();

    setCount(128);
    await flush();
    expect(document.querySelector(".placeholders-meta-tokens")!.textContent).toContain(
      "~128 tokens",
    );

    document.querySelector<HTMLButtonElement>(".placeholders-cancel")!.click();
    await promise;
  });

  it("resets the estimate when the modal settles", async () => {
    render(() => <PromptFillModal />, { container: mountShell() });
    const promise = requestPlaceholderFill(["name"], "Hi {{name}}");
    await flush();
    document.querySelector<HTMLButtonElement>(".placeholders-cancel")!.click();
    await promise;
    expect(resetMock).toHaveBeenCalled();
  });
});
