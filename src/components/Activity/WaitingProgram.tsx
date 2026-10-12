import { createSignal, For, Show, type JSX } from "solid-js";

import Button from "../Button/Button";
import { showToast } from "../Notifications/Toast";
import { activityStore, type PendingClient } from "../../stores/global/activity";
import "./WaitingProgram.css";

/** A program that sent no name still made a call, and the row still shows it. */
export const UNNAMED_PROGRAM = "Unknown program";

export function programName(name: string): string {
  return name.trim() || UNNAMED_PROGRAM;
}

/** The time of day, in the reader's own locale. */
export function timeOf(at: string): string {
  const stamp = new Date(at);
  if (Number.isNaN(stamp.getTime())) return "";
  return stamp.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/**
 * When a program first called: the time today, the date and time before that.
 *
 * A waiting program stays on the list until it is decided on, so its first call
 * can be days old and an hour on its own would read as today.
 */
export function firstSeenLabel(at: string): string {
  const stamp = new Date(at);
  if (Number.isNaN(stamp.getTime())) return "";
  if (stamp.toDateString() === new Date().toDateString()) return timeOf(at);
  return stamp.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function WaitingChoices(props: { client: PendingClient; onDenied?: (name: string) => void }) {
  async function approve(write: boolean) {
    try {
      await activityStore.setPermission(props.client.name, true, write);
    } catch {
      showToast("Could not save the approval", "error");
    }
  }

  async function deny() {
    const name = props.client.name;
    try {
      await activityStore.deny(name);
      props.onDenied?.(name);
    } catch {
      showToast("Could not save the change", "error");
    }
  }

  return (
    <span class="activity-decide-controls">
      <Button data-action="approve-read" onClick={() => void approve(false)}>
        Approve reading
      </Button>
      <Button data-action="approve-write" onClick={() => void approve(true)}>
        Approve reading and writing
      </Button>
      <Button variant="ghost" data-action="refuse" onClick={() => void deny()}>
        Don't allow
      </Button>
    </span>
  );
}

/**
 * One program the user has not decided on.
 *
 * Read from the store's own list rather than from a log row: the log is capped,
 * and a program making calls it is not allowed to make would otherwise push
 * itself out of the list where it is decided on.
 */
export function WaitingProgram(props: { client: PendingClient; onDenied?: (name: string) => void }) {
  return (
    <li class="activity-waiting" data-program={props.client.name}>
      <span class="activity-waiting-what">
        <span class="activity-program">{programName(props.client.name)}</span>
        <Show when={firstSeenLabel(props.client.first_seen)}>
          {(label) => <span class="activity-waiting-when">since {label()}</span>}
        </Show>
      </span>
      <span class="activity-decide-line">It reads and writes nothing until you approve it.</span>
      <WaitingChoices client={props.client} onDenied={props.onDenied} />
    </li>
  );
}

export function WaitingList(props: { empty?: JSX.Element }) {
  const [denied, setDenied] = createSignal<string[]>([]);

  function onDenied(name: string) {
    if (!name.trim()) return;
    setDenied((names) => [...names.filter((entry) => entry !== name), name]);
  }

  async function undo(name: string) {
    try {
      await activityStore.undoDenial(name);
      setDenied((names) => names.filter((entry) => entry !== name));
    } catch {
      showToast("Could not undo the change", "error");
    }
  }

  return (
    <Show
      when={activityStore.pending().length > 0 || denied().length > 0}
      fallback={props.empty}
    >
      <ul class="activity-waiting-list">
        <For each={activityStore.pending()}>
          {(client) => <WaitingProgram client={client} onDenied={onDenied} />}
        </For>
        <For each={denied()}>
          {(name) => (
            <li class="activity-denied" data-denied={name}>
              <span class="activity-denied-text">{programName(name)} is not allowed.</span>
              <Button data-action="undo-deny" onClick={() => void undo(name)}>
                Undo
              </Button>
            </li>
          )}
        </For>
      </ul>
    </Show>
  );
}
