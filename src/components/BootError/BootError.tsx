import FailureScreen from "../FailureScreen/FailureScreen";
import { failureDetails } from "../../lib/app-info";
import type { BootFailure } from "../../stores/window/boot";
import "./BootError.css";

interface Props {
  failure: BootFailure;
  onRetry: () => void;
}

/**
 * What the window shows when the boot cannot give it tabs. It covers the
 * editor rather than replacing it, so nothing under it is torn down.
 */
export default function BootError(props: Props) {
  return (
    <div class="boot-error">
      <FailureScreen
        heading="Writ could not open its list of files"
        message="The database that remembers your tabs and Recent could not be read. Your files on disk are unchanged."
        details={failureDetails(props.failure.detail)}
        retryLabel="Try again"
        onRetry={props.onRetry}
      />
    </div>
  );
}
