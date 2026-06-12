import Kbd from "../Kbd/Kbd";
import "./SidebarEmpty.css";

// Cold front door: with no open buffers and no history, the sidebar would
// otherwise be a blank sunken void. Point a first-run user at the two ways to
// get a buffer.
export default function SidebarEmpty() {
  return (
    <div class="sidebar-empty">
      <p class="sidebar-empty-title">No open files</p>
      <p class="sidebar-empty-hint">
        <Kbd binding="CmdOrCtrl+O" /> open a file
      </p>
      <p class="sidebar-empty-hint">
        <Kbd binding="CmdOrCtrl+T" /> new scratch buffer
      </p>
    </div>
  );
}
