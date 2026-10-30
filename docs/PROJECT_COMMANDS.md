# Project-defined slash commands (`.jcode/commands.json`)

A repository can declare deterministic, user-invoked slash commands next to the
project tools it already declares. Typing the command runs one shell line and
prints its output in the transcript. Unlike a skill, a command starts no model
turn: no provider tokens, no tool call, and no dependence on the session's tool
list. This is the fork's C8 change.

## Files

Loaded in this order, per session working directory:

1. `$JCODE_HOME/commands.json` (global; normally `~/.jcode/commands.json`)
2. `<session working dir>/.jcode/commands.json` (project)

A project entry replaces a global entry with the same name. Missing or malformed
files are ignored and never stop a session from starting. An unreadable file and
each invalid entry are logged as warnings and skipped individually.

The loader reads only regular files, up to 1 MiB per file. The merged set is
capped at 128 commands. Descriptions are capped at 4 KiB and command lines at
8 KiB. Nothing is executed at load time.

The list is resolved from the session working directory on first use and
re-resolved when that directory changes (`/new`, `/resume`, a working-directory
switch). Editing the file takes effect on the next session that resolves it.

A name that collides with a built-in slash command is skipped with a warning.
Built-ins always win. A project command is preferred over a skill with the same
name.

## Format

```json
{
  "commands": [
    {
      "name": "feature-start",
      "description": "Start an alpha feature: /feature-start <name> [repo ...]",
      "command": "scripts/feature-workflow/feature-start --jcode-lead",
      "cwd": "project",
      "timeout_secs": 300,
      "confirm": false
    }
  ]
}
```

| Field | Required | Meaning |
| --- | --- | --- |
| `name` | yes | Command name after the slash. `[A-Za-z0-9_-]`, max 64. |
| `description` | yes | One line shown in `/help`; completion labels the command generically. Control characters are rejected, so a repo cannot inject terminal escapes. |
| `command` | yes | Shell prefix run with `bash -c`. May itself be a shell line. |
| `cwd` | no | `project`: the root that owns the config file (its real path, so a symlinked `.jcode/commands.json` runs the owner's scripts). `session`: the session working dir. Default: `session` for the global file, `project` for the repo file. |
| `timeout_secs` | no | Kill the command after this many seconds. Default 120, clamped to 1..3600. |
| `confirm` | no | Require `y` before running. Default false. |

Unknown fields are rejected at both the file and command level. A typo such as
`timeout_second` is an invalid entry, not a silent fallback.

## Arguments

The text typed after the command name is appended to `command` verbatim:

```
/feature-start demo-x back-end/state-machine
```

runs

```sh
bash -c 'scripts/feature-workflow/feature-start --jcode-lead demo-x back-end/state-machine'
```

Quoting, globs, `$VAR`, and paths with slashes therefore behave exactly as they
would in a terminal. Do not write `"$@"` in `command`; jcode already appends the
typed arguments.

## Dispatch and rendering

The client resolves the command, then sends the existing `input_shell` request
to the daemon with an optional `cwd` and `timeout_secs`. The daemon runs it and
streams the usual shell result card: stdout, stderr under `[stderr]`, exit code,
duration, truncation, and `Command timed out after Ns` or `Command cancelled`.

- No provider request is made for the invoking session.
- Ctrl-C cancels a running command and kills the child. This also applies to
  plain `!cmd`.
- A timeout kills the child.
- `confirm: true` prints the resolved command and working directory and waits
  for `y` or `yes`. Any other input cancels. The confirmation never runs on a
  bare Enter.
- Commands are listed in the `/help` overlay under "Project commands" and in
  composer completion. A path such as `/tmp/x` is never treated as a command.

## SSH

An SSH client runs the UI on the laptop and the daemon on the remote host. The
laptop's project files are not the remote project's files, so project commands
are refused in SSH mode with the standard "unavailable in SSH mode" message
instead of running the laptop's copy. Run jcode on the remote host to use that
project's commands.

## Security

A commands file is executable content, exactly like `AGENTS.md` plus `bash`:
only open a repo whose `.jcode/` you trust. Invoking a command is equivalent to
typing its line after `!`, so the `pre_tool` hook gate does not apply. Nothing
executes at load. Use `confirm: true` for teardown and other destructive
entries.
