import { sidebarStore } from "../stores/sidebar";
import { focusSearchBar } from "../components/Sidebar/SearchBar";

export function openContentSearch() {
  sidebarStore.show();
  focusSearchBar();
}
