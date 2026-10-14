// The browser stands in for the Tauri host: the same window.__TAURI_INTERNALS__
// surface the app's tests install, answered by the in-memory backend.

export type CommandArgs = Record<string, unknown> | undefined;
/**
 * Answers a command. `raw` says its body arrived as bytes rather than JSON,
 * which a handler measuring what the page sends needs to know.
 */
export type CommandHandler = (cmd: string, args: CommandArgs, raw?: boolean) => unknown;

/** What `invoke` passes beside a command's arguments. */
interface InvokeOptions {
  headers?: Record<string, string> | Headers;
}

/** The request header a raw-body save names its note in (`NOTE_ID_HEADER` in Rust). */
const NOTE_ID_HEADER = "writ-note-id";

/**
 * A command's arguments as the host reads them. A save sends the note's text
 * as a raw UTF-8 body and names the note in a header; the host reads both,
 * and the handlers take the result as the arguments they always had.
 */
function argsOf(body: unknown, options: InvokeOptions | undefined): CommandArgs {
  if (!ArrayBuffer.isView(body) && !(body instanceof ArrayBuffer)) return body as CommandArgs;
  const headers = options?.headers;
  const id = headers instanceof Headers ? headers.get(NOTE_ID_HEADER) : headers?.[NOTE_ID_HEADER];
  const bytes = body instanceof ArrayBuffer ? new Uint8Array(body) : new Uint8Array(body.buffer, body.byteOffset, body.byteLength);
  return { id, content: new TextDecoder().decode(bytes) };
}

type Callback = (data: unknown) => void;

interface ChannelLike {
  id: number;
}

export interface IpcBridge {
  emit(event: string, payload: unknown): void;
  send(channel: ChannelLike, index: number, message: unknown): void;
  end(channel: ChannelLike, index: number): void;
}

export function installIpc(handle: (bridge: IpcBridge) => CommandHandler): IpcBridge {
  const callbacks = new Map<number, Callback>();
  const listeners = new Map<string, number[]>();

  const runCallback = (id: number, data: unknown): void => {
    callbacks.get(id)?.(data);
  };

  const bridge: IpcBridge = {
    emit(event, payload) {
      for (const id of listeners.get(event) ?? []) runCallback(id, { event, id, payload });
    },
    send(channel, index, message) {
      runCallback(channel.id, { index, message });
    },
    end(channel, index) {
      runCallback(channel.id, { index, end: true });
    },
  };

  const commands = handle(bridge);

  const eventPlugin = (cmd: string, args: Record<string, unknown>): unknown => {
    const event = String(args.event);
    switch (cmd) {
      case "plugin:event|listen": {
        const handler = Number(args.handler);
        listeners.set(event, [...(listeners.get(event) ?? []), handler]);
        return handler;
      }
      case "plugin:event|unlisten": {
        listeners.set(
          event,
          (listeners.get(event) ?? []).filter((id) => id !== Number(args.eventId)),
        );
        return null;
      }
      case "plugin:event|emit":
      case "plugin:event|emit_to":
        bridge.emit(event, args.payload);
        return null;
      default:
        return null;
    }
  };

  const internals = {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { windowLabel: "main", label: "main" },
    },
    invoke: async (cmd: string, args?: unknown, options?: InvokeOptions) =>
      cmd.startsWith("plugin:event|")
        ? eventPlugin(cmd, (args as Record<string, unknown> | undefined) ?? {})
        : commands(cmd, argsOf(args, options), ArrayBuffer.isView(args) || args instanceof ArrayBuffer),
    transformCallback: (callback?: Callback, once = false): number => {
      const id = window.crypto.getRandomValues(new Uint32Array(1))[0];
      callbacks.set(id, (data) => {
        if (once) callbacks.delete(id);
        callback?.(data);
      });
      return id;
    },
    unregisterCallback: (id: number) => callbacks.delete(id),
    runCallback,
    callbacks,
    convertFileSrc: (path: string, protocol = "asset") =>
      `${protocol}://localhost/${encodeURIComponent(path)}`,
  };

  const host = window as unknown as Record<string, unknown>;
  host.__TAURI_INTERNALS__ = internals;
  host.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (_event: string, id: number) => callbacks.delete(id),
  };
  return bridge;
}
