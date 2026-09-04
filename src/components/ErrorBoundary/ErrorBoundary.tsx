import { ErrorBoundary as SolidErrorBoundary, type ParentProps } from "solid-js";
import Button from "../Button/Button";
import "./ErrorBoundary.css";

export default function ErrorBoundary(props: ParentProps) {
  return (
    <SolidErrorBoundary
      fallback={(err, reset) => (
        <div class="error-boundary">
          <div class="error-boundary-title">Something went wrong</div>
          <pre class="error-boundary-message">{String(err)}</pre>
          <Button variant="primary" onClick={reset}>Try again</Button>
        </div>
      )}
    >
      {props.children}
    </SolidErrorBoundary>
  );
}
