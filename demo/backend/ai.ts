// The connection rules behind Chat, Rewrite and Connected programs, from
// writ_core::ai::providers, writ_core::polish, writ_core::chat and the ai,
// chat and activity commands. A web page reaches no model server and no
// program reaches a web page, so every request that would leave the machine
// fails the way the app's own does when nothing answers: at the address, as a
// refused connection.

import type {
  AiConnectionStatus,
  AiEndpointState,
  AiKeyState,
  AiProviderInfo,
  ChatEndpointState,
  ChatErrorFrame,
  McpServerCommand,
  McpTools,
  ModelCatalog,
  ModelListError,
  RequestIdentity,
} from "../../src/services/tauri";
import type { AiConfig } from "../../src/types/config";

/** writ_core::ai::providers::PROVIDERS. */
export const PROVIDERS: readonly AiProviderInfo[] = [
  { id: "ollama", label: "Ollama", group: "local", wire: "openai", base_url: "http://localhost:11434/v1", models_url: "http://localhost:11434/api/tags", key_page_url: null, default_model: "", needs_key: false, supports_connect: false, probe_port: 11434, curated_models: ["qwen3:4b", "qwen3:8b", "gemma3:4b", "llama3.2:3b"] },
  { id: "lmstudio", label: "LM Studio", group: "local", wire: "openai", base_url: "http://localhost:1234/v1", models_url: "http://localhost:1234/v1/models", key_page_url: null, default_model: "", needs_key: false, supports_connect: false, probe_port: 1234, curated_models: [] },
  { id: "anthropic", label: "Anthropic", group: "hosted", wire: "anthropic", base_url: "https://api.anthropic.com", models_url: "https://api.anthropic.com/v1/models", key_page_url: "https://console.anthropic.com/settings/keys", default_model: "claude-sonnet-5", needs_key: true, supports_connect: false, probe_port: null, curated_models: ["claude-sonnet-5", "claude-opus-5", "claude-haiku-4-5"] },
  { id: "openai", label: "OpenAI", group: "hosted", wire: "openai", base_url: "https://api.openai.com/v1", models_url: "https://api.openai.com/v1/models", key_page_url: "https://platform.openai.com/api-keys", default_model: "gpt-5-mini", needs_key: true, supports_connect: false, probe_port: null, curated_models: ["gpt-5-mini", "gpt-5", "gpt-4.1-mini"] },
  { id: "gemini", label: "Google Gemini", group: "hosted", wire: "openai", base_url: "https://generativelanguage.googleapis.com/v1beta/openai", models_url: "https://generativelanguage.googleapis.com/v1beta/models", key_page_url: "https://aistudio.google.com/apikey", default_model: "gemini-2.5-flash", needs_key: true, supports_connect: false, probe_port: null, curated_models: ["gemini-2.5-flash", "gemini-2.5-flash-lite"] },
  { id: "openrouter", label: "OpenRouter", group: "hosted", wire: "openai", base_url: "https://openrouter.ai/api/v1", models_url: "https://openrouter.ai/api/v1/models", key_page_url: "https://openrouter.ai/settings/keys", default_model: "meta-llama/llama-3.3-70b-instruct", needs_key: true, supports_connect: true, probe_port: null, curated_models: ["meta-llama/llama-3.3-70b-instruct", "openai/gpt-5-mini", "google/gemma-4-26b-a4b-it:free"] },
  { id: "groq", label: "Groq", group: "hosted", wire: "openai", base_url: "https://api.groq.com/openai/v1", models_url: "https://api.groq.com/openai/v1/models", key_page_url: "https://console.groq.com/keys", default_model: "llama-3.3-70b-versatile", needs_key: true, supports_connect: false, probe_port: null, curated_models: ["llama-3.3-70b-versatile", "llama-3.1-8b-instant"] },
  { id: "deepseek", label: "DeepSeek", group: "hosted", wire: "openai", base_url: "https://api.deepseek.com", models_url: "https://api.deepseek.com/models", key_page_url: "https://platform.deepseek.com/api_keys", default_model: "deepseek-flash", needs_key: true, supports_connect: false, probe_port: null, curated_models: ["deepseek-flash", "deepseek-v4-pro"] },
  { id: "mistral", label: "Mistral", group: "hosted", wire: "openai", base_url: "https://api.mistral.ai/v1", models_url: "https://api.mistral.ai/v1/models", key_page_url: "https://console.mistral.ai/api-keys", default_model: "mistral-small-latest", needs_key: true, supports_connect: false, probe_port: null, curated_models: ["mistral-small-latest", "mistral-large-latest"] },
  { id: "xai", label: "xAI", group: "hosted", wire: "openai", base_url: "https://api.x.ai/v1", models_url: "https://api.x.ai/v1/models", key_page_url: "https://console.x.ai", default_model: "grok-4", needs_key: true, supports_connect: false, probe_port: null, curated_models: ["grok-4", "grok-3-mini"] },
  { id: "together", label: "Together", group: "hosted", wire: "openai", base_url: "https://api.together.xyz/v1", models_url: "https://api.together.xyz/v1/models", key_page_url: "https://api.together.ai/settings/api-keys", default_model: "meta-llama/Llama-3.3-70B-Instruct-Turbo", needs_key: true, supports_connect: false, probe_port: null, curated_models: ["meta-llama/Llama-3.3-70B-Instruct-Turbo"] },
  { id: "fireworks", label: "Fireworks", group: "hosted", wire: "openai", base_url: "https://api.fireworks.ai/inference/v1", models_url: "https://api.fireworks.ai/inference/v1/models", key_page_url: "https://app.fireworks.ai/settings/users/api-keys", default_model: "accounts/fireworks/models/llama-v3p3-70b-instruct", needs_key: true, supports_connect: false, probe_port: null, curated_models: ["accounts/fireworks/models/llama-v3p3-70b-instruct"] },
  { id: "custom", label: "Custom (OpenAI-compatible)", group: "custom", wire: "openai", base_url: "", models_url: "", key_page_url: null, default_model: "", needs_key: false, supports_connect: false, probe_port: null, curated_models: [] },
];

