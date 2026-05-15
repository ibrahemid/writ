import { createSignal, createRoot } from "solid-js";

export type FocusRegion = "sidebar" | "tabstrip" | "editor" | "statusbar";

function createFocusStore() {
  const [activeRegion, setActiveRegion] = createSignal<FocusRegion>("editor");
  return {
    activeRegion,
    setActiveRegion,
  };
}

export const focusStore = createRoot(createFocusStore);
