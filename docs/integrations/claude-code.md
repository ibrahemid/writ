# Jotra + Claude Code integration

Claude Code reaches Jotra two ways: as an MCP client of Connected programs, which searches, reads and, if you allow it, writes the files in Jotra's folder, and through the `jotra` command, which a pipe or a hook hands text to.

## Connect Claude Code over MCP

Switch on Settings, Apps, Connected programs. Under Connect a program, Add on the Claude Code card runs Claude Code's own command, at the user scope so Jotra is there in every project:

```sh
claude mcp add --scope user jotra -- "/Applications/Jotra.app/Contents/MacOS/jotra" mcp
```

When `claude` is not found, the card shows this line with the path of your install to copy into a terminal. Claude Code's first call then waits on the card and under Waiting for you: choose Approve reading, Approve reading and writing, or Don't allow. Test on the card starts the same command and reports how many tools it lists.

An entry added under the name `writ` keeps working under that name. Its command, `writ`, lists the tool `open_in_writ`, so permissions Claude Code holds for `mcp__writ__open_in_writ` still apply.

## Prerequisites for the `jotra` command

On macOS and Linux, install the `jotra` command from Settings, Integrations, Terminal command, or create the link yourself:

```sh
ln -sf "/Applications/Jotra.app/Contents/MacOS/jotra" /usr/local/bin/jotra
```

On Windows the installer puts `jotra.exe` on the PATH.

The `writ` command runs the same program: the Windows installer adds `writ.exe`, and a `writ` link made by an earlier version keeps working, so hooks that call `writ` need no change.

## Pipe any output into Jotra

```sh
# Save command output as a file named "test results" and open it
cargo test 2>&1 | jotra --title "test results"

# Pipe a file's contents from a script
cat draft.md | jotra --title "draft"

# Open specific files
jotra src/main.rs Cargo.toml

# Open the current folder
jotra .
```

## Claude Code hook: keep tool output in one file

Add a `PostToolUse` hook to `.claude/settings.json` in your project. After each Bash run the hook adds the output to the end of one file in Jotra's folder. `jotra append` opens no window, so a session of many runs leaves one file and Jotra stays where it is; Revert To, in the File menu, lists what the file held before each run.

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.tool_response.stdout // empty' | jotra append \"bash output\""
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
            "command": "jq -r '.error // empty' | jotra --title \"error\""
          }
        ]
      }
    ]
  }
}
```

## Hook: open generated files after Write

This hook opens any file written by the `Write` tool in Jotra immediately after it is created:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write",
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.tool_input.file_path // empty' | xargs -I{} jotra {}"
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
            "command": "jq -r '.last_assistant_message // empty' | jotra --title \"session summary\""
          }
        ]
      }
    ]
  }
}
```

Hook input arrives on stdin as JSON. Use `jq` to extract the relevant field and pipe the result to `jotra`. `jotra` ignores empty or whitespace-only input and opens no window for it.
