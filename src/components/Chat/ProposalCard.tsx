import { For, Show, createMemo, createSignal } from "solid-js";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import {
  chatStore,
  type ChatProposal,
  type ChatProposalOutcome,
  type DiffHunk,
  type DiffLine,
} from "../../stores/global/chat";
import { byteLabel } from "../../commands/chat";

/** One diff row, with the line number each side gives it. */
interface Row {
  kind: DiffLine["kind"];
  text: string;
  before: number | null;
  after: number | null;
}

/** Numbers the lines of one hunk, counting each side only where it has one. */
export function hunkRows(hunk: DiffHunk): Row[] {
  let before = hunk.before_start;
  let after = hunk.after_start;
  return hunk.lines.map((line) => {
    const row: Row = {
      kind: line.kind,
      text: line.text,
      before: line.kind === "added" ? null : before,
      after: line.kind === "removed" ? null : after,
    };
    if (line.kind !== "added") before += 1;
    if (line.kind !== "removed") after += 1;
    return row;
  });
}

/**
 * One offered change, read as a diff.
 *
 * The hunks are computed in Rust, where both texts already are, so the card
 * shows what would change rather than two full texts. Nothing here writes a
 * note: applying is a person's click that goes through the guarded write like
 * every other (ADR-031 rule 4.3).
 */
export default function ProposalCard(props: { turn: number; proposal: ChatProposal }) {
  const status = () => props.proposal.status ?? "pending";
  const refusal = () => chatStore.refusalFor(props.turn, props.proposal.path);
  const rows = createMemo(() => (props.proposal.hunks ?? []).map(hunkRows));
  // What the write did to the note, which only this card saw: the stored turn
  // records that it was applied, not how many bytes the file ended up holding.
  const [outcome, setOutcome] = createSignal<ChatProposalOutcome | null>(null);
  const [discarding, setDiscarding] = createSignal(false);

  const applying = () => chatStore.isApplying(props.turn, props.proposal.path);
  /** A refused offer stays on the card: the note it names is still the one the
   * reply meant, and the reason is usually something a person can answer. */
  const settled = () =>
    status() === "applied" || status() === "discarded" || outcome() !== null;

  async function applyIt() {
    const done = await chatStore.apply(props.turn, props.proposal);
    if (done) setOutcome(done);
  }

  async function discardIt() {
    setDiscarding(true);
    try {
      await chatStore.discard(props.turn, props.proposal);
    } finally {
      setDiscarding(false);
    }
  }

  return (
    <section class="chat-proposal" aria-label={`Change to ${props.proposal.path}`}>
      <header class="chat-proposal-header">
        <Icon name="file-text" size={14} />
        <span class="chat-proposal-path">{props.proposal.path}</span>
      </header>

      <Show when={props.proposal.summary.length > 0}>
        <p class="chat-proposal-summary">{props.proposal.summary}</p>
      </Show>

      <Show when={props.proposal.stale}>
        <p class="chat-proposal-stale">The note changed since this was offered.</p>
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
          {verdict(status(), refusal(), outcome(), props.proposal.stale === true)}
        </p>
      </Show>

      <Show when={!settled()}>
        <div class="chat-proposal-actions">
          <Button disabled={discarding() || applying()} onClick={() => void discardIt()}>
            Discard
          </Button>
          <Button
            variant="primary"
            disabled={applying() || discarding()}
            aria-busy={applying() ? true : undefined}
            onClick={() => void applyIt()}
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
  outcome: ChatProposalOutcome | null,
  stale: boolean,
): string {
  if (outcome && !outcome.changed) return "The note already held this text.";
  if (outcome) return `Applied. The note is now ${byteLabel(outcome.bytes)}.`;
  if (status === "applied") return "Applied.";
  if (status === "discarded") return "Discarded.";
  if (refusal) return refusal;
  return stale ? "Nothing was written." : "Not applied.";
}