/** writ_core::tools::{READ_TOOLS, WRITE_TOOLS}. */
export const MCP_TOOLS: McpTools = {
  read: ["list_notes", "search_notes", "read_note", "note_links", "note_backlinks", "note_properties", "note_tags", "folder_tags"],
  write: ["write_note", "create_note", "rename_note"],
};

/** activity::server_command_for over the `writ` binary bundled beside the app on macOS. */
export function mcpServerCommand(): McpServerCommand {
  const path = "/Applications/Writ.app/Contents/MacOS/writ";
  return { path, command: `"${path}" mcp` };
}

/** Why a request refused before it left, carried as the app's own sentence. */
export class AiRefusal extends Error {
  constructor(message: string) {
    super(message);
    this.name = "AiRefusal";
  }
}

export function provider(id: string): AiProviderInfo | undefined {
  return PROVIDERS.find((row) => row.id === id);
}

interface EndpointTarget {
  host: string;
  host_port: string;
  is_hosted: boolean;
  is_allowed: boolean;
}

const isLocalhost = (host: string) => ["localhost", "127.0.0.1", "::1", "[::1]"].includes(host);

/** polish::resolve_endpoint, or null for a base URL that does not parse. */
export function resolveEndpoint(baseUrl: string): EndpointTarget | null {
  let url: URL;
  try {
    url = new URL(baseUrl.trim());
  } catch {
    return null;
  }
  const host = url.hostname;
  if (!host) return null;
  const scheme = url.protocol.replace(/:$/, "");
  return {
    host,
    host_port: url.port ? `${host}:${url.port}` : host,
    is_hosted: !isLocalhost(host),
    is_allowed: scheme === "https" || (scheme === "http" && isLocalhost(host)),
  };
}

/** AiConfig::effective_base_url: the row's own address, or the typed one for `custom`. */
export function effectiveBaseUrl(ai: AiConfig): string {
  const row = provider(ai.provider);
  return row && row.base_url ? row.base_url : ai.base_url;
}

