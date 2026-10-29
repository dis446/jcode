# Feature request: native project slash commands (shell, no model turn)

Status: proposed
Owner: jcode fork (C3)
Related: C2 project-defined tools (`docs/PROJECT_TOOLS.md`), qLab `/feature-*` skills

## Problem

qLab's alpha feature workflow is reachable in jcode through two layers that both
depend on the model:

1. `feature_*` project tools. The model must decide to call them, and a
   long-running session that froze its tool list before the tools were
   registered never sees them at all (`locked_tools`).
2. `/feature-*` skills. A skill invocation starts a model turn, so the user pays
   tokens and latency before anything runs, and the result still depends on the
   model choosing the right tool with correctly parsed arguments.

The workflow is deterministic: parse arguments, run one script, print its
output. It should behave like pi's `/feature-*` commands: the command itself runs
the shell, opens the herdr workspace, spawns the jcode pane, and seeds that pane
with a minimal "wait for instructions" message. No tools, no turn, no tokens in
the invoking session, and no dependence on a live tool list.

## Desired behavior

Typing a project command in the jcode composer executes it directly.

```
/feature-start demo-x back-end/state-machine
/feature-continue demo-x
/feature-mr demo-x --dry-run
/feature-stop demo-x
/feature-list
/feature-link AGENTS.md
```

- No model call, no tool call, no provider tokens. Verified by the absence of an
  `AGENT_PROVIDER_STREAM_LIFECYCLE` event for the invoking session.
- Arguments are passed verbatim as shell positional parameters. No parsing or
  reformatting by the model.
- The command runs in the project root by default, with the session working
  directory available as an option.
- stdout and stderr appear in the transcript as a shell result, including exit
  code and duration, reusing the existing `!command` presentation.
- Ctrl-C cancels the running command; a timeout kills it.
- `feature-start` (via the existing script) creates the herdr workspace, opens
  the nvim/terminal tabs, spawns the jcode pane, and that pane receives one
  queued user message only:

  ```
  Do not call any tools. Wait for the user's instructions.
  ```

  The message is queued, not sent: the new session makes zero model calls and
  sits idle until the user types. If queuing is not possible from the CLI, the
  fallback is one turn with all tools disabled for that turn and the same text.
- `/feature-*` keeps working when the invoking session's tool list is frozen or
  the project tools are missing: the command path is independent of both.

## Proposed design

### 1. Project command definitions: `.jcode/commands.json`

Symmetric with `.jcode/tools.json`, loaded from `$JCODE_HOME/commands.json` and
`<working dir>/.jcode/commands.json`, project entries winning on a name clash.

```json
{
  "commands": [
    {
      "name": "feature-start",
      "description": "Start an alpha feature: /feature-start <name> [repo ...]",
      "command": "scripts/feature-workflow/feature-start --jcode-lead \"$@\"",
      "cwd": "project",
      "timeout_secs": 300
    }
  ]
}
```

- `command` is a shell line. Dispatch runs `bash -c <command> jcode-command <user args...>`
  so `$@` is exactly the text after the command, one token per shell word.
- `cwd`: `project` (default, the root that owns the config file) or `session`.
- `timeout_secs`: default 120, clamped 1..3600.
- Optional `confirm: true`: print the resolved command and require `y` before
  execution. Use it for teardown.
- Names are validated like tool names (`[A-Za-z0-9_-]`, max 64). A name that
  collides with a built-in slash command is skipped with a warning; built-ins win.

The file is read on session attach only. Nothing is executed at load time.

### 2. Dispatch

- Register project command names in `registered_command_names()` so the slash
  parser, completion and command sequences recognise them.
- In `dispatch_single_local_command`, try project commands after the built-in
  handlers and before falling through to model input. Return `true` when claimed.
- Execute through the existing shell machinery in
  `crates/jcode-tui/src/tui/app/input/shell_commands.rs` (`bash -c`, stdin null,
  captured output, `InputShellCompleted`), so output rendering, truncation and
  the result card already exist.
- Run client-side, like `!commands`. Under `/ssh` remote sessions, refuse with
  the existing "local action unavailable" message, because the project tree on
  the laptop is not the remote one.
- Commands are client-side only for v1. A daemon-side variant is a follow-up if
  a remote project needs its own commands.

### 3. `jcode --initial-message` (and optional `--first-turn-tools none`)

The generated pane needs a message without a model turn.

- Add `--initial-message <TEXT>` to `Args` (`src/cli/args.rs`), global.
- After the session attaches, queue the text as a pending user message in the
  daemon's input queue. The message is persisted and visible, but no turn starts
  until the user sends something. This guarantees zero tool calls.
- If a queue injection point is not reachable in that code path, the fallback is
  to send the message as the first turn with a session tool overlay
  (`SessionToolConfig { enabled: Some(vec![]), .. }`) applied for that turn only
  and cleared before the next user message.
- The flag is also useful outside features: seed a spawned worker with one line.

### 4. Herdr and script changes (qLab side)

