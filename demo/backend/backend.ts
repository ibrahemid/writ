import type { CommandArgs, CommandHandler, IpcBridge } from "../ipc";
import { createAiChatCommands } from "./commands/ai-chat";
import { createBufferCommands } from "./commands/buffers";
import { createConfigCommands } from "./commands/config";
import { createSearchCommands } from "./commands/search";
import { createVersionCommands } from "./commands/versions";
import { OPEN_AT_START } from "./seed";
import { BufferMissingError, DemoState, refuseOnThrow, type CommandTable } from "./state";
import { NOTES_ROOT } from "./vfs";

export class DemoCommandError extends Error {
  constructor(readonly command: string) {
    super(`the demo has no answer for ${command}`);
    this.name = "DemoCommandError";
  }
}

export class DuplicateCommandError extends Error {
  constructor(readonly command: string) {
    super(`two command tables answer ${command}`);
    this.name = "DuplicateCommandError";
  }
}

/** A missing buffer as the command rejects it. Anything else is a fault in the page, thrown on. */
function describeCommandFailure(error: unknown): string {
  if (error instanceof BufferMissingError) return error.message;
  throw error;
}

export class DemoBackendNotInstalledError extends Error {
  constructor() {
    super("the demo backend was not created when the IPC bridge was installed");
    this.name = "DemoBackendNotInstalledError";
  }
}

export interface DemoControls {
  resetVersions(path: string): Promise<void>;
}

export interface DemoBackend {
  handle: CommandHandler;
  controls: DemoControls;
}

function mergeCommandTables(tables: CommandTable[]): CommandTable {
  const merged: CommandTable = Object.create(null);
  for (const table of tables) {
    for (const [command, handler] of Object.entries(table)) {
      if (command in merged) throw new DuplicateCommandError(command);
      merged[command] = handler;
    }
  }
  return merged;
}

export function createBackend(bridge: IpcBridge): DemoBackend {
  const state = new DemoState(bridge);
  const handlers = mergeCommandTables([
    createConfigCommands(state),
    createBufferCommands(state),
    createVersionCommands(state),
    createSearchCommands(state),
    createAiChatCommands(state),
  ]);

  for (const relative of OPEN_AT_START) state.openPath(`${NOTES_ROOT}/${relative}`);

  const handle: CommandHandler = (cmd: string, args: CommandArgs) => {
    if (cmd.startsWith("plugin:")) return null;
    const handler = handlers[cmd];
    if (!handler) {
      console.warn(`[writ-demo] unhandled command ${cmd}`);
      throw new DemoCommandError(cmd);
    }
    return refuseOnThrow(() => handler(args ?? {}), describeCommandFailure);
  };

  return { handle, controls: { resetVersions: (path) => state.resetVersions(path) } };
}
