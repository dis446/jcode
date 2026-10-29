<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/jcode-mark-inverse.svg">
  <img src="assets/brand/jcode-mark.svg" alt="jcode halftone donut logo" width="140">
</picture>

# jcode (personal fork)

My personal fork of [jcode](https://github.com/1jehuang/jcode)
([@dis446](https://github.com/dis446/jcode)), tracking upstream
`master` with a small set of local changes.

</div>

> This is **not** the official project. For installation, docs, releases,
> benchmarks, and everything else, use
> [github.com/1jehuang/jcode](https://github.com/1jehuang/jcode) and
> [jcode.sh](https://jcode.sh). This README only covers how the fork differs.

## Differences from upstream

### `@file` completion in the composer

Typing `@` in the TUI input opens an inline fuzzy file
picker, matching pi's behavior:

- Type `@` followed by a partial name/path, e.g. `@src/ma` or `@README`.
- Candidates come from a gitignore-aware walk of the working directory (`.git`
  itself is skipped; hidden files are included).
- Ranking: exact name > name prefix > name substring > path substring, with
  directories boosted, shallower paths first, 20 results max.
- `Up`/`Down` move the selection, `Enter` accepts, `Esc` dismisses. Accepting a
  directory keeps it open for drilling down.
- `@"..."` quoting is supported for paths with spaces, and `Tab` can force
  completion of a plain (non-`@`) path token.
- Implemented in `crates/jcode-at-file` (pure completion logic) plus the TUI
  overlay in `crates/jcode-tui/src/tui/ui_input/at_file.rs`.

### Project-defined tools (`.jcode/tools.json`)

A repository can declare local commands as native tools with their own names,
without an MCP server. Loaded from `$JCODE_HOME/tools.json` (normally
`~/.jcode/tools.json`) and `<working dir>/.jcode/tools.json`, and replaced on
every session attach; the tool input is passed as JSON on stdin and stdout
becomes the result. See [docs/PROJECT_TOOLS.md](docs/PROJECT_TOOLS.md).

### `/new [name]`

Starts a fresh session in place, like pi's `/new`, and optionally names it:

- `/new` replaces the current conversation with a new empty session in the same
  working directory.
- `/new refactor auth` starts the session with the display name
  `refactor auth`, so it is easy to find in `/resume`.
- The previous session is closed and saved as usual.
- `/new` is refused while a turn is running. Press Esc or run `/cancel` first.

### Live provider credit and quota chip

The status line's right edge shows the remaining credit or plan usage for the
provider in use, when that provider exposes a metering endpoint:

- DeepSeek and Moonshot AI show remaining credit, such as `DeepSeek $28.11`.
  The chip is amber below 5 and red at zero.
- OpenCode Go shows its rolling, weekly, and monthly windows with reset
  countdowns, such as
  `OpenCode Go 5h 2% (58m) · 1w 20% (3d 6h) · 1mo 31% (15d)`. The values are
  percent used; the chip is amber at 70% and red at 100%. Data comes from
  `https://opencode.ai/zen/go/v1/usage`, an unofficial endpoint with no
  documented public API.
- `/usage` shows the same quota windows as bars with reset times.
- Polls once a minute with a 10 second deadline. A failed query retries after
  two minutes and never blocks a turn or shows a fabricated number.
- Switching providers clears the old chip immediately.
- On by default. Disable with `[display] provider_balance = false`.

### `ask_user_question`

A tool that lets the model ask a branching questionnaire instead of guessing.
The TUI renders a full-frame dialog with authored options, an optional typed
answer on each question, and a review screen:

- Questions form a tree. Each option can name its own `next`, and a node-level
  `next` covers custom answers and options without a branch. Different answers
  may converge on the same follow-up question.
- No cap on question or option count. Validation rejects unknown targets,
  unreachable nodes, and cycles (reported with their path). The dialog scrolls
  instead of truncating.
- Multi-select questions, optional descriptions, back navigation that restores
  prior selections, and Shift+Enter for newlines in a typed answer.
- Answers return to the model as structured JSON with the path taken;
  cancellation is a normal result.
- Available only in the TUI: headless runs, the SDK, the harness bridge, and
  Desktop do not register it. See
  [docs/ASK_USER_QUESTION.md](docs/ASK_USER_QUESTION.md).

### Project-defined slash commands (`.jcode/commands.json`)

A repository can also declare deterministic slash commands that run a shell
line with no model turn, no provider tokens, and no dependence on the tool
list. Loaded from `$JCODE_HOME/commands.json` and
`<working dir>/.jcode/commands.json`; arguments are appended verbatim, output
renders as the usual shell card, `confirm: true` gates destructive entries, and
Ctrl-C or the configured timeout kills the child. See
[docs/PROJECT_COMMANDS.md](docs/PROJECT_COMMANDS.md).

Everything else in this repository is upstream code.
