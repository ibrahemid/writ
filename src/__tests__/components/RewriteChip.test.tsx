import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const hoisted = vi.hoisted(() => ({
  aiCheckConnection: vi.fn(),
  openSettings: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  updateConfig: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn(),
  aiCheckConnection: hoisted.aiCheckConnection,
}));

vi.mock("../../components/SettingsModal/SettingsModal", () => ({
  default: () => null,
  openSettings: hoisted.openSettings,
}));

vi.mock("../../commands/ai", () => ({ runRewriteAction: vi.fn() }));

import RewriteChip from "../../components/Editor/RewriteChip";
import ContextMenu, { hideContextMenu } from "../../components/ContextMenu/ContextMenu";
import { aiConnectionStore } from "../../stores/global/ai-connection";
import { configStore } from "../../stores/global/config";

const HOSTED_CONSENT_REQUIRED = {
  reachable: false,
  model_listed: null,
  kind: "consent_required",
  detail: "api.groq.com",
  models: [],
};

async function enableRewrite() {
  const current = configStore.config();
  await configStore.save({
    ...current,
    ai: {
      ...current.ai,
      enabled: true,
      preset: "groq",
      base_url: "https://api.groq.com/openai/v1",
      model: "llama3",
    },
  });
}

beforeEach(async () => {
  hoisted.aiCheckConnection.mockReset();
  hoisted.openSettings.mockClear();
  aiConnectionStore.reset();
  await enableRewrite();
});

afterEach(() => {
  hideContextMenu();
  cleanup();
});

describe("RewriteChip connection line", () => {
  // A hosted endpoint is not probed until the user allows it, so the menu must
  // not claim the provider is unreachable.
  it("shows the consent-needed state rather than an error", async () => {
    hoisted.aiCheckConnection.mockResolvedValue(HOSTED_CONSENT_REQUIRED);
    await aiConnectionStore.check();

    const { container, getByText } = render(() => (
      <>
        <RewriteChip />
        <ContextMenu />
      </>
    ));
    fireEvent.click(container.querySelector(".statusbar-chip")!);

    expect(getByText("Not checked until you allow api.groq.com")).not.toBeNull();
  });

  it("shows the connected state once the host is allowed", async () => {
    hoisted.aiCheckConnection.mockResolvedValue({
      reachable: true,
      model_listed: true,
      kind: "ok",
      detail: "",
      models: ["llama3"],
    });
    await aiConnectionStore.check();

    const { container, getByText } = render(() => (
      <>
        <RewriteChip />
        <ContextMenu />
      </>
    ));
    fireEvent.click(container.querySelector(".statusbar-chip")!);

    await waitFor(() => expect(getByText("Connected")).not.toBeNull());
  });
});
