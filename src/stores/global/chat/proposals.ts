import { createSignal } from "solid-js";
import { onAutosaveSuccess } from "../../../services/autosave";
import { onEvent, type UnlistenFn } from "../../../services/events";
import {
  chatApplyProposal,
  chatDiscardProposal,
  chatOpen,
  chatUndoProposal,
  type ChatConversation,
  type ChatProposal,
  type ChatProposalOutcome,
} from "../../../services/tauri";
import { bufferRegistry } from "../buffer-registry";
import { notesStore } from "../notes";
import { showToast } from "../notifications";
import { windowRegistry } from "../window-registry";
import { isAbsolutePath, proposalLabel } from "./attachments";
import { readableError } from "./errors";
import type { ChatView } from "./exchange";

/** What a proposal's card can say about the write this session made of it. */
export type ProposalVerdict =
  | { kind: "applied"; outcome: ChatProposalOutcome }
  | { kind: "undone"; outcome: ChatProposalOutcome };

/** An offer a person can still apply. */
function isOpen(proposal: ChatProposal): boolean {
  const status = proposal.status ?? "pending";
  return status === "pending" || status === "refused";
}

/** A path as a change is matched against an offer: one separator and one
 * case, so a spelling that differs only there still matches. A match too many
 * costs one read; one too few leaves Apply offering a write the guard refuses. */
