import { createContext, useContext, onMount, onCleanup, type JSX } from "solid-js";
import { createWindowState, type WindowState } from "../../stores/window/createWindowState";
import { windowRegistry } from "../../stores/global/window-registry";

const WindowContext = createContext<WindowState | null>(null);

export interface WindowProviderProps {
  windowId: number;
  children: JSX.Element;
}

export default function WindowProvider(props: WindowProviderProps) {
  const state = createWindowState({ windowId: props.windowId });

  onMount(() => {
    const off = windowRegistry.register(state);
    onCleanup(off);

    // A window with no event bridge behind it still renders; a listener that
    // never attaches is the whole loss, and it must not surface as a rejection.
    const listening = state.downloads.mount().catch(() => undefined);
    onCleanup(() => void listening.then((stop) => stop?.()));
  });

  return (
    <WindowContext.Provider value={state}>{props.children}</WindowContext.Provider>
  );
}

export function useWindow(): WindowState {
  const ctx = useContext(WindowContext);
  if (!ctx) {
    throw new Error("useWindow() must be called inside a <WindowProvider>");
  }
  return ctx;
}
