import { sidebarStore } from "../../stores/sidebar";
import SearchBar from "./SearchBar";
import HistoryList from "./HistoryList";
import "./Sidebar.css";

export default function Sidebar() {
  return (
    <div
      class="sidebar"
      classList={{ "is-open": sidebarStore.isOpen() }}
    >
      <SearchBar />
      <div class="sidebar-section">
        <div class="sidebar-section-title">History</div>
        <HistoryList />
      </div>
    </div>
  );
}
