# Prompt templates (`.md` files as slash commands)

A markdown file becomes a reusable `/name` command. Invoking it expands the body
into an ordinary user message, so it behaves exactly like typing that text: one
normal turn, no tool call, no protocol request. Arguments typed after the name
are substituted into the body.

This mirrors [pi prompt templates](https://github.com/earendil-works/pi) so a
template written for pi loads here unchanged.

## Templates, skills, and project commands

| | Prompt template | Skill | Project command (`.jcode/commands.json`) |
| --- | --- | --- | --- |
| What runs | A user message | A standing system reminder | A shell line |
| Model turn | Yes | Yes | No |
| Arguments | Substituted into the body | Trailing prompt | Appended verbatim |
| Good for | Reusable instructions (`/review`, `/rebase`) | Standing rules and tool knowledge | Deterministic scripts |

## Where templates load from

Per session working directory, later sources win on a name clash:

1. `$JCODE_HOME/prompts/*.md` (normally `~/.jcode/prompts/`)
2. `~/.agents/prompts/*.md` (the cross-tool `.agents` convention)
3. `<session working dir>/.jcode/prompts/*.md`
4. `<session working dir>/.agents/prompts/*.md`

Direct `.md` children only. `.agents/prompts` is a jcode addition for
cross-tool portability; pi itself reads `~/.pi/agent/prompts/` and
`.pi/prompts/`.

Nothing executes at load. A malformed, oversized, or badly named file is skipped
with a warning and never blocks session start. Files are re-read when the
working directory changes, each time a template is invoked, and whenever the
source directories' fingerprint changes, so a new or edited file also appears in
completion and `/help` without restarting the daemon (pi asks for `/reload`;
jcode does not).

## Format

```markdown
---
description: Review staged git changes
argument-hint: "[focus]"
---
Review the staged changes. Focus on ${1:-correctness, security, and error handling}.
```

- The file name (without `.md`) is the command name: `review.md` is `/review`.
- `description` is shown in `/help` and in the ACP command list. When it is
  omitted, the first non-empty body line is used, truncated to 60 characters.
- `argument-hint` is optional and only used by the ACP advertisement.
- Frontmatter is stripped before the body is sent. Unknown frontmatter keys are
  ignored, so a file written for a newer pi still loads.

## Arguments

The text after the command name is split the way pi splits it: single or double
quotes group one argument, quotes are removed, and backslash is an ordinary
character.

```
/review "API compatibility" auth
```

| Syntax | Result |
| --- | --- |
| `$1`, `$2`, … | Positional argument. A missing position becomes the empty string. |
| `$@` or `$ARGUMENTS` | All arguments joined with spaces. |
| `${N:-default}` | Argument N, or the default when it is missing or empty. |
| `${@:-default}` | All arguments, or the default when there are none. |
| `${ARGUMENTS:-default}` | Same as above. |
| `${@:N}` | Arguments from position N (1-based). |
| `${@:N:L}` | L arguments starting at position N. |

Substitution is one pass: an argument or default containing `$1` is not
expanded again. A `$` that does not form one of the forms above (for example
`$foo` or `${bar}`) stays literal. There is no `\$` escape, matching pi.

## Dispatch and precedence

1. Built-in slash commands.
2. Project commands (`.jcode/commands.json`).
3. Prompt templates.
4. Skills.

A template with a built-in or project-command name is dropped with a warning.
A template that shadows a skill wins, because the user named a command; the
skill stays available through `/skills`.

Expansion happens on the client, before the message is sent, so templates work
in local, remote and SSH sessions and in ACP hosts. The transcript shows the
expanded body, not the typed `/name`. A body that starts with `!` is sent as
prompt text; it is never re-dispatched as an input-line shell command.

## Limits

- 128 templates per session.
- 64 KiB body, 4 KiB description.
- Names are letters, digits, `_` and `-`, at most 64 characters. This is
  stricter than pi, which accepts any file name; a stem like `my.prompt` is
  skipped here with a warning.
- Only regular files are read. Symlinked files and symlinked directories are
  followed, so a dotfiles-managed template works.

## Security

A project `.jcode/prompts/*.md` is repository-controlled prompt text, like
`AGENTS.md`: only open projects you trust. It is weaker than
`.jcode/commands.json`, which executes a shell line, and weaker than a skill,
which is injected as a system reminder: a template is a user message, so it
cannot grant itself tools, prompt layers, or permissions. Descriptions are
stripped of control characters before they are drawn into `/help`.

## Relation to the other prompt layers

`prompt-overlay.md` and `system-prompt.md` are session-wide system text. A
template is per-invocation user text. Use a skill when the instructions should
stay active for the rest of the session, and a template when they should apply
to one turn.
