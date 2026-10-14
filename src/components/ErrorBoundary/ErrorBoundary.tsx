import { ErrorBoundary as SolidErrorBoundary, type ParentProps } from "solid-js";
import FailureScreen from "../FailureScreen/FailureScreen";
import { failureDetails } from "../../lib/app-info";
import { logFailure } from "../../lib/log";

export default function ErrorBoundary(props: ParentProps) {
  return (
    <SolidErrorBoundary
      fallback={(err, reset) => {
        logFailure("the window could not be drawn");
        return (
          <FailureScreen
            heading="Writ could not draw this window"
            message="Saved files are unchanged on disk. Try again draws the window again."
            details={failureDetails(String(err))}
            retryLabel="Try again"
            onRetry={reset}
            withWindowControls
          />
        );
      }}
    >
      {props.children}
    </SolidErrorBoundary>
  );
}
