import { Show, Switch, Match, createEffect, onCleanup } from "solid-js";
import { updateStore } from "../../stores/update";
import "./UpdateBanner.css";

const UP_TO_DATE_VISIBLE_MS = 2500;
const UNREACHABLE_COPY = "Couldn't reach the update server. Try again later.";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${Math.round(kb)} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}

export default function UpdateBanner() {
  const phase = updateStore.phase;

  const available = () => {
    const p = phase();
    return p.status === "available" ? p : undefined;
  };
  const downloading = () => {
    const p = phase();
    return p.status === "downloading" ? p : undefined;
  };

  createEffect(() => {
    if (phase().status !== "up_to_date") return;
    const timer = setTimeout(() => void updateStore.dismiss(), UP_TO_DATE_VISIBLE_MS);
    onCleanup(() => clearTimeout(timer));
  });

  return (
    <Show when={phase().status !== "idle"}>
      <div class="update-banner" role="status" aria-live="polite">
        <Switch>
          <Match when={phase().status === "checking"}>
            <span class="update-banner-spinner" aria-hidden="true" />
            <span class="update-banner-text">Checking for updates…</span>
          </Match>

          <Match when={phase().status === "up_to_date"}>
            <span class="update-banner-text">Writ is up to date.</span>
          </Match>

          <Match when={available()}>
            {(p) => (
              <>
                <span class="update-banner-text">
                  Update available — <strong>v{p().version}</strong>
                </span>
                <div class="update-banner-actions">
                  <button class="update-banner-btn ghost" onClick={() => void updateStore.dismiss()}>
                    Later
                  </button>
                  <button class="update-banner-btn primary" onClick={() => void updateStore.install()}>
                    Install
                  </button>
                </div>
              </>
            )}
          </Match>

          <Match when={downloading()}>
            {(p) => {
              const pct = () => {
                const total = p().total;
                return total ? Math.min(100, Math.round((p().downloaded / total) * 100)) : null;
              };
              return (
                <div class="update-banner-progress">
                  <span class="update-banner-text">
                    Downloading update…{" "}
                    {pct() !== null ? `${pct()}%` : formatBytes(p().downloaded)}
                  </span>
                  <div class={`update-banner-track${pct() === null ? " indeterminate" : ""}`}>
                    <div
                      class="update-banner-fill"
                      style={pct() !== null ? { width: `${pct()}%` } : undefined}
                    />
                  </div>
                </div>
              );
            }}
          </Match>

          <Match when={phase().status === "installing"}>
            <span class="update-banner-spinner" aria-hidden="true" />
            <span class="update-banner-text">Installing update…</span>
          </Match>

          <Match when={phase().status === "ready"}>
            <span class="update-banner-text">Update installed.</span>
            <div class="update-banner-actions">
              <button class="update-banner-btn ghost" onClick={() => void updateStore.dismiss()}>
                Later
              </button>
              <button class="update-banner-btn primary" onClick={() => void updateStore.restart()}>
                Restart now
              </button>
            </div>
          </Match>

          <Match when={phase().status === "failed"}>
            <span class="update-banner-text update-banner-error">{UNREACHABLE_COPY}</span>
            <div class="update-banner-actions">
              <button class="update-banner-btn ghost" onClick={() => void updateStore.dismiss()}>
                Dismiss
              </button>
              <button class="update-banner-btn primary" onClick={() => void updateStore.checkForUpdate()}>
                Retry
              </button>
            </div>
          </Match>
        </Switch>
      </div>
    </Show>
  );
}
