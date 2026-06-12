# Writ + Claude Code integration

Writ works well as a scratchpad alongside Claude Code. Two patterns are useful: piping command output into a new buffer, and wiring a hook that opens the result of a tool run automatically.

## Prerequisites

Install the `writ` command from Settings → Files → "Install `writ` command", or run the symlink manually:

```sh
ln -sf "/Applications/Writ.app/Contents/MacOS/writ-aarch64-apple-darwin" /usr/local/bin/writ
```

Adjust the binary name for your architecture (`x86_64-apple-darwin` on Intel Macs).

## Pipe any output into Writ

```sh
# Pipe command output into a named buffer
cargo test 2>&1 | writ --title "test results"

# Pipe a file's contents (useful in scripts)
cat notes.md | writ --title "notes"

# Open specific files
writ src/main.rs Cargo.toml

# Open the current directory as a workspace
writ .
```

## Claude Code hook: open tool output automatically

Add a `PostToolUse` hook to `.claude/settings.json` in your project. The hook fires after each tool run and opens the output in Writ when the tool matches.

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.tool_response.stdout // empty' | writ --title \"bash output\""
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
    "PostToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "jq -r 'select(.tool_response.exit_code != 0) | .tool_response.stderr // .tool_response.stdout // empty' | writ --title \"error\""
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

A `Stop` hook lets you capture the final assistant message as a buffer for review or archiving:

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.assistant_message // empty' | writ --title \"session summary\""
          }
        ]
      }
    ]
  }
}
```

Hook input arrives on stdin as JSON. Use `jq` to extract the relevant field and pipe the result to `writ`. Empty output from `jq` is ignored by `writ` (no window opens for empty pipes).
