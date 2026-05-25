import { describe, it, expect, vi, afterEach, beforeEach, beforeAll } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

const h = vi.hoisted(() => ({
  usage: {} as Record<string, number>,
  recordCommandUse: vi.fn(),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ commands: { usage: h.usage } }),
    recordCommandUse: h.recordCommandUse,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { focusEditor: vi.fn() },
  }),
}));

import CommandPalette, {
  openCommandPalette,
  closeCommandPalette,
} from "../../components/CommandPalette/CommandPalette";
import { registerCommand, getAllCommands, unregisterCommand } from "../../commands/registry";

function resetDom() {
  while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
}

function mountInAppShell() {
  resetDom();
  const sibling = document.createElement("div");
  sibling.id = "outside-peer";
  const peerBtn = document.createElement("button");
  peerBtn.textContent = "outside";
  sibling.appendChild(peerBtn);
  document.body.appendChild(sibling);

  const appRoot = document.createElement("div");
  appRoot.id = "app";
  document.body.appendChild(appRoot);
  const container = document.createElement("div");
  container.className = "app-container";
  appRoot.appendChild(container);
  return container;
}

describe("CommandPalette keyboard integration", () => {
  beforeEach(() => {
    for (const cmd of [...getAllCommands()]) unregisterCommand(cmd.id);
    h.recordCommandUse.mockClear();
    registerCommand({
      id: "cmd.alpha",
      label: "Alpha command",
      scope: "app",
      execute: vi.fn(),
    });
    registerCommand({
      id: "cmd.bravo",
      label: "Bravo command",
      scope: "app",
      execute: vi.fn(),
    });
    registerCommand({
      id: "cmd.charlie",
      label: "Charlie command",
      scope: "app",
      execute: vi.fn(),
    });
  });

  afterEach(() => {
    closeCommandPalette();
    cleanup();
    resetDom();
  });

  async function setup() {
    const container = mountInAppShell();
    const user = userEvent.setup({ document });
    render(() => <CommandPalette />, { container });
    openCommandPalette();
    await new Promise<void>((r) => setTimeout(r, 0));
    return user;
  }

  it("focuses the input on open", async () => {
    await setup();
    const input = document.querySelector<HTMLInputElement>(".palette-input")!;
    expect(document.activeElement).toBe(input);
  });

  it("does not mark the app root as inert (regression for nested ancestor inert)", async () => {
    await setup();
    const appRoot = document.getElementById("app")!;
    expect(appRoot.hasAttribute("inert")).toBe(false);
    const outside = document.getElementById("outside-peer")!;
    expect(outside.hasAttribute("inert")).toBe(true);
  });

  it("typing filters results", async () => {
    const user = await setup();
    await user.keyboard("Bravo");
    const items = document.querySelectorAll(".palette-item");
    expect(items.length).toBeGreaterThan(0);
    const labels = Array.from(items).map((el) => el.textContent ?? "");
    expect(labels.some((l) => l.includes("Bravo"))).toBe(true);
    expect(labels.some((l) => l.includes("Alpha"))).toBe(false);
  });

  it("ArrowDown moves selection forward", async () => {
    const user = await setup();
    await user.keyboard("{ArrowDown}");
    const selected = document.querySelector(".palette-item.is-selected");
    expect(selected?.textContent ?? "").toContain("Bravo");
  });

  it("ArrowUp from top stays at top", async () => {
    const user = await setup();
    await user.keyboard("{ArrowUp}");
    const selected = document.querySelector(".palette-item.is-selected");
    expect(selected?.textContent ?? "").toContain("Alpha");
  });

  it("Enter executes the highlighted command and closes", async () => {
    const user = await setup();
    await user.keyboard("{ArrowDown}{Enter}");
    expect(h.recordCommandUse).toHaveBeenCalledWith("cmd.bravo");
    expect(document.querySelector(".palette")).toBeNull();
  });

  it("Escape closes the palette", async () => {
    const user = await setup();
    await user.keyboard("{Escape}");
    expect(document.querySelector(".palette")).toBeNull();
  });

  it("the input keeps focus during type-to-filter", async () => {
    const user = await setup();
    const input = document.querySelector<HTMLInputElement>(".palette-input")!;
    await user.keyboard("br");
    expect(document.activeElement).toBe(input);
    expect(input.value).toBe("br");
  });
});
