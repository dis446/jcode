# Feature request: markdown prompt templates as slash commands

Status: implementing (fork C9)
Owner: jcode fork
Related: `plans/native-project-slash-commands.md` (C8, shell commands),
`docs/SYSTEM_PROMPT_CONFIG.md` (prompt layers), skills (`SKILL.md`)

## Problem

jcode has three ways to reuse written text, and none of them is a prompt template:

1. **Skills** (`~/.jcode/skills/<name>/SKILL.md`, `.jcode/skills/`,
   `.agents/skills/`) are markdown and invocable as `/name`, but the body is
   injected as an `# Active Skill` system reminder, not as the user message. The
   typed tail is a separate prompt. There is no argument substitution.
2. **Project commands** (`.jcode/commands.json`) run one shell line and start no
   model turn at all.
3. **Prompt files** (`prompt-overlay.md`, `system-prompt.md`) are static and
   apply to the whole session, not per invocation.

A user coming from pi expects to drop `review.md` into a prompts directory and
get `/review` in the composer, with the file body becoming the prompt and `$1` /
`$ARGUMENTS` filled from what they typed. Today the closest workaround is a
skill, which changes the shape of the request: the template text lands in the
system prompt instead of the conversation, and arguments cannot be spliced into
it.

## Desired behavior

- A markdown file becomes a slash command named after the file.
- Invoking it expands the body into an ordinary user message and starts a normal
  turn. No new tool, no new protocol request.
- Arguments typed after the name are substituted into the body.
- The command appears in `/help` and in composer completion with its
  description.
- The expansion is plain text, so it behaves identically in local, remote and
  ACP sessions.

Example — `~/.agents/prompts/review.md`:

```markdown
---
description: Review staged git changes
argument-hint: "[focus]"
---

Review the staged changes. Focus on ${1:-correctness, security, and error handling}.
```

`/review concurrency` sends:

```text
Review the staged changes. Focus on concurrency.
```

## Why not one of the existing layers

Checked in the lowest-first order required by `AGENTS.md`:

1. **Existing upstream behaviour.** There is no prompt-template concept.
   `docs/SYSTEM_PROMPT_CONFIG.md` covers session-wide prompt layers only. jcode
   itself is the fork here, so upstream has nothing to inherit.
2. **Existing primitive.** The skill registry is the right pattern to copy — a
   directory of markdown, filename-to-command, project overlay, refresh-on-miss
   — but not the right semantics to reuse. A skill sets `active_skill` and
   injects a system reminder; a template replaces the user message and must not
   become a sticky skill. Project commands are the wrong shape entirely (shell,
   no turn).
3. **Operator configuration.** Not satisfiable by config: skills cannot
   substitute arguments into a user message, and `prompt-overlay.md` is not
   per-invocation.
4. **Minimal additive code.** One small module plus thin hooks into the existing
   slash dispatch, completion and ACP advertisement paths.

So the layer is 4, but the surface is deliberately narrow: a loader, a
substituter, and dispatch that produces ordinary input.

## Proposed design

### 1. Files and format

Loaded per session working directory, later wins:

1. `$JCODE_HOME/prompts/*.md` (normally `~/.jcode/prompts/`)
2. `~/.agents/prompts/*.md` (the cross-tool `.agents` convention, resolved with
   `storage::user_home_path` so sandboxed runs stay under `$JCODE_HOME/external`)
3. `<session working dir>/.jcode/prompts/*.md`
4. `<session working dir>/.agents/prompts/*.md`

`.agents/prompts` is a jcode addition, not a pi path, but it is where the pi
templates already installed on this machine live and it matches how skills
already honor `.agents/skills`.

Direct `.md` children only, read with a metadata check and a hard size ceiling
(regular files only; symlinked files and symlinked directories are followed so a
dotfiles-managed template loads). Missing or malformed files are skipped with a
warning; they never block session start.

- Command name = file stem, validated `[A-Za-z0-9_-]`, max 64. Other stems are
  skipped with a warning. This is a deliberate divergence from pi, which accepts
  any basename (so `my.prompt.md` is `/my.prompt` there and skipped here).
- Frontmatter: `description` (optional; falls back to the first non-empty body
  line truncated to 60 characters plus `...`, exactly like pi; control
  characters are stripped because it is drawn into `/help`), `argument-hint`
  (optional, stored for completion hints).
- Frontmatter is stripped from the sent text. Unknown frontmatter keys are
  ignored, not rejected: this is prose, not executable config.
- Body size cap 64 KiB, to keep an accidental blob out of the transcript.

### 2. Substitution

Exactly pi's table, checked against `dist/core/prompt-templates.js` in the
installed pi package, applied to the body in one non-recursive pass:

| Syntax                         | Result                                          |
| ------------------------------ | ----------------------------------------------- |
| `$1`, `$2`, …                  | Positional argument; missing becomes `""`       |
| `$@` / `$ARGUMENTS`            | All arguments joined with spaces                |
| `${N:-default}`                | Argument N, or the default when missing/empty   |
| `${@:-default}`                | All arguments, or the default when empty        |
| `${ARGUMENTS:-default}`        | Same as above                                   |
| `${@:N}` / `${@:N:L}`          | Arguments from N / L of them (1-based)          |