function comparable(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

/** Whether a change at `changed` can move the file an offer names: the file
 * itself, or a folder holding it, which is what a walk or a move names. */
function changeTouches(changed: string, offered: string, root: string | null): boolean {
  const at = comparable(changed);
  if (at.length === 0) return false;
  const relative = !isAbsolutePath(offered);
  const whole = relative
    ? root
      ? `${comparable(root)}/${comparable(offered)}`
      : null
    : comparable(offered);
  if (whole !== null && (whole === at || whole.startsWith(`${at}/`))) return true;
  return relative && at.endsWith(`/${comparable(offered)}`);
}

function sameMeasure(held: ChatProposal, now: ChatProposal): boolean {
  return (
    (held.stale ?? false) === (now.stale ?? false) &&
    held.new_content === now.new_content &&
    JSON.stringify(held.hunks) === JSON.stringify(now.hunks)
  );
}

/** What proposals need from the facade around them. */
export interface ProposalHooks {
  /** Reads the chips' sizes again after a write changed a note. */
  refreshAttachedSizes: () => Promise<void>;
}

/**
 * Applying, discarding and undoing the changes a reply offered.
 *
 * Every write goes through the guarded facade (ADR-031 rule 4.3). What a write
 * did is kept for the session under the conversation it belongs to, so the
 * card and the toast that offer Undo answer the same write.
 */
export function createProposals(view: ChatView, hooks: ProposalHooks) {
  const [refusals, setRefusals] = createSignal<Record<string, string>>({});
  // Proposals whose write is in flight, keyed `${turn}:${path}`. Applying is
  // one-shot: a second press while the first write runs does nothing. It is a
  // signal rather than a plain set because the card's disabled state is read
  // from it, and a card that cannot see the write end stays dead.
  const [applying, setApplying] = createSignal<Record<string, true>>({});
  // What this session's writes did, keyed `${conversation}:${turn}:${path}`:
  // a turn number alone names a different turn in every conversation.
  const [verdicts, setVerdicts] = createSignal<Record<string, ProposalVerdict>>({});

  const turnKey = (turn: number, path: string) => `${turn}:${path}`;
  const writeKey = (id: string, turn: number, path: string) => `${id}:${turn}:${path}`;
  let measuring: Promise<void> | null = null;
  let measureAgain = false;
  let watching = false;

  /** Whether a change at `path` can move a file an offer on screen names. */
  function touchesOffer(path: string): boolean {
    const root = notesStore.root();
    return (view.current()?.turns ?? []).some((turn) =>
      turn.proposals.some(
        (proposal) => isOpen(proposal) && changeTouches(path, proposal.path, root),
      ),
    );
  }

  /** `held` with what `fresh` measured for each offer still open on both
   * sides: whether the note moved since the offer was made, and the diff and
   * text Apply would write. Status stays as this session left it, and an offer
   * whose write is in flight is left to that write. */
  function takeMeasures(held: ChatConversation, fresh: ChatConversation): ChatConversation {
    let moved = false;
    const turns = held.turns.map((turn, index) => {
      const again = fresh.turns[index]?.proposals ?? [];
      let changed = false;
      const proposals = turn.proposals.map((proposal) => {
        const now = again.find(
          (offer) => offer.path === proposal.path && offer.before_hash === proposal.before_hash,
        );
        if (
          !now ||
          !isOpen(proposal) ||
          !isOpen(now) ||
          isApplying(index, proposal.path) ||
          sameMeasure(proposal, now)
        ) {
          return proposal;
        }
        changed = true;
        return { ...proposal, stale: now.stale, hunks: now.hunks, new_content: now.new_content };
      });
      if (!changed) return turn;
      moved = true;
      return { ...turn, proposals };
    });
    return moved ? { ...held, turns } : held;
  }

  async function measureOnce(): Promise<void> {
    const id = view.current()?.id;
    if (!id || !view.current()?.turns.some((turn) => turn.proposals.some(isOpen))) return;
    let fresh: ChatConversation;
    try {
      fresh = await chatOpen(id);
    } catch {
      return;
    }
    view.setCurrent((held) => (held && held.id === id ? takeMeasures(held, fresh) : held));
  }

  /** Measures the open offers on screen against their notes again. One read
   * runs at a time, and a request while it runs gets one more read after it,
   * so the last change is always measured. */
  function remeasure(): Promise<void> {
    if (measuring) {
      measureAgain = true;
      return measuring;
    }
    measuring = (async () => {
      try {
        do {
          measureAgain = false;
          await measureOnce();
        } while (measureAgain);
      } finally {
        measuring = null;
      }
    })();
    return measuring;
  }

  /** Measures the offers again whenever a file one names changes: a save
   * Writ made (which the folder watcher drops), another program's write, and
   * a rename or restore Writ indexed. Subscribed once. */
  function watchOfferedNotes(): void {
    if (watching) return;
    watching = true;
    const changedAt = (path: string | null | undefined) => {
      if (path && touchesOffer(path)) void remeasure();
    };
    const listen = (subscribe: () => Promise<UnlistenFn>) => {
      try {
        void subscribe().catch(() => undefined);
      } catch {
        // A host with no event bus leaves the offers as the last read left them.
      }
    };
    onAutosaveSuccess((bufferId) =>
      changedAt(bufferRegistry.activeTabs().find((tab) => tab.id === bufferId)?.source_path),
    );
    listen(() => onEvent("notes:changed", (payload) => changedAt(payload?.path)));
    listen(() =>
      onEvent("notes:indexed", (payload) => {
        for (const path of payload?.paths ?? []) changedAt(path);
      }),
    );
    listen(() =>
      onEvent("buffer:external", (payload) => {
        changedAt(payload?.path);
        changedAt(payload?.newPath);
      }),
    );
  }

  /** Why applying was refused, for the proposal it was refused for. */
  function refusalFor(turn: number, path: string): string | undefined {
    return refusals()[turnKey(turn, path)];
  }

  /** Whether that proposal's write is in flight. */
  function isApplying(turn: number, path: string): boolean {
    return applying()[turnKey(turn, path)] === true;
  }

  /** What this session's write of that proposal did, if it made one. */
  function verdictFor(turn: number, path: string): ProposalVerdict | undefined {
    const id = view.current()?.id;
    return id ? verdicts()[writeKey(id, turn, path)] : undefined;
  }

  function endApplying(key: string) {
    setApplying(({ [key]: _done, ...rest }) => rest);
  }

  function setProposalStatus(turn: number, path: string, next: ChatProposal["status"]) {
    view.setCurrent((held) => {
      if (!held) return held;
      return {
        ...held,
        turns: held.turns.map((stored, index) =>
          index === turn
            ? {
                ...stored,
                proposals: stored.proposals.map((proposal) =>
                  proposal.path === path ? { ...proposal, status: next } : proposal,
                ),
              }
            : stored,
        ),
      };
    });
  }

  function refuse(turn: number, path: string, why: string) {
    setRefusals((held) => ({ ...held, [turnKey(turn, path)]: why }));
  }

  /** Forgets the refusals the view shows, for a view about to show another
   * conversation. What a write did is kept: it is keyed by conversation. */
  function clearRefusals() {
    setRefusals({});
  }

  /** Forgets everything, for a pane put back to holding nothing. */
  function reset() {
    setRefusals({});
    setApplying({});
    setVerdicts({});
  }

  /** Writes one proposal, through the guarded facade and nothing else.
   *
   * A note that changed since the proposal was made is refused there, and the
   * refusal is what the card then shows: the note is as its last writer left
   * it, and the proposed text is still in the conversation. A write that moved
   * bytes is announced with Undo, which is the same undo the card offers. */
  async function apply(turn: number, proposal: ChatProposal): Promise<ChatProposalOutcome | null> {
    const id = view.current()?.id;
    const key = turnKey(turn, proposal.path);
    if (!id) {
      refuse(turn, proposal.path, "This chat is no longer open.");
      return null;
    }
    // Applying is one-shot: a second press while the write runs does nothing.
    if (isApplying(turn, proposal.path)) return null;
    setApplying((held) => ({ ...held, [key]: true }));
    // A retry answers the refusal it was pressed under, so that refusal goes
    // before the write rather than after it.
    setRefusals(({ [key]: _gone, ...rest }) => rest);
    try {
      const outcome = await chatApplyProposal(
        id,
        turn,
        proposal.path,
        proposal.new_content,
        proposal.before_hash,
      );
      // The write is over before anything says so: the card reads both, and a
      // status written first would leave it holding a write that has ended.
      endApplying(key);
      setVerdicts((held) => ({
        ...held,
        [writeKey(id, turn, proposal.path)]: { kind: "applied", outcome },
      }));
      setProposalStatus(turn, proposal.path, "applied");
      if (outcome.changed) {
        // The note is a different size now, and a chip for it must state the
        // size the next message will carry.
        void hooks.refreshAttachedSizes();
        void remeasure();
        showToast({
          text: `Applied to ${proposalLabel(proposal.path)}.`,
          type: "success",
          ...(outcome.undoable
            ? { action: { label: "Undo", run: () => void undo(turn, proposal, id) } }
            : {}),
        });
      }
      return outcome;
    } catch (error) {
      endApplying(key);
      refuse(turn, proposal.path, readableError(error));
      setProposalStatus(turn, proposal.path, "refused");
      // A refusal for a note that moved is one every further Apply repeats,
      // each leaving another dated copy beside the note.
      void remeasure();
      return null;
    }
  }

  /** Puts back what the note held before this session applied the proposal.
   *
   * The text comes from the version the apply kept, and it goes back through
   * the same guarded write: a note that changed since the apply is refused
   * rather than overwritten. `conversationId` is the conversation the apply
   * was made in, which the toast's Undo still names after the pane moved on. */
  async function undo(
    turn: number,
    proposal: ChatProposal,
    conversationId: string | undefined = view.current()?.id,
  ): Promise<boolean> {
    if (!conversationId) return false;
    const key = writeKey(conversationId, turn, proposal.path);
    const held = verdicts()[key];
    if (held?.kind !== "applied" || !held.outcome.undoable) return false;
    const busy = turnKey(turn, proposal.path);
    if (applying()[busy]) return false;
    const onScreen = () => view.current()?.id === conversationId;
    setApplying((all) => ({ ...all, [busy]: true }));
    if (onScreen()) setRefusals(({ [busy]: _gone, ...rest }) => rest);
    try {
      const back = await chatUndoProposal(
        proposal.path,
        proposal.before_hash,
        held.outcome.hash,
      );
      endApplying(busy);
      setVerdicts((all) => ({ ...all, [key]: { kind: "undone", outcome: back } }));
      void hooks.refreshAttachedSizes();
      void remeasure();
      return true;
    } catch (error) {
      endApplying(busy);
      const why = readableError(error);
      if (onScreen()) refuse(turn, proposal.path, why);
      else showToast(why, "error");
      return false;
    }
  }

  /** Opens the file a proposal names in a tab. */
  async function openTarget(proposal: ChatProposal): Promise<void> {
    const root = notesStore.root();
    const path = isAbsolutePath(proposal.path)
      ? proposal.path
      : root
        ? `${root.replace(/[\\/]+$/, "")}/${proposal.path}`
        : null;
    if (!path) {
      showToast(`Could not open ${proposalLabel(proposal.path)}.`, "error");
      return;
    }
    await windowRegistry.getActive()?.tabs.openFile(path);
  }

  /** Puts a proposal aside. The file records the verdict, so the pane says so
   * once the record is written and not before. */
  async function discard(turn: number, proposal: ChatProposal) {
    const id = view.current()?.id;
    if (!id) {
      refuse(turn, proposal.path, "This chat is no longer open.");
      return;
    }
    try {
      await chatDiscardProposal(id, turn, proposal.path);
    } catch (error) {
      refuse(turn, proposal.path, readableError(error));
      return;
    }
    setProposalStatus(turn, proposal.path, "discarded");
  }

  return {
    refusalFor,
    isApplying,
    verdictFor,
    clearRefusals,
    reset,
    apply,
    undo,
    openTarget,
    discard,
    watchOfferedNotes,
  };
}

export type ChatProposals = ReturnType<typeof createProposals>;
