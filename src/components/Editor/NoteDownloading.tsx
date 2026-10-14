import { Match, Switch, Show } from "solid-js";
import type { DownloadFailure, PendingDownload } from "../../stores/window/download-store";
import "./NoteDownloading.css";
import Button from "../Button/Button";

// A failure the provider reported is about the file; the other two are about
// Writ.
function failureLine(reason: DownloadFailure): string {
  switch (reason) {
    case "open":
      return "The file downloaded but it did not open.";
    case "listener":
      return "Writ lost track of this download.";
    case "download":
      return "This file could not be downloaded.";
  }
}

interface Props {
  download: PendingDownload;
  onRetry: () => void;
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
          <Button onClick={props.onDismiss}>Cancel</Button>
        </Match>

        <Match when={props.download.state === "failed"}>
          <p class="note-downloading-line">{failureLine(props.download.reason)}</p>
          <Show when={props.download.reason === "download" && props.download.message}>
            {(message) => <p class="note-downloading-detail">{message()}</p>}
          </Show>
          <div class="note-downloading-choices">
            <Button onClick={props.onRetry}>Try again</Button>
            <Button onClick={props.onDismiss}>Close</Button>
          </div>
        </Match>

        <Match when={props.download.state === "timed_out"}>
          <p class="note-downloading-line">
            {provider()
              ? `${provider()} has not finished downloading this file.`
              : "This file has not finished downloading."}
          </p>
          <div class="note-downloading-choices">
            <Button onClick={props.onRetry}>Try again</Button>
            <Button onClick={props.onDismiss}>Close</Button>
          </div>
        </Match>
      </Switch>
    </div>
  );
}
