import { Match, Switch, Show } from "solid-js";
import type { DownloadFailure, PendingDownload } from "../../stores/window/download-store";
import "./NoteDownloading.css";

// A failure the provider reported is about the file; the other two are about
// Writ, and each says what the person can do next.
function failureLine(reason: DownloadFailure): string {
  switch (reason) {
    case "open":
      return "The file downloaded but the note did not open. Open it again.";
    case "listener":
      return "Writ lost track of this download. Open the note again.";
    case "download":
      return "This file could not be downloaded.";
  }
}

interface Props {
  download: PendingDownload;
  onDismiss: () => void;
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
          <button type="button" class="note-downloading-action" onClick={props.onDismiss}>
            Cancel
          </button>
        </Match>

        <Match when={props.download.state === "failed"}>
          <p class="note-downloading-line">{failureLine(props.download.reason)}</p>
          <Show when={props.download.reason === "download" && props.download.message}>
            {(message) => <p class="note-downloading-detail">{message()}</p>}
          </Show>
          <button type="button" class="note-downloading-action" onClick={props.onDismiss}>
            Close
          </button>
        </Match>

        <Match when={props.download.state === "timed_out"}>
          <p class="note-downloading-line">
            {provider()
              ? `Still waiting for ${provider()}. Try again once the file has downloaded.`
              : "Still waiting. Try again once the file has downloaded."}
          </p>
          <button type="button" class="note-downloading-action" onClick={props.onDismiss}>
            Close
          </button>
        </Match>
      </Switch>
    </div>
  );
}
