import { describe, it, expect, vi, afterEach, beforeAll } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

const h = vi.hoisted(() => ({
  focusEditor: vi.fn(),
  saveConfig: vi.fn().mockResolvedValue(undefined),
  setOverride: vi.fn(),
}));

vi.mock("../../stores/global/theme", () => ({
  themeStore: {
    toConfig: () => ({ preset: "default", overrides: {} }),
    loadConfig: vi.fn(),
    resolvedTokens: () => ({
      "surface.background": "#000",
      "foreground.default": "#fff",
    }),
    setOverride: h.setOverride,
    setPreset: vi.fn(),
    resetOverrides: vi.fn(),
    presetId: () => "default",
    presets: () => [{ id: "default", name: "Default" }],
    activePreset: () => ({
      surface: { background: "#000" },
      foreground: { default: "#fff" },
    }),
  },
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ theme: {} }),
    save: h.saveConfig,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { focusEditor: h.focusEditor },
  }),
}));

vi.mock("../../types/theme", () => ({
  TOKEN_GROUPS: ["surface", "foreground"],
}));

vi.mock("../Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import ThemeEditor, {
  openThemeEditor,
  closeThemeEditor,
} from "../../components/ThemeEditor/ThemeEditor";

function resetDom() {
  while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
}

function mountInAppShell() {
  resetDom();
  const editorBuf = document.createElement("button");
  editorBuf.id = "editor-stub";
  editorBuf.textContent = "editor";
  document.body.appendChild(editorBuf);

  const appRoot = document.createElement("div");
  appRoot.id = "app";
  document.body.appendChild(appRoot);
  return appRoot;
}

describe("ThemeEditor keyboard integration", () => {
  afterEach(() => {
    closeThemeEditor();
    cleanup();
    resetDom();
    h.focusEditor.mockClear();
  });

  async function setup() {
    const container = mountInAppShell();
    const user = userEvent.setup({ document });
    render(() => <ThemeEditor />, { container });
    openThemeEditor();
    await new Promise<void>((r) => setTimeout(r, 0));
    return user;
  }

  it("renders the dialog and traps inside #app without inerting it", async () => {
    await setup();
    const dialog = document.querySelector(".theme-editor");
    expect(dialog).not.toBeNull();
    expect(document.getElementById("app")!.hasAttribute("inert")).toBe(false);
    expect(document.getElementById("editor-stub")!.hasAttribute("inert")).toBe(true);
  });

  it("focuses an element inside the dialog on open", async () => {
    await setup();
    const dialog = document.querySelector<HTMLElement>(".theme-editor")!;
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("Tab cycles through controls inside the editor only", async () => {
    const user = await setup();
    const dialog = document.querySelector<HTMLElement>(".theme-editor")!;
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("Shift+Tab reverses direction and stays inside the dialog", async () => {
    const user = await setup();
    const dialog = document.querySelector<HTMLElement>(".theme-editor")!;
    await user.tab({ shift: true });
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("Escape closes the dialog and restores focus to the editor stub", async () => {
    const user = await setup();
    document.getElementById("editor-stub")!.focus();
    closeThemeEditor();
    openThemeEditor();
    await new Promise<void>((r) => setTimeout(r, 0));
    await user.keyboard("{Escape}");
    expect(document.querySelector(".theme-editor")).toBeNull();
    expect(document.activeElement?.id).toBe("editor-stub");
  });

  it("typing inside the color hex input does not get intercepted", async () => {
    const user = await setup();
    const colorInput = document.querySelector<HTMLInputElement>(".theme-editor-picker");
    if (!colorInput) return;
    colorInput.focus();
    await user.keyboard("a");
    expect(document.activeElement).toBe(colorInput);
  });
});
