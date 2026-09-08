import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  createUniqueId,
} from "solid-js";

import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import { installFocusTrap } from "../../lib/focus-trap";
import { showToast } from "../Notifications/Toast";
import {
  activityStore,
  ACTIVITY_POLL_MS,
  type ActivityRecord,
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

function Row(props: { record: ActivityRecord; decidable: boolean }) {
  const record = () => props.record;
  const name = () => actorName(record());

  async function approve(read: boolean, write: boolean) {
    try {
      await activityStore.setPermission(name(), read, write);
    } catch {
      showToast("Could not save the approval", "error");
    }
  }

  return (
    <li class="activity-row" classList={{ "activity-row-waiting": props.decidable }}>
      <span class="activity-time">{timeOf(record().at)}</span>
      <span class="activity-what">
        <span class="activity-program">{name()}</span>
        <span class="activity-action">{record().action}</span>
        <Show when={record().path}>
          {(path) => <span class="activity-note">{path()}</span>}
        </Show>
      </span>
      <span class="activity-verdict" data-decision={record().decision}>
        {VERDICT[record().decision]}
      </span>
      <Show when={props.decidable}>
        <span class="activity-decide">
          <span class="activity-decide-line">
            It reads and writes nothing until you approve it.
          </span>
          <span class="activity-decide-controls">
            <Button data-action="approve-read" onClick={() => void approve(true, false)}>
              Approve reading
            </Button>
            <Button data-action="approve-write" onClick={() => void approve(true, true)}>
              Approve writing
            </Button>
          </span>
        </span>
      </Show>
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

  // A program that made three waiting calls is still one decision, and a
  // program already on the approved list is no decision at all. The store names
  // who is actually waiting; the controls go on that program's newest row.
  const decidable = createMemo(() => {
    const waiting = new Set(activityStore.pending().map((client) => client.name));
    const rows = new Set<number>();
    activityStore.records().forEach((row, index) => {
      if (row.actor.kind !== "client") return;
      if (row.decision !== "pending") return;
      if (!waiting.has(row.actor.name)) return;
      waiting.delete(row.actor.name);
      rows.add(index);
    });
    return rows;
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

        <Show
          when={activityStore.records().length > 0}
          fallback={<p class="activity-empty">No program has called yet.</p>}
        >
          <ul class="activity-list">
            <For each={activityStore.records()}>
              {(record, index) => (
                <Row record={record} decidable={decidable().has(index())} />
              )}
            </For>
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
