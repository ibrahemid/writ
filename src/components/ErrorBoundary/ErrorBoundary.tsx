import { ErrorBoundary as SolidErrorBoundary, type ParentProps } from "solid-js";
import "./ErrorBoundary.css";

export default function ErrorBoundary(props: ParentProps) {
  return (
    <SolidErrorBoundary
      fallback={(err, reset) => (
        <div class="error-boundary">
          <div class="error-boundary-title">Something went wrong</div>
          <pre class="error-boundary-message">{String(err)}</pre>
          <button class="error-boundary-reset" onClick={reset}>Try Again</button>
        </div>
      )}
    >
      {props.children}
    </SolidErrorBoundary>
  );
}
