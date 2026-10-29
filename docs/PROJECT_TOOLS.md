# Project-defined tools (`.jcode/tools.json`)

A repository can declare local commands as first-class jcode tools, with the
model-facing name, description and JSON schema chosen by the repository. This is
the fork's C2 change; it exists so alpha's `feature_*` workflow tools are native
jcode tools instead of MCP tools with an `mcp__server__` prefix.

## Files

Loaded in this order, per session:

1. `~/.jcode/tools.json` (global)
2. `<session working dir>/.jcode/tools.json` (project)

A project entry replaces a global entry with the same name. Missing or malformed
files are ignored (a warning is logged); they never stop a session from starting.
Invalid entries are skipped individually. A name that is already registered
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
| `command` | yes | argv array. No shell is involved. |
| `parameters` | no | JSON Schema; must be `{"type":"object",...}`. Defaults to an empty object. |
| `timeout_secs` | no | Kill the command after this many seconds. Default 120, clamped to 1..3600. |
| `cwd` | no | `project` (default: the project root that owns the config file) or `session` (the session working dir). |
| `env` | no | Extra environment variables for the command. |

The tool input object is written to the command's stdin as one JSON line
(`accept_large_output` is removed first). Exit 0 means success: stdout becomes
the result, with stderr appended under `[stderr]` when present. A non-zero exit
or a timeout is a tool error carrying the captured output. Results are capped at
128 KiB.

A relative `command[0]` resolves against the **project root** that owns the
config file (its real path, so a symlinked `.jcode/tools.json` in a feature
workspace runs the commands of the project that owns the file). A bare name with
no path separator falls back to `PATH` when no local file of that name exists.

## Security

A project file is executable content, exactly like `AGENTS.md` plus `bash`: only
open a repo whose `.jcode/` you trust. There is no shell interpretation, the
environment is inherited, and jcode's `pre_tool` hooks still see each call under
the declared tool name.

## Example: qLab feature tools

`.jcode/tools.json` declares `feature_start`, `feature_continue`, `feature_mr`,
`feature_stop`, `feature_list` and `feature_link`. Each runs
`scripts/feature-workflow/jcode-tool <action>`, a small JSON-stdin adapter over
the existing scripts, which remain the source of truth.
