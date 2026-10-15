import { For, onMount, Show } from "solid-js";

import Button from "../../Button/Button";
import { programName, WaitingChoices } from "../../Activity/WaitingProgram";
import { showToast } from "../../../stores/global/notifications";
import { activityStore } from "../../../stores/global/activity";
import {
  mcpConnectStore,
  type CardStanding,
  type McpClientCard,
} from "../../../stores/global/mcp-connect";
import { mcpClientLabel } from "../../../lib/mcp-clients";
import "./ClientCards.css";

/** What the program may do in Writ now, in one line. */
function accessLine(standing: CardStanding): string {
  switch (standing.kind) {
    case "waiting":
      return "Waiting for your approval. No access until then.";
    case "decided": {
      const { read, write } = standing.approval;
      if (write) return "Can read and write the files in your folder.";
      if (read) return "Can read the files in your folder. Writing is off.";
      return "Not allowed to read or write your files.";
    }
    case "expecting":
      return "Waiting for the first call.";
    case "new":
      return "No access until you approve its first call.";
  }
}

/** "Don't allow" on a card: the program leaves the waiting list, so the
 * Undo the list keeps inline rides on a notice instead. */
function onDenied(name: string): void {
  showToast({
    text: `${programName(name)} is not allowed.`,
    type: "info",
    action: {
      label: "Undo",
      run: async () => {
        try {
          await activityStore.undoDenial(name);
        } catch {
          showToast("Could not undo the change", "error");
        }
      },
    },
  });
}

/** What Add does for this card, when the card shows no command to read. */
function howLine(card: McpClientCard): string | null {
  switch (card.connection) {
    case "link":
      return `Add opens ${mcpClientLabel(card.client)}, which asks to install Writ.`;
    case "bundle":
      return "Add opens Writ.mcpb in Claude Desktop, which asks to install it.";
    case "json":
      return "Paste this into the program's MCP settings.";
    case "command":
      return card.runs_command ? null : "Not found on this computer. Run this in a terminal.";
  }
}

function ClientCard(props: { card: McpClientCard; blocked: boolean }) {
  const id = () => props.card.client;
  const label = () => mcpClientLabel(id());
  const standing = () => mcpConnectStore.standing(id());
  const action = () => mcpConnectStore.actionOf(id());
  const test = () => mcpConnectStore.testOf(id());
  const canAdd = () =>
    props.card.connection === "link" || props.card.connection === "bundle" || props.card.runs_command;
  const canCopy = () => props.card.copy_text !== null;
  const waitingClient = () => {
    const now = standing();
    return now.kind === "waiting" ? now.client : null;
  };
  const addFailure = () => {
    const now = action();
    return now.phase === "failed" ? now.message : null;
  };
  const testLine = () => {
    const now = test();
    if (now.phase === "passed") {
      return `Writ answered with ${now.tools} ${now.tools === 1 ? "tool" : "tools"}.`;
    }
    return now.phase === "failed" ? now.message : null;
  };

  async function onCopy() {
    try {
      await mcpConnectStore.copy(props.card);
      showToast(props.card.connection === "json" ? "JSON copied" : "Command copied", "success");
    } catch {
      showToast("Could not copy", "error");
    }
  }

  return (
    <li class="client-card" data-client={id()}>
      <span class="client-card-name">{label()}</span>
      <span class="client-card-access" data-standing={standing().kind} aria-live="polite">
        {accessLine(standing())}
      </span>
      <Show when={waitingClient()}>
        {(client) => <WaitingChoices client={client()} onDenied={onDenied} />}
      </Show>
      <Show when={howLine(props.card)}>
        {(line) => <span class="client-card-how">{line()}</span>}
      </Show>
      <Show when={props.card.copy_text}>
        {(text) => (
          <code
            class="client-card-text"
            classList={{ "client-card-json": props.card.connection === "json" }}
            data-copy-text
          >
            {text()}
          </code>
        )}
      </Show>
      <span class="client-card-actions">
        <Show when={canAdd()}>
          <Button
            variant="primary"
            data-action="mcp-card-add"
            aria-label={`Add Writ to ${label()}`}
            aria-busy={action().phase === "working"}
            disabled={props.blocked || action().phase === "working"}
            onClick={() => void mcpConnectStore.add(props.card)}
          >
            Add
          </Button>
        </Show>
        <Show when={canCopy()}>
          <Button
            data-action="mcp-card-copy"
            aria-label={`Copy the ${props.card.connection === "json" ? "JSON" : "command"} for ${label()}`}
            disabled={props.blocked}
            onClick={() => void onCopy()}
          >
            Copy
          </Button>
        </Show>
        <Button
          variant="ghost"
          data-action="mcp-card-test"
          aria-label={`Test the command ${label()} runs`}
          aria-busy={test().phase === "testing"}
          disabled={props.blocked || test().phase === "testing"}
          onClick={() => void mcpConnectStore.test(id())}
        >
          Test
        </Button>
      </span>
      <span class="client-card-result" aria-live="polite">
        <Show when={addFailure()}>
          {(message) => (
            <span class="client-card-line" data-result="add" data-tone="error">
              {message()}
            </span>
          )}
        </Show>
        <Show when={testLine()}>
          {(line) => (
            <span
              class="client-card-line"
              data-result="test"
              data-tone={test().phase === "passed" ? "success" : "error"}
            >
              {line()}
            </span>
          )}
        </Show>
      </span>
    </li>
  );
}

/**
 * One card per MCP client: Add puts Writ in that program the way the program
 * documents (its own command, its install link, a bundle it installs), Copy
 * hands over the command or the JSON, and Test starts the command the program
 * is given and counts the tools it lists. Each card then follows the program
 * through the same lists the section shows, from waiting for its first call
 * to the approval choices to what it may do (ADR-031 rule 2.4).
 */
export default function ClientCards() {
  onMount(() => {
    void mcpConnectStore.load();
  });

  const blocked = () => activityStore.commandCaution() !== undefined;

  return (
    <Show
      when={mcpConnectStore.cards()}
      fallback={
        <span class="settings-programs-none">
          {mcpConnectStore.loadFailed() ? "The programs could not be listed." : "…"}
        </span>
      }
    >
      {(cards) => (
        <ul class="client-cards">
          <For each={cards()}>{(card) => <ClientCard card={card} blocked={blocked()} />}</For>
        </ul>
      )}
    </Show>
  );
}
