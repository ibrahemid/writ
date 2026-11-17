import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { DictationPhase } from "../../../stores/window/dictation-store";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [phase, setPhase] = createSignal<DictationPhase>("idle");
  const [level, setLevel] = createSignal(0);
  const [progress, setProgress] = createSignal<number | null>(null);
  const [pending, setPending] = createSignal(0);
  const [elapsed, setElapsed] = createSignal<number | null>(null);
  return {
    updateConfig: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
    phase,
    setPhase,
    level,
    setLevel,
    progress,
    setProgress,
    pending,
    setPending,
    elapsed,
    setElapsed,
  };
});

vi.mock("../../../services/tauri", () => ({ updateConfig: h.updateConfig }));
vi.mock("../../WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    dictation: {
      phase: h.phase,
      isLive: () => h.phase() !== "idle",
      level: h.level,
      progress: h.progress,
      pending: h.pending,
      elapsedSeconds: h.elapsed,
      stop: h.stop,
    },
  }),
}));

import { configStore } from "../../../stores/global/config";
import DictationIndicator, { formatElapsed } from "../DictationIndicator";

function indicator(container: HTMLElement): HTMLButtonElement | null {
  return container.querySelector<HTMLButtonElement>(".dictation-indicator");
}

beforeEach(async () => {
  await configStore.setAppOn("dictation", true);
});

afterEach(() => {
  cleanup();
  h.setPhase("idle");
  h.setLevel(0);
  h.setProgress(null);
  h.setPending(0);
  h.setElapsed(null);
  vi.clearAllMocks();
});

describe("DictationIndicator", () => {
  it("is absent while nothing is being dictated", () => {
    const { container } = render(() => <DictationIndicator />);
    expect(indicator(container)).toBeNull();
  });

  it("is absent while Dictation is off, even mid-session", async () => {
    h.setPhase("recording");
    await configStore.setAppOn("dictation", false);
    const { container } = render(() => <DictationIndicator />);
    expect(indicator(container)).toBeNull();
  });

  it("shows the time the microphone has been on as m:ss", () => {
    h.setPhase("recording");
    h.setElapsed(0);
    const { container } = render(() => <DictationIndicator />);
    expect(container.querySelector(".dictation-indicator-time")?.textContent).toBe("0:00");
    h.setElapsed(75);
    expect(container.querySelector(".dictation-indicator-time")?.textContent).toBe("1:15");
  });

  it("drives the meter from the microphone level", () => {
    h.setPhase("recording");
    h.setLevel(0.25);
    const { container } = render(() => <DictationIndicator />);
    const meter = container.querySelector<HTMLElement>(".dictation-indicator-level");
    expect(meter?.style.getPropertyValue("--dictation-level")).toBe("0.25");
    expect(meter?.getAttribute("aria-hidden")).toBe("true");
    h.setLevel(0.8);
    expect(meter?.style.getPropertyValue("--dictation-level")).toBe("0.8");
  });

  it("shows the model download while preparing", () => {
    h.setPhase("preparing");
    h.setProgress(0.426);
    const { container } = render(() => <DictationIndicator />);
    expect(container.querySelector(".dictation-indicator-note")?.textContent).toBe(
      "Getting the speech model 43%",
    );
    expect(container.querySelector(".dictation-indicator-level")).toBeNull();
  });

  it("says Transcribing after stop while segments are still out", () => {
    h.setPhase("stopping");
    h.setElapsed(12);
    h.setPending(2);
    const { container } = render(() => <DictationIndicator />);
    const button = indicator(container)!;
    expect(container.querySelector(".dictation-indicator-note")?.textContent).toBe("Transcribing");
    expect(button.getAttribute("aria-label")).toBe("Transcribing");
    expect(button.getAttribute("aria-disabled")).toBe("true");
    h.setPending(0);
    expect(container.querySelector(".dictation-indicator-note")).toBeNull();
  });

  it("stops the session when pressed", () => {
    h.setPhase("recording");
    const { container } = render(() => <DictationIndicator />);
    const button = indicator(container)!;
    expect(button.getAttribute("aria-label")).toBe("Stop dictation");
    fireEvent.click(button);
    expect(h.stop).toHaveBeenCalledTimes(1);
  });
});

describe("formatElapsed", () => {
  it("pads seconds and lets minutes grow", () => {
    expect(formatElapsed(9)).toBe("0:09");
    expect(formatElapsed(600)).toBe("10:00");
    expect(formatElapsed(-3)).toBe("0:00");
  });
});

describe("the meter under reduced motion", () => {
  const css = readFileSync(
    resolve(process.cwd(), "src/components/Editor/DictationIndicator.css"),
    "utf8",
  );

  it("moves only for a reader who allows motion", () => {
    const motion = css.indexOf("@media (prefers-reduced-motion: no-preference)");
    expect(motion).toBeGreaterThan(-1);
    const resting = css.slice(0, motion);
    expect(resting).not.toMatch(/scaleX|transition:\s*transform/);
    expect(css.slice(motion)).toMatch(/transform:\s*scaleX\(var\(--dictation-level/);
  });
});