`$0` and out-of-range positions become the empty string, not literal text.
Values and defaults are never re-scanned, so an argument containing `$1` stays
literal. There is **no** `\$` escape in pi; only a `$` that does not form one of
the forms above (for example `$foo` or `${bar}`) stays literal.

Arguments are split the way pi's `parseCommandArgs` does: single or double
quotes group one argument, quotes are removed, adjacent quoted runs concatenate,
empty quoted arguments survive, and backslash is an ordinary character. jcode
has no existing argv splitter to reuse (`jcode-command-risk::tokenize` is a
shell-risk tokenizer that emits operators), so the module carries the ~30-line
port plus tests.

### 3. Dispatch and precedence

- Built-in slash commands always win.
- Then project commands (`.jcode/commands.json`, C8).
- Then prompt templates.
- Then skills.

A template that shadows an installed skill wins, because the user wrote a
command with that name; the skill remains reachable through `/skills`. A refused
collision logs one warning.

Insertion points, mirroring skill dispatch:

The normal TUI is a remote client, so the local hook alone is not enough.

- `crates/jcode-tui/src/tui/app/input.rs::submit_input` — after the
  shell/command handling and before the `resolve_invocation` skill branch.
- `crates/jcode-tui/src/tui/app/remote/input_dispatch.rs::submit_remote_slash_input`
  — after project commands and before the SSH gate and skill resolution. Every
  non-built-in slash input reaches this function from
  `submit_remote_enter_input`, so one hook covers both remote entry points.
- `crates/jcode-tui/src/tui/app/remote/input_dispatch.rs::submit_prepared_remote_input`
  — split its gates/shell-check/tail so a template can deliver an already
  expanded prompt without re-running the `!cmd` shell check on body text. A body
  that begins with `!` must not execute a shell command.
- `src/cli/acp.rs` — template resolution before skill resolution (an explicit
  client `skill` metadata selection still wins), plus entries in
  `availableCommands`. Built-ins and skills with the same name are filtered.
- Completion and `/help` read from the same resolved set. Templates are re-read
  each time one is invoked, so a project switch, an edit and a new file all take
  effect without a daemon restart. Completion labels templates
  `"Prompt template"`, matching how skills show `"Activate skill"`; the
  description is shown in `/help`. This avoids widening the
  `(String, &'static str)` suggestion type in this change.

### 4. Module layout

- `crates/jcode-base/src/prompt_template.rs`: registry, loader, argv splitter,
  substitution, tests, exposed as `crate::prompt_template` through the existing
  `pub use jcode_base::*` chain.
- Hooks: the four paths above, plus the help-section renderer used by project
  commands.

## Acceptance criteria

1. `/review` with no arguments sends the body verbatim, in one turn.
2. `/review auth` substitutes `$1` / `$ARGUMENTS`; quoted arguments stay one
   argument; a missing position becomes the empty string and an unknown `$` form
   stays literal (pi's exact behavior).
3. The project file overrides the global file of the same name.
4. A template appears in completion and under a "Prompt templates" section in
   `/help`, with its description.
5. A malformed or oversized file is skipped with a warning and the session still
   starts.
6. A template name that collides with a built-in is skipped with a warning.
7. A template that shadows a skill wins, and the skill is still listable.
8. `/review` works in a remote/SSH session (expansion is client-side text).
9. Editing or adding a file takes effect without restarting the daemon.
10. Frontmatter never reaches the model.
11. Templates also load from `~/.agents/prompts/` for cross-tool portability
    (checked against the four pi templates already installed there).
12. A body that begins with `!` or `/` is sent as prompt text, never re-dispatched
    as a shell or slash command.
13. ACP `availableCommands` advertises templates and a prompt naming one is
    expanded before skill resolution.

## Non-goals

- Template-defined tools, shell hooks, or model/effort overrides.
- Nested directories, globs, or package manifests selecting templates.
- Argument completion or per-template slash subcommands.
- `${@:N}` / `${@:N:L}` slices if the v1 substitution stays small.
- Replacing skills. Templates send a message; skills carry standing instructions.

## Risks and edge cases

- **Untrusted project text.** A repo `.jcode/prompts/*.md` is prompt content
  like `AGENTS.md`: only open projects you trust. Expansion is a user message,
  not a system message, so a template cannot grant itself tool or prompt
  privileges, but it can still ask the model to do anything the user could type.
- **Silent drift from pi.** Partial substitution support is worse than a
  documented subset; the doc must state exactly what is and is not expanded.
- **Name collisions.** Two silent wins (built-in, skill) could confuse; both log
  a warning, and `/help` shows the owner of the name.
- **Very large bodies** inflate transcript and cost; the size cap is the guard.
- **Skills reload path.** The existing refresh-on-slash-miss calls the skill
  registry; the template registry needs its own refresh or a shared reload, or a
  new file will look "unknown" until restart.

## Resolved questions

1. Load `~/.agents/prompts/` as well: that is where this machine's pi templates
   live (`~/.pi/agent/prompts` is symlinked into the dotfiles copy).
2. Full pi substitution table on day one; the deferred slice forms are cheap and
   a partial table would silently diverge.
3. Name validation stays stricter than pi (deliberate), completion labels by
   type, and no `\$` escape because pi has none.