/** AiConfig::chat_model: the chat's own pick for this provider, else the shared model. */
export function chatModel(ai: AiConfig): string {
  const own = ai.chat.model && ai.chat.model_provider && ai.chat.model_provider === ai.provider;
  return own ? ai.chat.model : ai.model;
}

const isConsented = (ai: AiConfig, host: string) => ai.consented_hosts.includes(host);

const NO_KEY: AiKeyState = { is_set: false, memory_only: false };

/** ai::endpoint_state_from; a local endpoint never reports a key. */
export function endpointState(ai: AiConfig, key: AiKeyState): AiEndpointState {
  const target = resolveEndpoint(effectiveBaseUrl(ai));
  if (!target) {
    return { host: null, host_port: null, is_hosted: false, is_allowed: false, is_consented: false, provider: ai.provider, key_state: key };
  }
  return {
    host: target.host,
    host_port: target.host_port,
    is_hosted: target.is_hosted,
    is_allowed: target.is_allowed,
    is_consented: !target.is_hosted || isConsented(ai, target.host),
    provider: ai.provider,
    key_state: target.is_hosted ? key : NO_KEY,
  };
}

/** chat::endpoint_state_from. */
export function chatState(ai: AiConfig, key: AiKeyState): ChatEndpointState {
  const state = endpointState(ai, key);
  return {
    enabled: ai.chat.enabled,
    provider: ai.provider,
    model: chatModel(ai),
    host: state.host,
    host_port: state.host_port,
    is_hosted: state.is_hosted,
    is_allowed: state.is_allowed,
    is_consented: state.is_consented,
    key_state: state.key_state,
  };
}

/** ai_consent_host: the config with the connection's host recorded, or the refusal. */
export function consentHost(ai: AiConfig): AiConfig {
  const target = resolveEndpoint(effectiveBaseUrl(ai));
  if (!target) throw new AiRefusal("The base URL is not a valid URL.");
  if (!target.is_allowed) throw new AiRefusal("This base URL is not allowed. Use https, or http for localhost.");
  if (!target.is_hosted) throw new AiRefusal("This endpoint is on your machine; nothing is sent.");
  if (isConsented(ai, target.host)) return ai;
  return { ...ai, consented_hosts: [...ai.consented_hosts, target.host].sort() };
}

/** AiConfig::with_provider, seeded with no model: a page never holds a live list. */
export function withProvider(ai: AiConfig, id: string): AiConfig {
  const row = provider(id);
  if (!row) throw new AiRefusal("That provider is not one this version knows.");
  const keepsModel = row.group === "custom";
  const chatKeeps = Boolean(ai.chat.model && ai.chat.model_provider && ai.chat.model_provider === id);
  return {
    ...ai,
    provider: id,
    model: keepsModel ? ai.model : "",
    chat: {
      enabled: ai.chat.enabled,
      model: chatKeeps ? ai.chat.model : "",
      model_provider: chatKeeps ? ai.chat.model_provider : "",
    },
  };
}

function modelsUrl(ai: AiConfig, baseUrl: string): string | null {
  const row = provider(ai.provider);
  if (!row) return null;
  return row.models_url || `${baseUrl.replace(/\/+$/, "")}/models`;
}

/** ModelCatalog::fallback: the row's suggestions, stamped with why the list could not be read. */
function fallback(id: string, error: ModelListError): ModelCatalog {
  const curated = [...(provider(id)?.curated_models ?? [])];
  return { provider: id, models: curated, source: curated.length ? "curated" : "none", error };
}

/** ai_list_models: the gate first, then the request, which nothing answers. */
export function listModels(ai: AiConfig): ModelCatalog {
  const baseUrl = effectiveBaseUrl(ai);
  const base = resolveEndpoint(baseUrl);
  const url = modelsUrl(ai, baseUrl);
  const list = url === null ? null : resolveEndpoint(url);
  if (!base?.is_allowed || !list?.is_allowed) return fallback(ai.provider, { kind: "unreachable" });
  for (const reached of [base, list]) {
    if (reached.is_hosted && !isConsented(ai, reached.host)) return fallback(ai.provider, { kind: "consent_required" });
  }
  return fallback(ai.provider, { kind: "unreachable" });
}

