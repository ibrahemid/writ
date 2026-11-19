"use strict";

const { spawn } = require("node:child_process");

const command = process.env.JOTRA_COMMAND || process.env["{{LEGACY_COMMAND_VAR}}"];
if (!command) {
  process.stderr.write("jotra: JOTRA_COMMAND is not set in the extension's configuration\n");
  process.exit(1);
}

const child = spawn(command, ["mcp"], { stdio: "inherit", windowsHide: true });

child.on("error", (error) => {
  process.stderr.write(`jotra: ${command} could not be started: ${error.message}\n`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  process.exit(code ?? (signal ? 1 : 0));
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => child.kill(signal));
}
