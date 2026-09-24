# Writ + Claude Code integration

The `writ` command connects Claude Code to Writ in two ways: piped output becomes a file in the Writ folder and opens in a tab, and a hook opens the result of a tool run as it happens.

## Prerequisites

On macOS and Linux, install the `writ` command from Settings, Advanced, Terminal command, or create the link yourself:

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
