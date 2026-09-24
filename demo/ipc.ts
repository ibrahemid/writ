// The browser stands in for the Tauri host: the same window.__TAURI_INTERNALS__
// surface the app's tests install, answered by the in-memory backend.

export type CommandArgs = Record<string, unknown> | undefined;
export type CommandHandler = (cmd: string, args: CommandArgs) => unknown;

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
    invoke: async (cmd: string, args?: Record<string, unknown>) =>
      cmd.startsWith("plugin:event|") ? eventPlugin(cmd, args ?? {}) : commands(cmd, args),
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
