import type { AiKeyState } from "../services/tauri";
import type { DictationEngine } from "./config";

export type { DictationEngine, DictationProvider, DictationConfig } from "./config";

export type DictationErrorCode =
  | "disabled"
  | "busy"
  | "mic_denied"
  | "silence"
  | "no_input_device"
  | "capture_failed"
  | "endpoint_not_allowed"
  | "model_required"
  | "consent_required"
  | "key_required"
  | "key_shared"
  | "unauthorized"
  | "model_not_found"
  | "rate_limited"
  | "server"
  | "refused"
  | "network"
  | "unsupported_os"
  | "language_unsupported"
  | "assets_failed"
  | "engine_failed"
  | "no_speech";

/** The button a failure offers next to its message. */
export type DictationAction = "open_mic_settings" | "open_dictation_settings";

/** Why a session did not start or stopped. Rust writes `message`. */
export interface DictationFailure {
  code: DictationErrorCode;
  message: string;
  action: DictationAction | null;
}

/** One message on a session's channel, in the order Rust sends them. */
export type DictationEvent =
  | { kind: "preparing"; progress: number | null }
  | { kind: "recording" }
  | { kind: "level"; level: number }
  | { kind: "partial"; text: string }
  | { kind: "transcribing"; pending: number }
  | { kind: "segment"; seq: number; text: string }
  | { kind: "notice"; message: string }
  | { kind: "done" }
  | { kind: "cancelled" }
  | { kind: "error"; failure: DictationFailure };

/** What this machine can do, and the engine an unset `engine` resolves to. */
export interface DictationCapabilities {
  on_device: boolean;
  on_device_languages: string[];
  system_language: string;
  mic_settings: boolean;
  effective_engine: DictationEngine;
}

/** Where the API engine sends audio and what it still needs. */
export interface DictationEndpointState {
  host: string;
  is_hosted: boolean;
  is_allowed: boolean;
  is_consented: boolean;
  needs_key: boolean;
  key_shared_with_ai: boolean;
  key: AiKeyState;
  key_page: string | null;
}

export interface DictationSession {
  session_id: number;
  engine: DictationEngine;
  locale: string | null;
}

export interface DictationLanguage {
  code: string;
  name: string;
}

const ERROR_CODES: ReadonlySet<string> = new Set<DictationErrorCode>([
  "disabled",
  "busy",
  "mic_denied",
  "silence",
  "no_input_device",
  "capture_failed",
  "endpoint_not_allowed",
  "model_required",
  "consent_required",
  "key_required",
  "key_shared",
  "unauthorized",
  "model_not_found",
  "rate_limited",
  "server",
  "refused",
  "network",
  "unsupported_os",
  "language_unsupported",
  "assets_failed",
  "engine_failed",
  "no_speech",
]);

/** Whether a rejected start carries Rust's structured failure. */
export function isDictationFailure(value: unknown): value is DictationFailure {
  if (typeof value !== "object" || value === null) return false;
  const failure = value as Record<string, unknown>;
  return (
    typeof failure.code === "string" &&
    ERROR_CODES.has(failure.code) &&
    typeof failure.message === "string" &&
    (failure.action === null ||
      failure.action === "open_mic_settings" ||
      failure.action === "open_dictation_settings")
  );
}
