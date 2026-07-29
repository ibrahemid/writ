import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import type { BufferDocument } from "../../types/buffer";
import type { LinkVerdict } from "../../types/link";

const mocks = vi.hoisted(() => ({
  forceRender: vi.fn().mockResolvedValue({
    kind: "rendered" as const,
    used_fallback_stylesheet: true,
    parser_warnings: [],
  }),
  classify: vi.fn(),
  openExternalUrl: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/tauri", () => ({
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewForceRender: mocks.forceRender,
  previewRender: mocks.forceRender,
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
  classifyExternalUrl: mocks.classify,
  openExternalUrl: mocks.openExternalUrl,
}));

vi.mock("../../components/Editor/EditorInstance", async () => {
  const { createEffect, onCleanup } = await import("solid-js");
  const { useWindow } = await import("../../components/WindowProvider/WindowProvider");
  return {
    default: (props: { buffer: { id: string } }) => {
      const win = useWindow();
      createEffect(() => win.editor.setCurrentBufferId(props.buffer.id));
      onCleanup(() => win.editor.setCurrentBufferId(null));
      return <div data-testid="editor-stub" />;
    },
  };
});

import PreviewLayout from "../../components/Preview/PreviewLayout";
import { hostLabel } from "../../components/Preview/LinkConfirm";

function allowed(url: string): LinkVerdict {
  return { allowed: true, url, reason: null, message: null };
}

function refused(reason: string, message: string): LinkVerdict {
  return { allowed: false, url: null, reason, message };
}

function htmlBuffer(): BufferDocument {
  return {
    id: "H1",
    title: "page.html",
    filename: "page.html",
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
  };
}

async function mountPreview() {
  const view = render(() => (
    <WindowProvider windowId={7401}>
      <PreviewLayout buffer={htmlBuffer()} />
    </WindowProvider>
  ));
  const frame = await waitFor(() => {
    const el = view.container.querySelector<HTMLIFrameElement>("iframe.preview-frame");
    expect(el).not.toBeNull();
    return el!;
  });
  return { ...view, frame };
}

function sendLinkOpen(
  frame: HTMLIFrameElement,
  href: string,
  source: Window | null = frame.contentWindow,
) {
  window.dispatchEvent(
    new MessageEvent("message", {
      data: { source: "writ-preview", dir: "up", type: "link:open", href, x: 40, y: 24 },
      source: source as MessageEventSource | null,
    }),
  );
}

function popovers(container: HTMLElement) {
  return container.querySelectorAll(".link-confirm");
}

function openButton(container: HTMLElement): HTMLButtonElement | null {
  return container.querySelector<HTMLButtonElement>(".link-confirm-open");
}

describe("preview link confirmation", () => {
  beforeEach(() => {
    mocks.classify.mockReset();
    mocks.classify.mockImplementation(async (url: string) => allowed(url));
    mocks.openExternalUrl.mockClear();
    rendererRegistry.setFromIpc([
      {
        content_type: "html",
        capabilities: {
          supports_live_render: true,
          supports_print: true,
          max_safe_document_bytes: 50 * 1024 * 1024,
        },
      },
    ]);
  });

  afterEach(() => {
    cleanup();
    rendererRegistry.setFromIpc([]);
  });

  it("shows the destination and opens nothing on the message alone", async () => {
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "https://example.com/docs");

    await waitFor(() => expect(popovers(container)).toHaveLength(1));
    await waitFor(() =>
      expect(container.querySelector(".link-confirm-host")?.textContent).toBe("example.com"),
    );
    expect(container.querySelector(".link-confirm-url")?.textContent).toBe(
      "https://example.com/docs",
    );
    expect(mocks.openExternalUrl).not.toHaveBeenCalled();
  });

  it("names the host only from the classified destination", async () => {
    // Rust refuses a credentialed URL, so nothing in the popover may present
    // `evil.example` as an approved host.
    mocks.classify.mockResolvedValue(
      refused("credentials", "That link hides its real destination behind a username."),
    );
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "https://bank.example@evil.example/login");

    await waitFor(() => expect(container.querySelector(".link-confirm-refused")).not.toBeNull());
    expect(container.querySelector(".link-confirm-host")).toBeNull();
    expect(container.querySelector(".link-confirm-url")?.textContent).toBe(
      "https://bank.example@evil.example/login",
    );
  });

  it("shows the normalized destination Rust approved, not the raw href", async () => {
    mocks.classify.mockResolvedValue(allowed("https://example.com/"));
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "https://EXAMPLE.com");

    await waitFor(() =>
      expect(container.querySelector(".link-confirm-url")?.textContent).toBe(
        "https://example.com/",
      ),
    );
    expect(container.querySelector(".link-confirm-host")?.textContent).toBe("example.com");
  });

  it("opens once when the shell's own button is pressed", async () => {
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "https://example.com/docs");

    await waitFor(() => expect(openButton(container)?.disabled).toBe(false));
    openButton(container)!.click();

    expect(mocks.openExternalUrl).toHaveBeenCalledTimes(1);
    expect(mocks.openExternalUrl).toHaveBeenCalledWith("https://example.com/docs");
    await waitFor(() => expect(popovers(container)).toHaveLength(0));
  });

  it("renders a refused link inert with its reason", async () => {
    mocks.classify.mockResolvedValue(
      refused("scheme", "Writ opens only http, https, and mailto links."),
    );
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "javascript:alert(1)");

    await waitFor(() =>
      expect(container.querySelector(".link-confirm-refused")?.textContent).toBe(
        "Writ opens only http, https, and mailto links.",
      ),
    );
    expect(openButton(container)).toBeNull();
    expect(mocks.openExternalUrl).not.toHaveBeenCalled();
  });

  it("dismisses on Escape", async () => {
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "https://example.com/");
    await waitFor(() => expect(popovers(container)).toHaveLength(1));

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await waitFor(() => expect(popovers(container)).toHaveLength(0));
    expect(mocks.openExternalUrl).not.toHaveBeenCalled();
  });

  it("dismisses on a click outside it", async () => {
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "https://example.com/");
    await waitFor(() => expect(popovers(container)).toHaveLength(1));

    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await waitFor(() => expect(popovers(container)).toHaveLength(0));
  });

  it("replaces rather than stacks on a burst of messages", async () => {
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "https://one.example/");
    sendLinkOpen(frame, "https://two.example/");
    sendLinkOpen(frame, "https://three.example/");

    await waitFor(() =>
      expect(container.querySelector(".link-confirm-host")?.textContent).toBe("three.example"),
    );
    expect(popovers(container)).toHaveLength(1);
  });

  it("ignores a message that did not come from the preview frame", async () => {
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "https://example.com/", window);

    await new Promise((r) => setTimeout(r, 0));
    expect(popovers(container)).toHaveLength(0);
    expect(mocks.classify).not.toHaveBeenCalled();
  });
});

describe("hostLabel", () => {
  it("leads with the host so a path cannot pose as one", () => {
    expect(hostLabel("https://example.com/a/b")).toBe("example.com");
    expect(hostLabel("https://evil.example/https://bank.example/login")).toBe("evil.example");
    expect(hostLabel("https://example.com:8443/x")).toBe("example.com:8443");
  });

  it("shows the address for a mailto link", () => {
    expect(hostLabel("mailto:hi@example.com")).toBe("hi@example.com");
  });

  it("falls back to the raw string when there is nothing to parse", () => {
    expect(hostLabel("not a url")).toBe("not a url");
    expect(hostLabel("./notes.md")).toBe("./notes.md");
  });
});
