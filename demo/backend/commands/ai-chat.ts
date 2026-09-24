import type { ChatConversation, ChatConversationSummary } from "../../../src/services/tauri";
import * as ai from "../ai";
import { countUtf8Bytes } from "../history";
import { digestText, refuse, refuseOnThrow, type Answer, type CommandTable, type DemoState } from "../state";
import { NOTES_ROOT, basename } from "../vfs";

/** chat::MISSING_CONVERSATION. */
export const MISSING_CONVERSATION = "This chat no longer exists.";
/** chat::MAX_ATTACHED_NOTES. */
export const MAX_ATTACHED_NOTES = 20;

/** ChatStore::now: RFC 3339 in UTC. */
const formatNowRfc3339 = () => new Date().toISOString().replace("Z", "+00:00");

/** Conversation::title_from: the first non-empty line, at most 60 characters. */
function deriveChatTitle(text: string): string {
  const line = text
    .split("\n")
    .map((l) => l.trim())
    .find((l) => l !== "");
  return line ? [...line].slice(0, 60).join("") : "New chat";
}

const summarizeChat = (chat: ChatConversation): ChatConversationSummary => ({
  id: chat.id,
  title: chat.title,
  created_at: chat.created_at,
  updated_at: chat.updated_at,
  turns: chat.turns.length,
});

/** An AI refusal's own sentence. Anything else is a fault in the page, thrown on. */
function describeAiRefusal(error: unknown): string {
  if (error instanceof ai.AiRefusal) return error.message;
  throw error;
}

