import Button from "../Button/Button";
import Kbd from "../Kbd/Kbd";
import { executeCommand } from "../../commands/registry";
import "./SidebarEmpty.css";

// Cold front door: with nothing open and no history, the sidebar would
// otherwise be blank. Point a first-run reader at the two ways in.
export default function SidebarEmpty() {
  return (
    <div class="sidebar-empty">
      <p class="sidebar-empty-title">No notes yet.</p>
      <div class="sidebar-empty-line">
        <Button
          variant="primary"
          icon="note-pencil"
          iconSize={16}
          onClick={() => executeCommand("note.new")}
        >
          New note
        </Button>
        <Kbd binding="CmdOrCtrl+N" />
      </div>
      <div class="sidebar-empty-line">
        <Button
          variant="secondary"
          icon="folder-open"
          iconSize={16}
          onClick={() => executeCommand("workspace.openFolder")}
        >
          Open a folder
        </Button>
        <Kbd binding="CmdOrCtrl+O" />
      </div>
    </div>
  );
}
