import { createSignal } from "solid-js";

export type FocusRegion = "sidebar" | "tabstrip" | "editor" | "statusbar";

export type FocusStore = ReturnType<typeof createFocusStore>;

export function createFocusStore() {
  const [activeRegion, setActiveRegion] = createSignal<FocusRegion>("editor");
  return {
    activeRegion,
    setActiveRegion,
  };
}
