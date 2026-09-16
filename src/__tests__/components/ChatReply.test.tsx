import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// A reply is model output. It is rendered into the pane's own element, never
// the preview iframe; a link in it opens where every other external link
// opens, and only after a click; and every code block can be copied as the
// source the model wrote.

const mocks = vi.hoisted(() => ({
  openExternalUrl: vi.fn(),
  writeClipboardText: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  openExternalUrl: mocks.openExternalUrl,
  classifyExternalUrl: vi.fn(),
  noteNameCandidates: vi.fn(),
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn(),
  chatAttachedSizes: vi.fn(),
  chatList: vi.fn(),
  chatOpen: vi.fn(),
  chatNew: vi.fn(),
  chatRename: vi.fn(),
  chatDelete: vi.fn(),
  chatRenderReply: vi.fn(),
  chatSend: vi.fn(),
  chatStop: vi.fn(),
  chatState: vi.fn(),
}));

vi.mock("../../services/clipboard", () => ({
  writeClipboardText: mocks.writeClipboardText,
}));

import ChatTurn from "../../components/Chat/ChatTurn";
import ProposalCard from "../../components/Chat/ProposalCard";
import { hunkRows } from "../../components/Chat/ProposalCard";
import type { Message } from "../../stores/global/chat";

function reply(html: string): Message {
  return {
    turn: 1,
    role: "assistant",
    content: "",
    html,
    attachments: [],
    proposals: [],
    dropped: [],
    truncated: false,
    identity: null,
  };
}

beforeEach(() => {
  mocks.openExternalUrl.mockReset().mockResolvedValue(undefined);
  mocks.writeClipboardText.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
});

describe("a rendered reply", () => {
  it("puts one copy button on every code block, and copies the source", async () => {
    const { container } = render(() => (
      <ChatTurn
        message={reply(
          '<p>two ways</p><pre><code class="language-rust">let x = 1;</code></pre>' +
            "<pre><code>echo hi</code></pre>",
        )}
        thinking={false}
      />
    ));

    const buttons = container.querySelectorAll(".chat-code-copy");
    expect(buttons).toHaveLength(2);

    fireEvent.click(buttons[0]);
    await waitFor(() => expect(mocks.writeClipboardText).toHaveBeenCalledWith("let x = 1;"));
    await waitFor(() => expect(buttons[0].textContent).toBe("Copied"));

    fireEvent.click(buttons[1]);
    await waitFor(() => expect(mocks.writeClipboardText).toHaveBeenLastCalledWith("echo hi"));
  });

  it("opens an http link outside, and never navigates", () => {
    const { container } = render(() => (
      <ChatTurn
        message={reply('<p><a href="https://writ.md/docs">the docs</a></p>')}
        thinking={false}
      />
    ));

    const link = container.querySelector("a") as HTMLAnchorElement;
    const click = new MouseEvent("click", { bubbles: true, cancelable: true });
    link.dispatchEvent(click);

    expect(click.defaultPrevented).toBe(true);
    expect(mocks.openExternalUrl).toHaveBeenCalledWith("https://writ.md/docs");
  });

  // The renderer keeps http, https and mailto and drops the rest, so the pane
  // opens the same three and nothing else.
  it("opens a mailto link outside as well", () => {
    const { container } = render(() => (
      <ChatTurn
        message={reply('<p><a href="mailto:x@example.com">the maintainer</a></p>')}
        thinking={false}
      />
    ));

    const link = container.querySelector("a") as HTMLAnchorElement;
    const click = new MouseEvent("click", { bubbles: true, cancelable: true });
    link.dispatchEvent(click);

    expect(click.defaultPrevented).toBe(true);
    expect(mocks.openExternalUrl).toHaveBeenCalledWith("mailto:x@example.com");
  });

  it("does nothing for a script link", () => {
    const { container } = render(() => (
      <ChatTurn
        message={reply('<p><a href="javascript:alert(1)">a trick</a></p>')}
        thinking={false}
      />
    ));

    const link = container.querySelector("a") as HTMLAnchorElement;
    const click = new MouseEvent("click", { bubbles: true, cancelable: true });
    link.dispatchEvent(click);

    expect(click.defaultPrevented).toBe(true);
    expect(mocks.openExternalUrl).not.toHaveBeenCalled();
  });

  it("does nothing for a link that is not http", () => {
    const { container } = render(() => (
      <ChatTurn message={reply('<p><a href="file:///etc/passwd">a file</a></p>')} thinking={false} />
    ));

    const link = container.querySelector("a") as HTMLAnchorElement;
    const click = new MouseEvent("click", { bubbles: true, cancelable: true });
    link.dispatchEvent(click);

    expect(click.defaultPrevented).toBe(true);
    expect(mocks.openExternalUrl).not.toHaveBeenCalled();
  });

  it("puts a table in a box of its own so a wide one scrolls", () => {
    const { container } = render(() => (
      <ChatTurn
        message={reply("<table><tbody><tr><td>a column</td></tr></tbody></table>")}
        thinking={false}
      />
    ));

    const table = container.querySelector("table") as HTMLElement;
    expect(table.parentElement?.className).toBe("chat-reply-table");
    expect(container.querySelectorAll(".chat-reply-table")).toHaveLength(1);
  });

  it("shows a waiting reply as a thinking line", () => {
    const { container } = render(() => <ChatTurn message={reply("")} thinking={true} />);
    expect(container.querySelector(".chat-thinking")?.textContent).toContain("Thinking");
  });
});

describe("a proposal card", () => {
  const HUNK = {
    before_start: 4,
    after_start: 4,
    lines: [
      { kind: "context" as const, text: "the line before" },
      { kind: "removed" as const, text: "one intro" },
      { kind: "added" as const, text: "one intro, folded" },
      { kind: "context" as const, text: "the line after" },
    ],
  };

  it("numbers each side only where it has a line", () => {
    expect(hunkRows(HUNK)).toEqual([
      { kind: "context", text: "the line before", before: 4, after: 4 },
      { kind: "removed", text: "one intro", before: 5, after: null },
      { kind: "added", text: "one intro, folded", before: null, after: 5 },
      { kind: "context", text: "the line after", before: 6, after: 6 },
    ]);
  });

  it("says when the note moved on since the offer", () => {
    const { container } = render(() => (
      <ProposalCard
        turn={1}
        proposal={{
          path: "Ideas/Launch.md",
          summary: "Fold the two intros together",
          before_hash: "before",
          new_content: "the whole text",
          hunks: [HUNK],
          status: "pending",
          stale: true,
        }}
      />
    ));

    expect(container.querySelector(".chat-proposal-stale")?.textContent).toBe(
      "The note changed since this was offered.",
    );
    expect(container.querySelectorAll(".chat-diff-row")).toHaveLength(4);
    expect(container.textContent).not.toContain("the whole text");
  });
});
