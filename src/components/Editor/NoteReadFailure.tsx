import { Show } from "solid-js";
import Button from "../Button/Button";
import { canShowInFileManager, describeReadFailure, type ReadFailure } from "../../lib/read-failure";
import { showInFileManagerLabel } from "../../lib/note-actions";
import "./NoteReadFailure.css";

interface Props {
  failure: ReadFailure;
  onRetry: () => void;
  onShowInFileManager: () => void;
  onClose: () => void;
}

// The editor pane for a note whose file could not be read. There is no
// document behind it, so nothing typed here can be saved over the file.
export default function NoteReadFailure(props: Props) {
  return (
    <div class="note-read-failure" role="alert">
      <p class="note-read-failure-line">{describeReadFailure(props.failure)}</p>
      <div class="note-read-failure-actions">
        <Button variant="primary" onClick={props.onRetry}>
          Try again
        </Button>
        <Show when={canShowInFileManager(props.failure)}>
          <Button onClick={props.onShowInFileManager}>{showInFileManagerLabel()}</Button>
        </Show>
        <Button onClick={props.onClose}>Close</Button>
      </div>
    </div>
  );
}
