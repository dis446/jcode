# Feature request: native project slash commands (shell, no model turn)

Status: merged in fork `master`, installed as 0.99.407-dev
Owner: jcode fork (C8)
Related: C2 project-defined tools (`docs/PROJECT_TOOLS.md`), qLab `/feature-*` skills

> As built: the command runs on the daemon through the existing
> `Request::InputShell` path (not client-side, not a new request), and the
> startup message is an optional composer draft (`--initial-message`) instead of
> a queued user message. See "As built" below for the deviations and the
> verification evidence.

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
  the nvim/terminal tabs, spawns the jcode pane, and that pane may be prefilled
  with one draft message only:

  ```
  Do not call any tools. Wait for the user's instructions.
  ```

  `jcode --initial-message` fills the composer without submitting it. The new
  session makes zero model calls and sits idle until the user types. A fresh
  jcode pane is already idle, so the flag is optional context, not a required
  part of the workflow.
- `/feature-*` keeps working when the invoking session's tool list is frozen or
  the project tools are missing: the command path is independent of both.

## Proposed design

> Superseded in part by "As built": execution is daemon-side (section 2), not
> client-side, and the startup message is a draft prefill (section 3), not a
> queued message.

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

Acceptance result: 1, 3, 4, 5, 6, 7, 9 and 10 are covered by unit or e2e
tests. 2 changed shape: the pane is prefilled with an optional draft and makes
zero calls; the herdr workspace itself is qLab-side and unchanged here. 8
(SSH refusal) is implemented and covered by inspection, not by an automated
test, because the refusal needs a live `RemoteConnection`.

## Implementation map (as built)

- `crates/jcode-app-core/src/project_config.rs`: shared bounded, symlink-aware
  config reader. `tool/project.rs` uses it too.
- `crates/jcode-app-core/src/command/project.rs`: `.jcode/commands.json`
  loader, validation, merge, `confirm`, timeout clamping, shell-line builder.
- `crates/jcode-app-core/src/input_shell_registry.rs` and
  `server/client_actions.rs::run_input_shell`: timeout and Ctrl-C kill the
  child. `server/client_lifecycle.rs` wires `Cancel` to the registry.
- `crates/jcode-protocol/src/wire.rs`: `InputShell` gains additive
  `cwd`/`timeout_secs`, so the daemon runs the project's script in place.
- `crates/jcode-tui/src/tui/app/commands_project.rs`: resolve, confirm, local
  execution, and remote dispatch.
- `app/state_ui_runtime.rs` (`project_commands`, `known_command_names`),
  `app/state_ui_input_helpers.rs` (completion), `app/commands_dispatch.rs` and
  `app/remote/{key_handling,input_dispatch}.rs` (hooks),
  `tui/ui_overlays.rs` + `tui/mod.rs`/`app/tui_state.rs` (help section).
- `src/cli/args.rs` + `tui_launch.rs` + `app/tui_lifecycle.rs`:
  `--initial-message` prefills the composer.
- New tests: `app/tests/project_commands.rs` and `e2e/suite_project_commands.py`.
- qLab itself (`.jcode/commands.json`, `feature-start`/`feature-respawn` spawn
  line, deleting the six skills) lives outside this repository and is not part
  of this change.

## Risks and edge cases

- **Security.** A project command executes arbitrary shell on invocation, with
  no tool call and therefore no `pre_tool` gate. It is equivalent to typing a
  `!command` in a repo you already trust. Nothing executes at load. `confirm`
  exists for destructive entries.
- **Argument quoting.** The typed tail is appended verbatim, so the shell sees
  the same words, quotes, globs, and paths. `command` is a prefix, not a full
  line with `"$@"`.
- **Working directory.** Project commands run at the config root, which is the
  qLab repo even when the config is symlinked into a feature root. That is what
  the feature scripts expect.
- **Shadowing.** Built-ins win. Skills and tools with the same name remain, but
  the command path is the one the user typed.
- **Remote sessions.** Execution is daemon-side, so a normal local daemon runs
  the project's own script. An `/ssh` client refuses: the laptop's project tree
  is not the remote one.
- **Initial message.** There is no "persisted but not dispatched" user message
  in the protocol; `queued_messages` auto-sends. The flag therefore fills the
  composer, which guarantees zero calls.

## Non-goals

- Replacing project tools. Tools remain the model-callable surface; commands are
  the deterministic user-invoked surface.
- A general plugin system for slash commands with subcommands or completion
  beyond names and descriptions.
- Running project commands over SSH in v1.

## Resolved decisions

1. The initial message is an optional composer draft (`--initial-message`), not
   a queued message and not a tools-disabled turn.
2. `confirm` is per-entry only. No name-pattern defaults; the script's own
   refusals still apply.
3. One `commands.json`, symmetric with `tools.json`.
4. Visible in both `/help` and completion.
5. Execution is daemon-side through `Request::InputShell`; there is no separate
   protocol request.
6. The ledger entry is C8: the fork master this branch rebased onto already used
   C5 (`ask_user_question`), C6 (Herdr graphics) and C7 (todo doc).

The review after that rebase added three hardening changes: the daemon clamps a
client-supplied deadline (an unchecked `u64::MAX` overflows `Instant + Duration`),
descriptions reject control characters because a repo file is drawn straight into
`/help`, and the completion candidate cache is keyed on the working dir so a
project switch cannot keep offering the previous project's names. A protocol test
pins that an older peer's `input_shell` payload still decodes.
