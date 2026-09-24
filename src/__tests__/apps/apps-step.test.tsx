import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { Show } from "solid-js";
import WindowProvider from "../../components/WindowProvider/WindowProvider";

// ADR-042 section 5: the Apps step after the format step. The store is the
// app's singleton and reads the launch once, so the tests run in order: the
// first launch, then the same screen opened again from Settings.

const mocks = vi.hoisted(() => ({
  firstRunState: vi.fn().mockResolvedValue({
    first_run: true,
    hint_dismissed: false,
    file_manager: "Finder",
  }),
  finishFirstRun: vi.fn().mockResolvedValue(null),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/tauri", () => ({
  firstRunState: mocks.firstRunState,
  finishFirstRun: mocks.finishFirstRun,
  updateConfig: mocks.updateConfig,
  dismissFirstRunHint: vi.fn().mockResolvedValue(undefined),
  autoRetitleNote: vi.fn().mockResolvedValue({ kind: "skipped" }),
  listActiveBuffers: vi.fn().mockResolvedValue([]),
  listHistory: vi.fn().mockResolvedValue([]),
  getBuffer: vi.fn(),
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn(),
}));

import FirstRunSetup from "../../components/FirstRun/FirstRunSetup";
import { firstRunStore } from "../../stores/global/first-run";
import { configStore } from "../../stores/global/config";
import { APPS } from "../../lib/apps";

function mount() {
  return render(() => (
    <WindowProvider windowId={8803}>
      <Show when={firstRunStore.step() !== null}>
        <FirstRunSetup />
      </Show>
    </WindowProvider>
  ));
}

function continueButton(container: Element): HTMLButtonElement {
  const button = Array.from(container.querySelectorAll("button")).find(
    (el) => el.textContent === "Continue",
  );
  if (!button) throw new Error("no Continue");
  return button;
}

function switchFor(container: Element, app: string): HTMLButtonElement {
  const el = container.querySelector<HTMLButtonElement>(`[data-app='${app}'] [role='switch']`);
  if (!el) throw new Error(`no switch for ${app}`);
  return el;
}

afterEach(() => cleanup());

describe("the Apps step", () => {
  it("follows the format step, which writes nothing on its way there", async () => {
    await firstRunStore.load();
    const { container, getByRole } = mount();
    expect(firstRunStore.step()).toBe("format");

    fireEvent.click(continueButton(container));

    expect(firstRunStore.step()).toBe("apps");
    expect(mocks.finishFirstRun).not.toHaveBeenCalled();
    expect(mocks.updateConfig).not.toHaveBeenCalled();
    expect(getByRole("dialog").getAttribute("aria-labelledby")).toBeTruthy();
    expect(getByRole("heading", { level: 1 }).textContent).toBe("Apps");
  });

  it("lists the six apps with their sentence, every switch off, and asks nothing else", () => {
    const { container, getByRole } = mount();
    const rows = Array.from(container.querySelectorAll<HTMLElement>("[data-app]"));
    expect(rows.map((row) => row.dataset.app)).toEqual(APPS.map((app) => app.id));
    for (const app of APPS) {
      const row = container.querySelector(`[data-app='${app.id}']`)!;
      expect(row.textContent).toContain(app.label);
      expect(row.textContent).toContain(app.detail);
      expect(switchFor(container, app.id).getAttribute("aria-checked")).toBe("false");
    }
    const controls = getByRole("dialog").querySelectorAll("button, input, select, [role='radio']");
    expect(controls.length).toBe(APPS.length + 1);
    expect(container.querySelector("[data-action='cancel-apps']")).toBeNull();
  });

  it("puts the reader on the first switch", () => {
    const { container } = mount();
    expect(document.activeElement).toBe(switchFor(container, "chat"));
  });

  it("toggles an app from its switch and from its row", () => {
    const { container } = mount();
    fireEvent.click(switchFor(container, "chat"));
    fireEvent.click(container.querySelector("[data-app='tags']")!);
    expect(switchFor(container, "chat").getAttribute("aria-checked")).toBe("true");
    expect(switchFor(container, "tags").getAttribute("aria-checked")).toBe("true");
    fireEvent.click(switchFor(container, "graph"));
    fireEvent.click(switchFor(container, "graph"));
    expect(switchFor(container, "graph").getAttribute("aria-checked")).toBe("false");
  });

  it("sends the format and the switched-on apps in one call, and leaves", async () => {
    const { container } = mount();
    fireEvent.click(continueButton(container));
    await waitFor(() => expect(firstRunStore.step()).toBeNull());
    expect(mocks.finishFirstRun).toHaveBeenCalledTimes(1);
    expect(mocks.finishFirstRun).toHaveBeenCalledWith("txt", ["chat", "tags"]);
    expect(configStore.isAppOn("chat")).toBe(true);
    expect(configStore.isAppOn("tags")).toBe(true);
    expect(configStore.isAppOn("graph")).toBe(false);
  });

  it("opens again from Settings with each switch where it is", () => {
    firstRunStore.showApps();
    const { container } = mount();
    expect(firstRunStore.step()).toBe("apps");
    expect(switchFor(container, "chat").getAttribute("aria-checked")).toBe("true");
    expect(switchFor(container, "tags").getAttribute("aria-checked")).toBe("true");
    expect(switchFor(container, "rewrite").getAttribute("aria-checked")).toBe("false");
  });

  it("there, Continue writes the switches and opens no file", async () => {
    const { container } = mount();
    fireEvent.click(switchFor(container, "graph"));
    fireEvent.click(switchFor(container, "chat"));
    fireEvent.click(continueButton(container));
    await waitFor(() => expect(firstRunStore.step()).toBeNull());
    expect(mocks.finishFirstRun).toHaveBeenCalledTimes(1);
    expect(mocks.updateConfig).toHaveBeenCalledTimes(1);
    const written = mocks.updateConfig.mock.calls[0][0];
    expect(written.apps).toEqual({ connections: false, graph: true, tags: true });
    expect(written.ai.chat.enabled).toBe(false);
  });

  it("and Escape leaves it without writing", () => {
    mocks.updateConfig.mockClear();
    firstRunStore.showApps();
    const { container } = mount();
    fireEvent.click(switchFor(container, "rewrite"));
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(firstRunStore.step()).toBeNull();
    expect(mocks.updateConfig).not.toHaveBeenCalled();
    expect(configStore.isAppOn("rewrite")).toBe(false);
  });

  it("and Cancel does the same, where a first launch has no Cancel at all", () => {
    firstRunStore.showApps();
    const { container } = mount();
    fireEvent.click(switchFor(container, "connections"));
    fireEvent.click(container.querySelector("[data-action='cancel-apps']")!);
    expect(firstRunStore.step()).toBeNull();
    expect(mocks.updateConfig).not.toHaveBeenCalled();
    expect(configStore.isAppOn("connections")).toBe(false);
  });
});
