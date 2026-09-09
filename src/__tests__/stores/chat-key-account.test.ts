import { describe, it, expect } from "vitest";
import { AI_PRESETS, CHAT_PROVIDERS, chatKeyAccount } from "../../stores/global/config";

// The rewrite path stores its key under a bare preset id read straight from
// config.toml. The chat pane must never land on one of those accounts: a
// shared account would let the rewrite key row read "Key set" for a key
// entered in the chat row, and clearing either row would destroy the other's.

describe("the account a chat key is stored under", () => {
  it("is never a rewrite preset id", () => {
    const chatAccounts = CHAT_PROVIDERS.map(chatKeyAccount);
    for (const account of chatAccounts) {
      expect(AI_PRESETS).not.toContain(account);
    }
    for (const preset of AI_PRESETS) {
      expect(chatAccounts).not.toContain(preset);
    }
  });

  it("is one account per provider", () => {
    const accounts = CHAT_PROVIDERS.map(chatKeyAccount);
    expect(new Set(accounts).size).toBe(accounts.length);
  });

  it("no longer offers anthropic as a rewrite preset", () => {
    expect(AI_PRESETS).not.toContain("anthropic");
  });
});
