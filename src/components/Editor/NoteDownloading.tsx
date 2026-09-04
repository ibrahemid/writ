import { Match, Switch, Show } from "solid-js";
import type { PendingDownload } from "../../stores/window/download-store";
import "./NoteDownloading.css";

interface Props {
  download: PendingDownload;
  onCancel: () => void;
  onClose: () => void;
}

// The editor pane for a note whose bytes are not on this machine yet. There is
// nothing to edit until the sync provider hands them over, and no progress to
// report: the provider does not say how far along it is.
export default function NoteDownloading(props: Props) {
  const provider = () => props.download.provider;

  return (
    <div class="note-downloading">
      <Switch>
        <Match when={props.download.state === "downloading"}>
          <p class="note-downloading-line">
            {provider() ? `Downloading from ${provider()}…` : "Downloading…"}
          </p>
          <button type="button" class="note-downloading-action" onClick={props.onCancel}>
            Cancel
          </button>
        </Match>

        <Match when={props.download.state === "failed"}>
          <p class="note-downloading-line">This file could not be downloaded.</p>
          <Show when={props.download.message}>
            {(message) => <p class="note-downloading-detail">{message()}</p>}
          </Show>
          <button type="button" class="note-downloading-action" onClick={props.onClose}>
            Close
          </button>
        </Match>

        <Match when={props.download.state === "timed_out"}>
          <p class="note-downloading-line">
            {provider()
              ? `Still waiting for ${provider()}. Try again once the file has downloaded.`
              : "Still waiting. Try again once the file has downloaded."}
          </p>
          <button type="button" class="note-downloading-action" onClick={props.onClose}>
            Close
          </button>
        </Match>
      </Switch>
    </div>
  );
}
