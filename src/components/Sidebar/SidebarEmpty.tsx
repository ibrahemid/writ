import Button from "../Button/Button";
import Kbd from "../Kbd/Kbd";
import { executeCommand } from "../../commands/registry";
import { firstRunStore } from "../../stores/global/first-run";
import "./SidebarEmpty.css";

// Shown in place of the notes folder's tree when the folder holds nothing,
// and only then: beside a folder of files it would be false (NV-01).
export default function SidebarEmpty() {
  return (
    <div class="sidebar-empty">
      <p class="sidebar-empty-title">This folder is empty.</p>
      <div class="sidebar-empty-line">
        <Button
          variant="primary"
          icon="note-pencil"
          iconSize={16}
          onClick={() => executeCommand("note.new")}
        >
          New file
        </Button>
        <Kbd binding="CmdOrCtrl+N" />
      </div>
      <div class="sidebar-empty-line">
        <Button
          variant="secondary"
          icon="folder-open"
          iconSize={16}
          onClick={() => executeCommand("notes.showFolder")}
        >
          {`Show folder in ${firstRunStore.fileManager()}`}
        </Button>
      </div>
    </div>
  );
}
