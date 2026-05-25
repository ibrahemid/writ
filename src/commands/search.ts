import { windowRegistry } from "../stores/global/window-registry";
import { focusSearchBar } from "../components/Sidebar/SearchBar";

export function openContentSearch() {
  const win = windowRegistry.getActive();
  if (!win) return;
  win.sidebar.show();
  focusSearchBar();
}
