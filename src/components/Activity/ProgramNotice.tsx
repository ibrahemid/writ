import { createEffect, createSignal, Show, untrack } from "solid-js";

import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import { showToast } from "../Notifications/Toast";
import { activityStore } from "../../stores/global/activity";
import { configStore } from "../../stores/global/config";
import { programName, WaitingChoices } from "./WaitingProgram";
import "./ProgramNotice.css";

function CloseButton(props: { onClick: () => void }) {
  return (
    <Tooltip label="Close">
      <Button
        variant="ghost"
        icon="x"
        iconSize={14}
        class="program-notice-close"
        aria-label="Close"
        onClick={props.onClick}
      />
    </Tooltip>
  );
}

export default function ProgramNotice() {
  const [deniedHere, setDeniedHere] = createSignal<string | null>(null);
  const denied = () => {
    const name = deniedHere();
    return name !== null && activityStore.isNotAllowed(name) ? name : null;
  };
  const on = () => configStore.isAppOn("programs");
  const waiting = () => (on() ? activityStore.notice() : null);

  createEffect(() => {
    if (denied() === null && untrack(deniedHere) !== null) setDeniedHere(null);
  });

  async function undo(name: string) {
    try {
      await activityStore.undoDenial(name);
      activityStore.announce(name);
      setDeniedHere(null);
    } catch {
      showToast("Could not undo the change", "error");
    }
  }

  function close() {
    if (denied() !== null) {
      setDeniedHere(null);
      return;
    }
    const client = waiting();
    if (client) activityStore.dismissNotice(client.name);
  }

  return (
    <div class="program-notice-live" role="status" aria-live="polite">
      <Show
        when={on() && denied()}
        fallback={
          <Show when={waiting()}>
            {(client) => (
              <section class="program-notice" data-program={client().name} aria-label="Program waiting">
                <span class="program-notice-text">
                  {programName(client().name)} wants to read your folder.
                </span>
                <WaitingChoices client={client()} onDenied={(name) => setDeniedHere(name)} />
                <CloseButton onClick={close} />
              </section>
            )}
          </Show>
        }
      >
        {(name) => (
          <section class="program-notice" data-denied={name()} aria-label="Program not allowed">
            <span class="program-notice-text">{programName(name())} is not allowed.</span>
            <span class="activity-decide-controls">
              <Button data-action="undo-deny" onClick={() => void undo(name())}>
                Undo
              </Button>
            </span>
            <CloseButton onClick={close} />
          </section>
        )}
      </Show>
    </div>
  );
}
