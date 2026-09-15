import { For, Show, createMemo } from "solid-js";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import {
  chatStore,
  type ChatProposal,
  type DiffHunk,
  type DiffLine,
} from "../../stores/global/chat";

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

      <Show
        when={status() === "pending"}
        fallback={
          <p class="chat-proposal-verdict" role="status">
            {verdict(status(), refusal())}
          </p>
        }
      >
        <div class="chat-proposal-actions">
          <Button onClick={() => void chatStore.discard(props.turn, props.proposal)}>
            Discard
          </Button>
          <Button
            variant="primary"
            onClick={() => void chatStore.apply(props.turn, props.proposal)}
          >
            Apply
          </Button>
        </div>
      </Show>
    </section>
  );
}

function verdict(status: string, refusal: string | undefined): string {
  if (status === "applied") return "Applied.";
  if (status === "discarded") return "Discarded.";
  return refusal ?? "Not applied.";
}