`scripts/feature-workflow/feature-start` already creates the workspace, tabs and
the jcode pane (`json pane run "$rp" "cd \"$ROOT\" && jcode"`). Change only the
spawn line for jcode leads:

```bash
json pane run "$rp" "cd \"$ROOT\" && jcode --initial-message \"$INITIAL_MSG\""
```

with

```bash
INITIAL_MSG='Do not call any tools. Wait for the user'"'"'s instructions.'
```

Apply the same to `feature-respawn` (used by `feature-continue`) so a resumed
lead also starts quiet. Keep the `.jcode/prompt-overlay.md` context: it is a
prompt layer, not a turn.

### 5. qLab command definitions

`.jcode/commands.json` declares:

| Command | Shell |
| --- | --- |
| `feature-start` | `scripts/feature-workflow/feature-start --jcode-lead "$@"` |
| `feature-continue` | `scripts/feature-workflow/feature-continue --jcode-lead "$@"` |
| `feature-mr` | `scripts/feature-workflow/feature-mr "$@"` |
| `feature-stop` | `scripts/feature-workflow/feature-stop "$@"` (confirm: true) |
| `feature-list` | `scripts/feature-workflow/feature-list` |
| `feature-link` | `scripts/feature-workflow/feature-link "$@"` |

`feature-stop` runs from the project root today, where the script refuses unless
cwd is inside the workspace. Either keep that refusal (the command name alone is
not consent) or add `cwd: "session"` on a variant that is only useful inside a
feature workspace. Decide in implementation, document the choice.

The six `/feature-*` skills can then be deleted. Keep the `feature-lifecycle`
skill as documentation. Built-in commands should shadow a same-named skill.

## Acceptance criteria

1. `/feature-list` in qLab prints the list in the transcript with exit code and
   duration, and the invoking session logs no provider stream event.
2. `/feature-start demo-x` creates `features/demo-x`, opens the herdr workspace,
   spawns the jcode pane, and the pane's session contains the queued initial
   message with zero provider calls and zero tool calls.
3. Arguments round-trip: `/feature-link AGENTS.md`, quoted paths, and a repo path
   with slashes all reach the script unchanged.
4. A failing command shows stderr and a non-zero exit; a timeout kills the child
   and reports the timeout.
5. Ctrl-C stops a long-running command.
6. Name collision with a built-in logs a warning and the built-in still wins.
7. `/feature-start` works in a session whose tool list was frozen before project
   tools existed.
8. `/ssh` remote sessions refuse project commands with an actionable message.
9. `confirm: true` prompts before execution and does nothing on `n`.
10. A malformed `commands.json` never prevents session startup; invalid entries
    are skipped with a warning.

## Implementation map (fork)

- New: `crates/jcode-app-core/src/command/project.rs` (or a `commands` module
  beside `tool`) with the loader, validation, merge and tests.
- `crates/jcode-tui/src/tui/app/commands_dispatch.rs`: project command lookup in
  `dispatch_single_local_command`, names in `registered_command_names`, remote
  guard.
- `crates/jcode-tui/src/tui/app/input/shell_commands.rs`: reuse
  `build_input_shell_command` / `spawn_input_shell_command` with argv support
  (today it takes one shell string; add a positional-args variant).
- `src/cli/args.rs`: `--initial-message`.
- Client startup path: queue the initial message after attach.
- `crates/jcode-tui/src/tui/app/input.rs` or the command completion source:
  expose project command names to the composer.
- qLab: `.jcode/commands.json`, `scripts/feature-workflow/feature-start` and
  `feature-respawn` spawn line.

## Risks and edge cases

- **Security.** A project command executes arbitrary shell on invocation, with
  no tool call and therefore no `pre_tool` gate. It is equivalent to typing a
  `!command` in a repo you already trust. Nothing executes at load. `confirm`
  exists for destructive entries.
- **Argument quoting.** Use positional parameters, never string interpolation
  into the shell line, so spaces and globs survive.
- **Working directory.** Project commands run at the config root, which is the
  qLab repo even when the config is symlinked into a feature root. That is what
  the feature scripts expect.
- **Shadowing.** Built-ins win. Skills and tools with the same name remain, but
  the command path is the one the user typed.
- **Remote sessions.** v1 is client-local. A user in `/ssh` mode gets a clear
  refusal instead of running the laptop's qLab against a remote project.
- **Initial message.** It must not start a turn. If a queue injection is not
  available on every transport, the fallback must still guarantee no tool call.

## Non-goals

- Replacing project tools. Tools remain the model-callable surface; commands are
  the deterministic user-invoked surface.
- A general plugin system for slash commands with subcommands or completion
  beyond names and descriptions.
- Running project commands on the daemon or over SSH in v1.

## Open questions

1. Queue the initial message with zero turns (preferred) or send one turn with
   tools disabled (fallback)? The plan assumes queue-first.
2. Should `confirm: true` default on for names matching a denied pattern
   (`stop`, `rm`, `delete`), or stay per-entry only?
3. One `commands.json` like `tools.json`, or one file per command for easier
   diffs?
4. Should project commands be visible in `/help` and command completion, or only
   in completion?
