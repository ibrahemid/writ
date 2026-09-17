import { Show, Switch, Match, createEffect, onCleanup } from "solid-js";
import Button from "../Button/Button";
import { updateStore } from "../../stores/global/update";
import { formatBytes } from "../../lib/format-bytes";
import "./UpdateBanner.css";

const UP_TO_DATE_VISIBLE_MS = 2500;
const UNREACHABLE_COPY = "Couldn't reach the update server. Try again later.";

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
  const failure = () => {
    const p = phase();
    return p.status === "failed" ? p.message.trim() || UNREACHABLE_COPY : undefined;
  };

  createEffect(() => {
    if (phase().status !== "up_to_date") return;
    const timer = setTimeout(() => void updateStore.dismiss(), UP_TO_DATE_VISIBLE_MS);
    onCleanup(() => clearTimeout(timer));
  });

  return (
    <div class="update-banner-live" role="status" aria-live="polite">
      <Show when={phase().status !== "idle"}>
        <div class="update-banner">
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
                  Update available: <strong>v{p().version}</strong>
                </span>
                <div class="update-banner-actions">
                  <Button variant="ghost" onClick={() => void updateStore.dismiss()}>
                    Later
                  </Button>
                  <Button variant="primary" onClick={() => void updateStore.install()}>
                    Install
                  </Button>
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
                    <span class="update-banner-amount" aria-live="off">
                      {pct() !== null ? `${pct()}%` : formatBytes(p().downloaded)}
                    </span>
                  </span>
                  <div
                    class={`update-banner-track${pct() === null ? " indeterminate" : ""}`}
                    role="progressbar"
                    aria-live="off"
                    aria-label="Downloading update"
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={pct() ?? undefined}
                    aria-valuetext={pct() === null ? "Downloading" : undefined}
                  >
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
              <Button variant="ghost" onClick={() => void updateStore.dismiss()}>
                Later
              </Button>
              <Button variant="primary" onClick={() => void updateStore.restart()}>
                Restart now
              </Button>
            </div>
          </Match>

          <Match when={failure()}>
            {(message) => (
              <>
                <span class="update-banner-text update-banner-error">{message()}</span>
                <div class="update-banner-actions">
                  <Button variant="ghost" onClick={() => void updateStore.dismiss()}>
                    Later
                  </Button>
                  <Button variant="primary" onClick={() => void updateStore.checkForUpdate()}>
                    Retry
                  </Button>
                </div>
              </>
            )}
          </Match>
          </Switch>
        </div>
      </Show>
    </div>
  );
}
