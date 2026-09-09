import { createEffect, createSignal, For, onCleanup, onMount, Show, createUniqueId } from "solid-js";

import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import { installFocusTrap } from "../../lib/focus-trap";
import { showToast } from "../Notifications/Toast";
import {
  activityStore,
  ACTIVITY_POLL_MS,
  type ActivityRecord,
  type PendingClient,
} from "../../stores/global/activity";
import "./ActivityPanel.css";

// Singleton state — Writ is single-window

const [isOpen, setIsOpen] = createSignal(false);

export function openActivity() {
  setIsOpen(true);
}

export function closeActivity() {
  setIsOpen(false);
}

export function toggleActivity() {
  setIsOpen((open) => !open);
}

export function isActivityOpen(): boolean {
  return isOpen();
}

/** What the row says the call was decided as. */
const VERDICT: Record<ActivityRecord["decision"], string> = {
  allow: "Allowed",
  refuse: "Not allowed",
  pending: "Waiting",
};

/** A program that sent no name still made a call, and the row still shows it. */
const UNNAMED_PROGRAM = "Unknown program";

function actorName(record: ActivityRecord): string {
  switch (record.actor.kind) {
    case "client":
      return record.actor.name.trim() || UNNAMED_PROGRAM;
    case "chat":
      return record.actor.host;
    case "app":
      return "Writ";
  }
}

/** The time of day, in the reader's own locale. */
function timeOf(at: string): string {
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
function firstSeenLabel(at: string): string {
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

function Row(props: { record: ActivityRecord }) {
  const record = () => props.record;

  return (
    <li class="activity-row">
      <span class="activity-time">{timeOf(record().at)}</span>
      <span class="activity-what">
        <span class="activity-program">{actorName(record())}</span>
        <span class="activity-action">{record().action}</span>
        <Show when={record().path}>
          {(path) => <span class="activity-note">{path()}</span>}
        </Show>
      </span>
      <span class="activity-verdict" data-decision={record().decision}>
        {VERDICT[record().decision]}
      </span>
    </li>
  );
}

/**
 * One program the user has not decided on.
 *
 * Read from the store's own list rather than from a log row: the log is capped,
 * and a program making calls it is not allowed to make would otherwise push
 * itself out of the list where it is decided on.
 */
function Waiting(props: { client: PendingClient }) {
  const name = () => props.client.name.trim() || UNNAMED_PROGRAM;

  async function approve(write: boolean) {
    try {
      await activityStore.setPermission(name(), true, write);
    } catch {
      showToast("Could not save the approval", "error");
    }
  }

  return (
    <li class="activity-waiting" data-program={props.client.name}>
      <span class="activity-waiting-what">
        <span class="activity-program">{name()}</span>
        <Show when={firstSeenLabel(props.client.first_seen)}>
          {(label) => <span class="activity-waiting-when">since {label()}</span>}
        </Show>
      </span>
      <span class="activity-decide-line">It reads and writes nothing until you approve it.</span>
      <span class="activity-decide-controls">
        <Button data-action="approve-read" onClick={() => void approve(false)}>
          Approve reading
        </Button>
        <Button data-action="approve-write" onClick={() => void approve(true)}>
          Approve reading and writing
        </Button>
      </span>
    </li>
  );
}

function ActivityDialog() {
  const titleId = createUniqueId();
  let dialogRef: HTMLDivElement | undefined;

  onMount(() => {
    void activityStore.load();
  });

  // The `writ mcp` process cannot tell a running app it wrote a line, so the
  // list re-reads while it is on screen. This whole component is unmounted when
  // the panel closes, so the interval is cleared with it.
  onMount(() => {
    const timer = setInterval(() => void activityStore.refresh(), ACTIVITY_POLL_MS);
    onCleanup(() => clearInterval(timer));
  });

  createEffect(() => {
    if (!dialogRef) return;
    const teardown = installFocusTrap(dialogRef, { onEscape: () => closeActivity() });
    onCleanup(teardown);
  });

  async function onClear() {
    try {
      await activityStore.clear();
    } catch {
      showToast("Could not clear the list", "error");
    }
  }

  return (
    <div class="activity-overlay" onClick={() => closeActivity()}>
      <div
        ref={dialogRef}
        class="activity-modal"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <div class="activity-header">
          <span id={titleId} class="activity-title">
            Activity
          </span>
          <span class="activity-header-controls">
            <Show when={activityStore.records().length > 0}>
              <Button data-action="activity-clear" onClick={() => void onClear()}>
                Clear
              </Button>
            </Show>
            <Tooltip label="Close activity">
              <Button
                variant="ghost"
                icon="x"
                iconSize={16}
                onClick={closeActivity}
                aria-label="Close activity"
              />
            </Tooltip>
          </span>
        </div>

        <Show when={activityStore.pending().length > 0}>
          <ul class="activity-waiting-list">
            <For each={activityStore.pending()}>
              {(client) => <Waiting client={client} />}
            </For>
          </ul>
        </Show>

        <Show
          when={activityStore.records().length > 0}
          fallback={<p class="activity-empty">No program has called yet.</p>}
        >
          <ul class="activity-list">
            <For each={activityStore.records()}>{(record) => <Row record={record} />}</For>
          </ul>
        </Show>
      </div>
    </div>
  );
}

/**
 * What connected programs did, newest first.
 *
 * Mounted only while it is open, so the poll it starts lives exactly as long as
 * the list is on screen.
 */
export default function ActivityPanel() {
  return (
    <Show when={isOpen()}>
      <ActivityDialog />
    </Show>
  );
}
