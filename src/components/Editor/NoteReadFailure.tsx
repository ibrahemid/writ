import { Show } from "solid-js";
import Button from "../Button/Button";
import { showInFileManagerLabel } from "../../lib/note-actions";
import "./NoteReadFailure.css";

interface Props {
  line: string;
  actionLabel: string;
  onAction: () => void;
  // A failure is announced. A file waiting on the person's answer is not one.
  alert: boolean;
  canShowInFileManager: boolean;
  onShowInFileManager: () => void;
  onClose: () => void;
}

// The editor pane for a note whose file is not in the editor: a read that
// failed, or a file Writ has not read because of its size. There is no
// document behind it, so nothing typed here can be saved over the file.
export default function NoteReadFailure(props: Props) {
  return (
    <div class="note-read-failure" role={props.alert ? "alert" : undefined}>
      <p class="note-read-failure-line">{props.line}</p>
      <div class="note-read-failure-actions">
        <Button variant="primary" onClick={props.onAction}>
          {props.actionLabel}
        </Button>
        <Show when={props.canShowInFileManager}>
          <Button onClick={props.onShowInFileManager}>{showInFileManagerLabel()}</Button>
        </Show>
        <Button onClick={props.onClose}>Close</Button>
      </div>
    </div>
  );
}
