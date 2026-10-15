# Writ + Claude Code integration

Claude Code reaches Writ two ways: as an MCP client of Connected programs, which searches, reads and, if you allow it, writes the files in Writ's folder, and through the `writ` command, which a pipe or a hook hands text to.

## Connect Claude Code over MCP

Switch on Settings, Apps, Connected programs. Under Connect a program, Add on the Claude Code card runs Claude Code's own command, at the user scope so Writ is there in every project:

```sh
claude mcp add --scope user writ -- "/Applications/Writ.app/Contents/MacOS/writ" mcp
```

When `claude` is not found, the card shows this line with the path of your install to copy into a terminal. Claude Code's first call then waits on the card and under Waiting for you: choose Approve reading, Approve reading and writing, or Don't allow. Test on the card starts the same command and reports how many tools it lists.

## Prerequisites for the `writ` command

On macOS and Linux, install the `writ` command from Settings, Integrations, Terminal command, or create the link yourself:

```sh
ln -sf "/Applications/Writ.app/Contents/MacOS/writ" /usr/local/bin/writ
```

On Windows the installer puts `writ.exe` on the PATH.

## Pipe any output into Writ

```sh
# Save command output as a file named "test results" and open it
cargo test 2>&1 | writ --title "test results"

# Pipe a file's contents from a script
cat draft.md | writ --title "draft"

# Open specific files
writ src/main.rs Cargo.toml

# Open the current folder
writ .
```

## Claude Code hook: keep tool output in one file

Add a `PostToolUse` hook to `.claude/settings.json` in your project. After each Bash run the hook adds the output to the end of one file in Writ's folder. `writ append` opens no window, so a session of many runs leaves one file and Writ stays where it is; Revert To, in the File menu, lists what the file held before each run.

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.tool_response.stdout // empty' | writ append \"bash output\""
          }
        ]
      }
    ]
  }
}
```

To capture only failing runs:

```json
{
  "hooks": {
    "PostToolUseFailure": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.error // empty' | writ --title \"error\""
          }
        ]
      }
    ]
  }
}
```

## Hook: open generated files after Write

This hook opens any file written by the `Write` tool in Writ immediately after it is created:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write",
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.tool_input.file_path // empty' | xargs -I{} writ {}"
          }
        ]
      }
    ]
  }
}
```

## Stop hook: review session output

A `Stop` hook saves the final assistant message as a file:

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.last_assistant_message // empty' | writ --title \"session summary\""
          }
        ]
      }
    ]
  }
}
```

Hook input arrives on stdin as JSON. Use `jq` to extract the relevant field and pipe the result to `writ`. `writ` ignores empty or whitespace-only input and opens no window for it.
