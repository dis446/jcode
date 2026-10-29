# Project-defined tools (`.jcode/tools.json`)

A repository can declare local commands as first-class jcode tools, with the
model-facing name, description and JSON schema chosen by the repository. This is
the fork's C2 change; it exists so alpha's `feature_*` workflow tools are native
jcode tools instead of MCP tools with an `mcp__server__` prefix.

MCP remains a separate, fully supported tool source. Project tools are the
MCP-free path for a repo that only needs a few local commands.

## Files

Loaded in this order, per session attach:

1. `$JCODE_HOME/tools.json` (global; normally `~/.jcode/tools.json`)
2. `<session working dir>/.jcode/tools.json` (project)

A project entry replaces a global entry with the same name. Missing or malformed
files are ignored and never stop a session from starting. An unreadable file and
each invalid entry are logged as warnings and skipped individually.

Project tools are replaced on every session attach. Editing or deleting either
file takes effect on the next attach. A session that changes its working
directory drops the old directory's project tools and loads the new directory's.

A name that is already registered by something that is not a project tool
(built-in, MCP, self-dev) is not overridden; the entry is skipped with a warning.

## Format

```json
{
  "tools": [
    {
      "name": "feature_list",
      "description": "List alpha features.",
      "command": ["scripts/feature-workflow/feature-list"],
      "parameters": {"type": "object", "properties": {}},
      "timeout_secs": 120,
      "cwd": "project",
      "env": {}
    }
  ]
}
```

| Field | Required | Meaning |
| --- | --- | --- |
| `name` | yes | Tool name shown to the model. `[A-Za-z0-9_-]`, max 64 (provider limit). |
| `description` | yes | What the tool does and when to use it. |
| `command` | yes | argv array. jcode adds no shell; the command may itself be a shell. |
| `parameters` | no | JSON Schema; must be `{"type":"object",...}`. Defaults to an empty object. |
| `timeout_secs` | no | Kill the command after this many seconds. Default 120, clamped to 1..3600. Covers stdin, stdout, stderr and process wait. |
| `cwd` | no | `project`: the project root for the repo file (the directory containing `.jcode`), or the jcode config directory for the global file (`$JCODE_HOME`, normally `~/.jcode`). `session`: the session working dir. Default: `session` for the global file, `project` for the project file. |
| `env` | no | Extra environment variables for the command. |

The tool input object is written to the command's stdin as one JSON line.
jcode's generic `accept_large_output` flag is removed first. The generic
`intent` display label is also removed, unless the declared `parameters` schema
has its own `intent` property.

Exit 0 means success: stdout becomes the result, with stderr appended under
`[stderr]` when present. A non-zero exit is a tool error carrying the captured
output. A timeout is a tool error. Results are capped at 128 KiB; output beyond
the cap is drained and discarded, not buffered in memory.

A relative `command[0]` resolves against the config file's root (its real path,
so a symlinked `.jcode/tools.json` in a feature workspace runs the commands of
the project that owns the file). The root is the repo for the project file and
`$JCODE_HOME` for the global file. A bare name with no path separator falls back
to `PATH` when no local file of that name exists.

## Security

A project file is executable content, exactly like `AGENTS.md` plus `bash`: only
open a repo whose `.jcode/` you trust. The environment is inherited, and jcode's
`pre_tool` hooks see each call under the declared tool name with the tool input.

Project tools are not commands passed through the `bash` tool, so the bash
destructive-command gate does not inspect the argv they run. A repo can declare a
destructive tool. Gate those by tool name in `pre_tool` when they matter. For the
qLab tools, that means `feature_stop` and `feature_mr`.

## Example: qLab feature tools

`.jcode/tools.json` declares `feature_start`, `feature_continue`, `feature_mr`,
`feature_stop`, `feature_list` and `feature_link`. Each runs
`scripts/feature-workflow/jcode-tool <action>`, a small JSON-stdin adapter over
the existing scripts, which remain the source of truth.