const status = (reachable: boolean, kind: string, detail: string): AiConnectionStatus => ({
  reachable,
  model_listed: null,
  kind,
  detail,
  models: [],
});

/** ai_check_connection, in its gate order; a request that leaves is refused. */
export function checkConnection(ai: AiConfig, hasKey: boolean): AiConnectionStatus {
  const baseUrl = effectiveBaseUrl(ai);
  const target = resolveEndpoint(baseUrl);
  if (!target?.is_allowed) return status(false, "invalid_url", "");
  if (target.is_hosted && !isConsented(ai, target.host)) return status(false, "consent_required", target.host);
  if (target.is_hosted && !hasKey) return status(false, "key_required", target.host);
  const url = modelsUrl(ai, baseUrl);
  if (url === null || !resolveEndpoint(url)?.is_allowed) return status(false, "invalid_url", "");
  return status(false, "refused", target.host_port);
}

/** ai::prepare_request's refusals, in its order. Null when the request would be sent. */
export function rewriteRefusal(
  ai: AiConfig,
  action: string,
  text: string,
  instruction: string | null,
  hasKey: boolean,
): string | null {
  if (!ai.rewrite.enabled) return "Rewriting is turned off.";
  if (!["proofread", "rephrase", "polish", "improve_prompt", "custom"].includes(action)) {
    return `unknown rewrite action: ${action}`;
  }
  if (action === "custom" && !instruction?.trim()) return "a custom rewrite needs an instruction";
  if (!text.trim()) return "there is no text to rewrite";
  const target = resolveEndpoint(effectiveBaseUrl(ai));
  if (!target) return "The base URL is not a valid URL.";
  if (!target.is_allowed) return "This base URL is not allowed. Use https, or http for localhost.";
  if (!ai.model.trim()) return "Choose a model in AI settings.";
  if (target.is_hosted && !isConsented(ai, target.host)) return `Confirm sending text to ${target.host} first.`;
  if (target.is_hosted && !hasKey) return `Add an API key for ${target.host} first.`;
  return null;
}

/** ai::connection_error_message for a request nothing answered. */
export function rewriteStreamError(ai: AiConfig): string {
  const target = resolveEndpoint(effectiveBaseUrl(ai));
  return target && !target.is_hosted
    ? "Could not reach the local model server. Is Ollama running?"
    : "error sending request for url (<redacted-url>)";
}

/** chat::prepare_chat's refusals, in its order. Null when the request would be sent. */
export function chatRefusal(ai: AiConfig, text: string, hasKey: boolean): string | null {
  if (!ai.chat.enabled) return "Chat is turned off.";
  if (!text.trim()) return "there is nothing to send";
  const target = resolveEndpoint(effectiveBaseUrl(ai));
  if (!target) return "The chat base URL is not a valid URL.";
  if (!target.is_allowed) return "This base URL is not allowed. Use https, or http for localhost.";
  if (!chatModel(ai).trim()) return "Choose a chat model in AI settings.";
  if (target.is_hosted && !isConsented(ai, target.host)) return `Confirm sending your files to ${target.host} first.`;
  if (target.is_hosted && !hasKey) return `Add an API key for ${target.host} first.`;
  return null;
}

/** The connection a chat request is frozen against. */
export function requestIdentity(ai: AiConfig): RequestIdentity {
  return { provider: ai.provider, model: chatModel(ai), host: resolveEndpoint(effectiveBaseUrl(ai))?.host ?? "" };
}

/** chat::transport_frame for a request nothing answered. */
export function chatTransportFrame(ai: AiConfig): ChatErrorFrame {
  const identity = requestIdentity(ai);
  const target = resolveEndpoint(effectiveBaseUrl(ai));
  if (target && !target.is_hosted) {
    const label = provider(ai.provider)?.label ?? ai.provider;
    return {
      kind: "local_server_offline",
      message: `${label} is not running at ${target.host_port}.`,
      provider: identity.provider,
      model: identity.model,
      status: null,
    };
  }
  return {
    kind: "unreachable",
    message: "error sending request for url (<redacted-url>)",
    provider: identity.provider,
    model: identity.model,
    status: null,
  };
}
