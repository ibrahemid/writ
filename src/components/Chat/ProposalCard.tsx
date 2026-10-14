import { For, Show, createMemo, createSignal } from "solid-js";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import {
  chatStore,
  proposalLabel,
  type ChatProposal,
  type ProposalVerdict,
} from "../../stores/global/chat";
import { askAgain } from "../../commands/chat";
import { hunkRows } from "../../lib/diff-rows";

/**
 * One offered change, read as a diff.
 *
 * The hunks are computed in Rust, where both texts already are, so the card
 * shows what would change rather than two full texts. Nothing here writes a
 * note: applying and undoing are a person's click that goes through the
 * guarded write like every other (ADR-031 rule 4.3).
 */
export default function ProposalCard(props: { turn: number; proposal: ChatProposal }) {
  const status = () => props.proposal.status ?? "pending";
  const refusal = () => chatStore.refusalFor(props.turn, props.proposal.path);
  const rows = createMemo(() => (props.proposal.hunks ?? []).map(hunkRows));
  // What this session's write did to the note, which the stored turn does not
  // record: it says the proposal was applied, not what the file now holds.
  const written = () => chatStore.verdictFor(props.turn, props.proposal.path);
  const [discarding, setDiscarding] = createSignal(false);

  const busy = () => chatStore.isApplying(props.turn, props.proposal.path);
  /** A refused offer stays on the card: the note it names is still the one the
   * reply meant, and the reason is usually something a person can answer. */
  const settled = () =>
    status() === "applied" || status() === "discarded" || written() !== undefined;
  /** The note moved on since the offer was made, so Apply would be refused
   * and would leave a dated copy beside it. */
  const stale = () => props.proposal.stale === true && !settled();

  /** A note by the key its folder gives it, a file outside the folder by its
   * own name: the whole path is in the title, which is where it fits. */
  const label = () => proposalLabel(props.proposal.path);

  async function discardIt() {
    setDiscarding(true);
    try {
      await chatStore.discard(props.turn, props.proposal);
    } finally {
      setDiscarding(false);
    }
  }

  return (
    <section class="chat-proposal" aria-label={`Change to ${label()}`}>
      <header class="chat-proposal-header">
        <Icon name="file-text" size={14} />
        {/* The tip carries the path only where the line does not. A note's
            line is its path already, and a tip repeating it says nothing. */}
        <Show
          when={label() !== props.proposal.path}
          fallback={<span class="chat-proposal-path">{label()}</span>}
        >
          <Tooltip label={props.proposal.path}>
            <span class="chat-proposal-path">{label()}</span>
          </Tooltip>
        </Show>
      </header>

      <Show when={props.proposal.summary.length > 0}>
        <p class="chat-proposal-summary">{props.proposal.summary}</p>
      </Show>

      <Show when={stale()}>
        <p class="chat-proposal-stale">The file changed since this was offered.</p>
      </Show>

      <Show when={rows().length > 0}>
        <div class="chat-diff">
          <For each={rows()}>
            {(hunk) => (
              <div class="chat-diff-hunk">
                <For each={hunk}>
                  {(row) => (
                    <div class="chat-diff-row" data-kind={row.kind}>
                      <span class="chat-diff-num">{row.before ?? ""}</span>
                      <span class="chat-diff-num">{row.after ?? ""}</span>
                      <span class="chat-diff-mark" aria-hidden="true">
                        {row.kind === "added" ? "+" : row.kind === "removed" ? "-" : " "}
                      </span>
                      <span class="chat-diff-text">{row.text}</span>
                    </div>
                  )}
                </For>
              </div>
            )}
          </For>
        </div>
      </Show>

      <Show when={settled() || status() === "refused"}>
        <p class="chat-proposal-verdict" role="status">
          {verdict(status(), refusal(), written(), stale(), label())}
        </p>
      </Show>

      <Show
        when={!settled()}
        fallback={
          <Show when={status() === "applied" || written() !== undefined}>
            <div class="chat-proposal-actions">
              <Button onClick={() => void chatStore.openTarget(props.proposal)}>Open</Button>
              <Show
                when={(() => {
                  const held = written();
                  return held?.kind === "applied" && held.outcome.undoable;
                })()}
              >
                <Button
                  disabled={busy()}
                  aria-busy={busy() ? true : undefined}
                  onClick={() => void chatStore.undo(props.turn, props.proposal)}
                >
                  Undo
                </Button>
              </Show>
            </div>
          </Show>
        }
      >
        <div class="chat-proposal-actions">
          <Button disabled={discarding() || busy()} onClick={() => void discardIt()}>
            Discard
          </Button>
          <Show when={stale()}>
            <Button onClick={() => void askAgain(props.proposal)}>Ask again</Button>
          </Show>
          <Button
            variant="primary"
            disabled={busy() || discarding() || stale()}
            aria-busy={busy() ? true : undefined}
            onClick={() => void chatStore.apply(props.turn, props.proposal)}
          >
            Apply
          </Button>
        </div>
      </Show>
    </section>
  );
}

function verdict(
  status: string,
  refusal: string | undefined,
  written: ProposalVerdict | undefined,
  stale: boolean,
  label: string,
): string {
  if (written?.kind === "undone") return `${label} is back as it was.`;
  // An Undo that was refused says why, under the write it could not undo.
  if (written && refusal) return refusal;
  if (written && !written.outcome.changed) return "The file already held this text.";
  if (written) return `Applied to ${label}.`;
  if (status === "applied") return "Applied.";
  if (status === "discarded") return "Discarded.";
  if (refusal) return refusal;
  return stale ? "Nothing was written." : "Not applied.";
}
