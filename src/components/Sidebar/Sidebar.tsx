import { Show } from "solid-js";
import { sidebarStore } from "../../stores/sidebar";
import SearchBar from "./SearchBar";
import TabList from "./TabList";
import HistoryList from "./HistoryList";
import "./Sidebar.css";

export default function Sidebar() {
  return (
    <Show when={sidebarStore.isVisible()}>
      <div class="sidebar">
        <SearchBar />
        <div class="sidebar-section">
          <div class="sidebar-section-title">Active Tabs</div>
          <TabList />
        </div>
        <div class="sidebar-section">
          <div class="sidebar-section-title">History</div>
          <HistoryList />
        </div>
      </div>
    </Show>
  );
}
