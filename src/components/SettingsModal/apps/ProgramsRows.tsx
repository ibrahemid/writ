import { createEffect, createMemo, createSignal, For, onMount, Show, untrack } from "solid-js";

import Button from "../../Button/Button";
import { showToast } from "../../../stores/global/notifications";
import { WaitingList } from "../../Activity/WaitingProgram";
import { activityStore, type ClientApproval } from "../../../stores/global/activity";
import { describeReadTools, describeWriteTools } from "../../../lib/tool-phrases";
import { PathText } from "../controls";
import { SettingsRow, ToggleSwitch } from "../SettingsRow";

/** The row id the client cards take, between the waiting list and the
 * command, once their settings entry and row exist. */
export const CLIENT_CARDS_SETTING_ID = "mcp.connect";

/**
 * Which programs may reach the notes folder, and how one is pointed at it.
 *
 * Turning the switch on approves nobody: a program's first call waits for the
 * user to decide on it, at the top of this section, which is where the refusal
 * the program relays sends the person (ADR-031 rules 3.2 and 7.2). The waiting
 * rows are the Activity window's own. Reading and writing are separate grants,
 * so a program approved to read never writes; a program not allowed either way
 * is listed with both off.
 *
 * The grants read as sentences: the server reports the tool ids it registers,
 * and the tool phrases turn them into the words under each grant.
 */
export default function ProgramsRows(props: { onOpenActivity: () => void }) {
  const [forgottenHere, setForgottenHere] = createSignal<ClientApproval[]>([]);
  const forgotten = createMemo(() =>
    forgottenHere().filter((client) => !activityStore.isDecided(client.name)),
  );

  createEffect(() => {
    const shown = forgotten();
    if (shown.length !== untrack(forgottenHere).length) setForgottenHere(shown);
  });

  onMount(() => {
    void activityStore.refreshClients();
    void activityStore.loadCommand();
    void activityStore.loadTools();
  });

  async function onCopyCommand() {
    try {
      await activityStore.copyCommand();
      showToast("Command copied", "success");
    } catch {
      showToast("Could not copy the command", "error");
    }
  }

  async function onSetPermission(name: string, read: boolean, write: boolean) {
    try {
      await activityStore.setPermission(name, read, write);
    } catch {
      showToast("Could not save the change", "error");
    }
  }

  async function onForget(client: ClientApproval) {
    try {
      await activityStore.forget(client.name);
      setForgottenHere((list) => [...list.filter((entry) => entry.name !== client.name), client]);
    } catch {
      showToast("Could not forget the program", "error");
    }
  }

  async function onUndoForget(client: ClientApproval) {
    try {
      if (!(await activityStore.restore(client))) return;
      setForgottenHere((list) => list.filter((entry) => entry.name !== client.name));
    } catch {
      showToast("Could not undo the change", "error");
    }
  }

  return (
    <div class="settings-app-detail" data-app-detail="programs">
      <SettingsRow id="mcp.waiting" align="start">
        <WaitingList
          empty={
            <span class="settings-programs-none">
              A program appears here when it first asks for your files.
            </span>
          }
        />
      </SettingsRow>

      <SettingsRow
        id="mcp.command"
        caution={activityStore.commandCaution()}
      >
        <span class="settings-inbox-controls">
          <PathText
            class="settings-inbox-path"
            data-mcp-command
            full={activityStore.serverCommand()?.command ?? ""}
            text={activityStore.serverCommand()?.command ?? "…"}
          />
          <Button data-action="mcp-copy-command" onClick={() => void onCopyCommand()}>
            Copy command
          </Button>
        </span>
      </SettingsRow>

      <SettingsRow
        id="mcp.tools"
        align="start"
        caution="Nothing deletes a file, and a rename leaves other files pointing at the old name."
      >
        <Show
          when={activityStore.tools()}
          fallback={<span class="settings-programs-none">…</span>}
        >
          {(tools) => (
            <ul class="settings-tools">
              <li class="settings-tool-grant" data-grant="read">
                <span class="settings-tool-grant-name">Reading:</span>{" "}
                <span class="settings-tool-names">{describeReadTools(tools().read)}.</span>
              </li>
              <li class="settings-tool-grant" data-grant="write">
                <span class="settings-tool-grant-name">Writing:</span>{" "}
                <span class="settings-tool-names">{describeWriteTools(tools().write)}.</span>
              </li>
            </ul>
          )}
        </Show>
      </SettingsRow>

      <SettingsRow id="mcp.clients" align="start">
        <Show
          when={activityStore.clients().length > 0 || forgotten().length > 0}
          fallback={
            <span class="settings-programs-none">
              A program appears here after you approve its first call.
            </span>
          }
        >
          <ul class="settings-programs">
            <For each={activityStore.clients()}>
              {(client) => (
                <li class="settings-program" data-program={client.name}>
                  <div class="settings-program-row">
                    <span class="settings-program-name">{client.name}</span>
                    <span class="settings-program-grants">
                      <label class="settings-program-grant">
                        Read
                        <ToggleSwitch
                          setting={`mcp_read_${client.name}`}
                          label={`Let ${client.name} read your files`}
                          checked={client.read || client.write}
                          disabled={client.write}
                          onChange={() => void onSetPermission(client.name, !client.read, false)}
                        />
                      </label>
                      <label class="settings-program-grant">
                        Write
                        <ToggleSwitch
                          setting={`mcp_write_${client.name}`}
                          label={`Let ${client.name} write your files`}
                          checked={client.write}
                          onChange={() => void onSetPermission(client.name, true, !client.write)}
                        />
                      </label>
                      <Button data-action="mcp-forget" onClick={() => void onForget(client)}>
                        Forget
                      </Button>
                    </span>
                  </div>
                  <Show when={client.write}>
                    <span class="settings-program-note" data-program-note="write">
                      Writing includes reading.
                    </span>
                  </Show>
                </li>
              )}
            </For>
            <For each={forgotten()}>
              {(client) => (
                <li class="settings-program settings-program-forgotten" data-forgotten={client.name}>
                  <div class="settings-program-row">
                    <span class="settings-program-name">{client.name} was forgotten.</span>
                    <Button data-action="mcp-undo-forget" onClick={() => void onUndoForget(client)}>
                      Undo
                    </Button>
                  </div>
                </li>
              )}
            </For>
          </ul>
          <span class="settings-program-note" data-program-note="forget">
            Forget removes a program. It can ask again next time it connects.
          </span>
        </Show>
      </SettingsRow>

      <SettingsRow id="mcp.activity">
        <Button data-action="mcp-open-activity" onClick={() => props.onOpenActivity()}>
          Open
        </Button>
      </SettingsRow>
    </div>
  );
}
