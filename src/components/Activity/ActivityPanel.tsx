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
import { useWindow } from "../WindowProvider/WindowProvider";
import { installFocusTrap } from "../../lib/focus-trap";
import { foldActivity, type ActivityRow } from "../../lib/activity-rows";
import { describeCall } from "../../lib/tool-phrases";
import { requestConfirm } from "../ConfirmDialog/ConfirmDialog";
import { showToast } from "../Notifications/Toast";
import { activityStore, ACTIVITY_POLL_MS, type ActivityRecord } from "../../stores/global/activity";
import { programName, timeOf, WaitingList } from "./WaitingProgram";
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

function actorName(record: ActivityRecord): string {
  switch (record.actor.kind) {
    case "client":
      return programName(record.actor.name);
    case "chat":
      return record.actor.host;
    case "app":
      return "Writ";
  }
}

function Row(props: { row: ActivityRow }) {
  const record = () => props.row.record;

  return (
    <li class="activity-row">
      <span class="activity-time">{timeOf(record().at)}</span>
      <span class="activity-what">
        <span class="activity-program">{actorName(record())}</span>
        <span class="activity-action">
          {describeCall(record().action, record().path !== null, record().decision)}
        </span>
        <Show when={record().path}>
          {(path) => <span class="activity-note">{path()}</span>}
        </Show>
        <Show when={props.row.calls > 1}>
          <span class="activity-count">{props.row.calls} times</span>
        </Show>
      </span>
      <span class="activity-verdict" data-decision={record().decision}>
        {VERDICT[record().decision]}
      </span>
    </li>
  );
}

function ActivityDialog() {
  const titleId = createUniqueId();
  const subtitleId = createUniqueId();
  const writHeadingId = createUniqueId();
  const win = useWindow();
  let dialogRef: HTMLDivElement | undefined;

  const programRows = createMemo(() =>
    foldActivity(activityStore.records().filter((record) => record.actor.kind === "client")),
  );
  const writRows = createMemo(() =>
    foldActivity(activityStore.records().filter((record) => record.actor.kind !== "client")),
  );

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
    const teardown = installFocusTrap(dialogRef, {
      onEscape: () => closeActivity(),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    onCleanup(teardown);
  });

  async function onCopyCommand() {
    try {
      await activityStore.copyCommand();
      showToast("Command copied", "success");
    } catch {
      showToast("Could not copy the command", "error");
    }
  }

  async function onClear() {
    const confirmed = await requestConfirm({
      title: "Clear the activity list?",
      message: "Writ will not have a record of what programs did before now.",
      confirmLabel: "Clear",
      danger: true,
      defaultAction: "cancel",
    });
    if (!confirmed) return;
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
        aria-describedby={subtitleId}
      >
        <div class="activity-header">
          <span class="activity-heading">
            <span id={titleId} class="activity-title">
              Program activity
            </span>
            <span id={subtitleId} class="activity-subtitle">
              Calls from programs connected to your folder, newest first.
            </span>
          </span>
          <span class="activity-header-controls">
            <Show when={activityStore.records().length > 0}>
              <Button data-action="activity-clear" onClick={() => void onClear()}>
                Clear
              </Button>
            </Show>
            <Tooltip label="Close program activity">
              <Button
                variant="ghost"
                icon="x"
                iconSize={16}
                onClick={closeActivity}
                aria-label="Close program activity"
              />
            </Tooltip>
          </span>
        </div>

        <WaitingList />

        <div class="activity-log">
          <Show
            when={programRows().length > 0}
            fallback={
              <div class="activity-empty">
                <p class="activity-empty-line">No program has called yet.</p>
                <p class="activity-empty-line">
                  Add the command to a program's MCP settings, then ask it to read a file.
                </p>
                <Show when={activityStore.serverCommand()}>
                  <Button data-action="activity-copy-command" onClick={() => void onCopyCommand()}>
                    Copy command
                  </Button>
                </Show>
              </div>
            }
          >
            <ul class="activity-list" data-rows="programs">
              <For each={programRows()}>{(row) => <Row row={row} />}</For>
            </ul>
          </Show>

          <Show when={writRows().length > 0}>
            <h2 id={writHeadingId} class="activity-section-title">
              Chat and earlier versions
            </h2>
            <ul class="activity-list" data-rows="writ" aria-labelledby={writHeadingId}>
              <For each={writRows()}>{(row) => <Row row={row} />}</For>
            </ul>
          </Show>
        </div>
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
