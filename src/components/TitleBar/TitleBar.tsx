import { hideWindow, minimizeWindow } from "../../services/tauri";
import "./TitleBar.css";

export default function TitleBar() {
  return (
    <div class="titlebar" data-tauri-drag-region>
      <div class="titlebar-controls">
        <button class="titlebar-btn titlebar-btn-close" onClick={hideWindow} title="Hide window">
          <svg width="8" height="8" viewBox="0 0 10 10">
            <path d="M2 2L8 8M8 2L2 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
          </svg>
        </button>
        <button class="titlebar-btn titlebar-btn-minimize" onClick={minimizeWindow} title="Minimize">
          <svg width="8" height="8" viewBox="0 0 10 10">
            <path d="M2 5H8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
          </svg>
        </button>
      </div>
      <div class="titlebar-title" data-tauri-drag-region>Writ</div>
      <div class="titlebar-spacer" />
    </div>
  );
}