export function createAiChatCommands(state: DemoState): CommandTable {
  const { folder } = state;
  // Which providers were given a key this page. The key itself is not kept:
  // nothing here can send it anywhere.
  const keyed = new Set<string>();
  const chats = new Map<string, ChatConversation>();

  const getKeyState = (provider: string) => ({ is_set: keyed.has(provider), memory_only: keyed.has(provider) });

  /** chat's refusals for the files a message attaches, in its order. Null when every one can be read. */
  const findAttachRefusal = (paths: string[]): string | null => {
    if (paths.length > MAX_ATTACHED_NOTES) return `Attach at most ${MAX_ATTACHED_NOTES} files to one conversation.`;
    const missing = paths.find((path) => !folder.has(path));
    return missing === undefined ? null : `${basename(missing)} could not be read.`;
  };

  return {
    // Connection: rewriting and chat share it. Nothing a page sends reaches a model.
    ai_providers: (): Answer<"aiProviders"> => ai.PROVIDERS.map((row) => ({ ...row })),
    ai_endpoint_state: (): Answer<"aiEndpointState"> =>
      ai.getEndpointState(state.config.ai, getKeyState(state.config.ai.provider)),
    ai_consent_host: (): Answer<"aiConsentHost"> | Promise<never> =>
      refuseOnThrow(() => {
        const next = ai.consentHost(state.config.ai);
        if (next !== state.config.ai) {
          state.config = { ...state.config, ai: next };
          state.emitConfigChanged(["ai"]);
        }
        return ai.getEndpointState(state.config.ai, getKeyState(state.config.ai.provider));
      }, describeAiRefusal),
    ai_set_provider: (args): Answer<"aiSetProvider"> | Promise<never> =>
      refuseOnThrow(() => {
        state.config = { ...state.config, ai: ai.applyProvider(state.config.ai, String(args.provider)) };
        return structuredClone(state.config.ai);
      }, describeAiRefusal),
    ai_list_models: (): Answer<"aiListModels"> => ai.listModels(state.config.ai),
    ai_probe_local: (): Answer<"aiProbeLocal"> => ({ ollama: false, lmstudio: false }),
    ai_check_connection: (): Answer<"aiCheckConnection"> =>
      ai.checkConnection(state.config.ai, keyed.has(state.config.ai.provider)),
    ai_has_api_key: (args): Answer<"aiHasApiKey"> => getKeyState(String(args.provider)),
    ai_set_api_key: (args): Answer<"aiSetApiKey"> | Promise<never> => {
      const id = String(args.provider);
      if (!String(args.key ?? "")) return refuse("The API key is empty.");
      keyed.add(id);
      return getKeyState(id);
    },
    ai_clear_api_key: (args): Answer<"aiClearApiKey"> => {
      keyed.delete(String(args.provider));
      return getKeyState(String(args.provider));
    },
    ai_openrouter_connect: () => {
      const host = ai.resolveEndpoint(ai.findProvider("openrouter")?.base_url ?? "")?.host ?? "";
      if (!state.config.ai.consented_hosts.includes(host)) return refuse("OpenRouter is not allowed yet.");
      return refuse("OpenRouter did not accept the connection.");
    },
    ai_openrouter_cancel: () => null,
    ai_rewrite: (args) => {
      const requestId = String(args.requestId);
      const refusal = ai.findRewriteRefusal(
        state.config.ai,
        String(args.action),
        String(args.text),
        typeof args.customInstruction === "string" ? args.customInstruction : null,
        keyed.has(state.config.ai.provider),
      );
      if (refusal) return refuse(refusal);
      const text = ai.getRewriteStreamError(state.config.ai);
      setTimeout(() => {
        state.bridge.emit("writ://ai-rewrite", { kind: "ai:rewrite", payload: { request_id: requestId, kind: "error", text } });
      }, 0);
      return requestId;
    },
    ai_cancel: () => null,

    // Chat
    chat_state: (): Answer<"chatState"> => ai.getChatState(state.config.ai, getKeyState(state.config.ai.provider)),
    chat_list: (): Answer<"chatList"> =>
      [...chats.values()]
        .map(summarizeChat)
        .sort((a, b) => b.updated_at.localeCompare(a.updated_at) || a.id.localeCompare(b.id)),
    chat_new: (): Answer<"chatNew"> => {
      const now = formatNowRfc3339();
      const chat: ChatConversation = {
        id: crypto.randomUUID(),
        title: "New chat",
        created_at: now,
        updated_at: now,
        provider: state.config.ai.provider,
        model: ai.getChatModel(state.config.ai),
        turns: [],
      };
      chats.set(chat.id, chat);
      return structuredClone(chat);
    },
    chat_open: (args) => {
      const chat = chats.get(String(args.id));
      return chat ? structuredClone(chat) : refuse(MISSING_CONVERSATION);
    },
    chat_rename: (args) => {
      const chat = chats.get(String(args.id));
      if (!chat) return refuse(MISSING_CONVERSATION);
      const title = String(args.title).trim();
      if (title) chat.title = title;
      chat.updated_at = formatNowRfc3339();
      return structuredClone(chat);
    },
    chat_delete: (args) => (chats.delete(String(args.id)) ? null : refuse(MISSING_CONVERSATION)),
    chat_send: async (args): Promise<Answer<"chatSend">> => {
      const text = String(args.text);
      if (!text.trim()) return refuse("there is nothing to send");
      const chat = chats.get(String(args.conversationId));
      if (!chat) return refuse(MISSING_CONVERSATION);
      const refusal = ai.findChatRefusal(state.config.ai, text, keyed.has(state.config.ai.provider));
      if (refusal) return refuse(refusal);
      const contextPaths = (args.contextPaths as string[] | undefined) ?? [];
      const attachRefusal = findAttachRefusal(contextPaths);
      if (attachRefusal) return refuse(attachRefusal);
      const attached = await Promise.all(
        [...new Set(contextPaths)]
          .map(async (path) => {
            const note = folder.read(path);
            return { path, prompt_path: path.slice(NOTES_ROOT.length + 1), text: note, before_hash: await digestText(note) };
          }),
      );
      if (typeof args.truncateTo === "number") chat.turns = chat.turns.slice(0, args.truncateTo);
      if (chat.title === "New chat") chat.title = deriveChatTitle(text);
      chat.turns.push({
        role: "user",
        content: text,
        attachments: attached.map((note) => ({ path: note.path, bytes: countUtf8Bytes(note.text), hash: note.before_hash })),
        proposals: [],
      });
      chat.provider = state.config.ai.provider;
      chat.model = ai.getChatModel(state.config.ai);
      chat.updated_at = formatNowRfc3339();
      const requestId = String(args.requestId);
      const error = ai.buildChatTransportFrame(state.config.ai);
      setTimeout(() => {
        state.bridge.emit("writ://ai-chat", {
          kind: "ai:chat",
          payload: { conversation_id: chat.id, request_id: requestId, kind: "error", text: error.message, error },
        });
      }, 0);
      return { conversation_id: chat.id, request_id: requestId, attached, identity: ai.getRequestIdentity(state.config.ai) };
    },
    chat_attached_sizes: (args): Answer<"chatAttachedSizes"> | Promise<never> => {
      const paths = (args.paths as string[]).map(String);
      const attachRefusal = findAttachRefusal(paths);
      if (attachRefusal) return refuse(attachRefusal);
      const sizes: Answer<"chatAttachedSizes"> = [];
      for (const path of paths) {
        const key = path.slice(NOTES_ROOT.length + 1);
        if (sizes.some((size) => size.key === key)) continue;
        sizes.push({ path, key, bytes: countUtf8Bytes(folder.read(path)) });
      }
      return sizes;
    },
    chat_stop: () => null,

    // Connected programs: none has connected to a page.
    mcp_clients: (): Answer<"mcpClients"> => ({ approved: [], waiting: [] }),
    mcp_server_command: (): Answer<"mcpServerCommand"> => ai.getMcpServerCommand(),
    mcp_tools: (): Answer<"mcpTools"> => structuredClone(ai.MCP_TOOLS),
    activity_recent: () => [],
    activity_clear: () => null,
  };
}
