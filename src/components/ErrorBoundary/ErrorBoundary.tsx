import { ErrorBoundary as SolidErrorBoundary, type ParentProps } from "solid-js";
import Button from "../Button/Button";
import { clipboardStore } from "../../stores/global/clipboard";
import { showToast } from "../Notifications/Toast";
import "./ErrorBoundary.css";

export default function ErrorBoundary(props: ParentProps) {
  return (
    <SolidErrorBoundary
      fallback={(err, reset) => {
        const details = String(err);

        async function onCopy() {
          try {
            await clipboardStore.copyText(details);
            showToast("Copied the details", "success");
          } catch {
            showToast("Could not copy the details", "error");
          }
        }

        return (
          <div class="error-boundary">
            <div class="error-boundary-title">Something went wrong</div>
            <pre class="error-boundary-message">{details}</pre>
            <div class="error-boundary-actions">
              <Button variant="primary" onClick={reset}>
                Try again
              </Button>
              <Button data-action="copy-crash-details" onClick={() => void onCopy()}>
                Copy the details
              </Button>
            </div>
          </div>
        );
      }}
    >
      {props.children}
    </SolidErrorBoundary>
  );
}
