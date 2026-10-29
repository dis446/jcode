# jcode fork governance

This is the team-of-one fork of [`1jehuang/jcode`](https://github.com/1jehuang/jcode)
behind `github.com/dis446/jcode`. We run a **pinned upstream plus our own custom
work**; upgrading upstream is a deliberate, planned act. Nothing here updates
jcode automatically.

Custom work is deliberate and minimal. The default answer to a feature request
is an existing upstream feature, an existing codebase primitive, or operator
configuration, not new Rust. Adding Rust to the fork is a high bar.

Everything about the fork lives **in this repo**: the ledger, the plans and the
TUI regression suite. The dotfiles repo only holds the pin, the config and the
install entry point.

## Rules

1. **Prudence before code.** Start every change with "what is the best way to
   deliver the operator's request?", not "what code do I add?". Work through
   upstream behaviour/config, then an existing codebase primitive, then
   operator config, skills, hooks, or dotfiles, and only then fork Rust. A code
   change must extend the existing pattern and record in the ledger why the
   earlier layers do not work. See "Feature-request prudence" in `AGENTS.md`.
2. **This repo is the source of truth.** `~/dotfiles/scripts/jcode-install.sh`
   clones/updates this repo at its default branch (`master`) and builds it, so
   pushing here is what makes a version installable. `~/dotfiles/jcode/UPSTREAM_VERSION`
   records the upstream base release (no `v`) that `master` sits on, for planning
   the next upgrade.
3. **`jcode update` is banned.** Upgrades follow the runbook below. In-app update
   checks are off (`~/dotfiles/jcode/config.toml`: `check_updates = false`), and
   `up` in the shell never touches jcode.
4. **Every custom change is tracked** in the ledger below *before* it is merged.
   Git history is the patch series; generate a diff with `git format-patch` only
   when one is needed, and do not commit generated patch files. No untracked
   local divergence.
5. **Remotes:** `origin` = this fork, `upstream` = `1jehuang/jcode`. `master`
   carries our commits on top of upstream `master`, kept as a small, rebased
   series; no long-lived feature branches.
6. **Keep patches small and isolated, and follow existing primitives.** Prefer
   new files/modules and thin hooks into upstream code over editing existing
   functions. Smaller touch surface = fewer conflicts. Extend the established
   tool, session, config, hook, skill, storage, provider, or UI pattern instead
   of adding a second one. Note the upstream files each change touches.
7. **Config/prompt/skills stay in dotfiles**, not here, wherever possible.

## Ledger of custom work

| ID | Title | Status | Base | Upstream files touched | Plan | Upstreamable |
| -- | ----- | ------ | ---- | ---------------------- | ---- | ------------ |
| C1 | Native `@file` picker (pi parity) | merged in `master`, installed | upstream `master` (b61e58b32, builds as 0.99.339-dev) | `jcode-tui`: `app.rs`, `app/input.rs`, `app/local.rs`, `app/remote.rs`, `app/remote/key_handling.rs`, `app/state_ui_input_helpers.rs` (`autocomplete`), `app/tui_lifecycle.rs`, `app/tui_state.rs`, `tui/mod.rs` (`TuiState::at_file_view`), `tui/ui.rs`, `tui/ui_input.rs`. New: `crates/jcode-at-file`, `app/at_file.rs`, `ui_input/at_file.rs`. Root `Cargo.toml` (workspace member) | [plan](plans/at-file-picker.md) | yes (additive, 1 new crate + ~14 one-line hooks) |
| C2 | Project-defined tools (`.jcode/tools.json`) | merged in `master`, installed | upstream `master` (b61e58b32, builds as 0.99.339-dev) | `jcode-tool-core`: `lib.rs` (`Tool::is_project_tool` default). `jcode-app-core`: `tool/mod.rs` (`mod project;`, `Registry::register_project_tools_for_dir` + `_with_global`), `server/client_session.rs` (subscribe + two resume paths), `server/headless.rs`, `server.rs`, `server/worker_revive.rs`. Root `src/cli/commands.rs`. New: `tool/project.rs`, `docs/PROJECT_TOOLS.md`. Project registration is independent of MCP and runs after the fixed MCP tools at each session site. | [doc](docs/PROJECT_TOOLS.md) | yes (new module + thin hooks) |
| C3 | `/new [name]` slash command (pi parity) | merged in `master`, installed | upstream `master` (b61e58b32, builds as 0.99.345-dev) | `jcode-tui`: `app.rs` (pending new-session state), `app/tui_lifecycle.rs` (2 inits), `app/commands.rs` (`handle_session_command`), `app/commands_review.rs` (`reset_current_session` keeps working dir, refreshes title/cache), `app/remote.rs` (deferred clear completion), `app/remote/key_handling.rs` (deferred `/new`, stale-state reset), `app/remote/server_events.rs` (Clear Done/Error and quiet rename), `app/state_ui_input_helpers.rs` (registry), `tui/backend.rs` (`rename_session` returns request id), `ui_overlays.rs` (help). New: `app/tests/new_session_command.rs` | n/a | yes (additive slash command) |
| C4 | Live provider credit and quota chip (DeepSeek/Moonshot balance, OpenCode Go plan usage) | merged in `master`, installed (abf49c992, 0.99.386-dev) | upstream `master` (b61e58b32, builds as 0.99.356-dev) | `jcode-base`: `usage.rs` (`mod balance;` + re-export), `usage/api_keys.rs` (`configured_key` becomes `pub(super)`; the DeepSeek/Moonshot balance fetch+parse bodies move into `usage/balance.rs`, and `/usage` calls the same functions the chip does). `jcode-config-types`: `display.rs` (`provider_balance`). `jcode-tui`: `tui/mod.rs` (module + `TuiState::provider_balance_chip`), `app.rs` (module + field), `app/tui_lifecycle.rs` (2 inits), `app/tui_state.rs`, `app/local.rs`, `app/remote.rs`, `ui_input.rs` (`draw_status` right-aligned chip). New: `jcode-base/src/usage/balance.rs`, `tui/provider_balance.rs`, `app/provider_balance.rs`. OpenCode Go expansion: `jcode-usage-types` (`UsageLimit` gains `PartialEq`); `usage/balance.rs` (`BalanceSnapshot.limits`, `LimitChip`, `ProviderChip`, `fetch_chip`, `opencode_go_snapshot` parsing rolling 5h/weekly/monthly `percent` + `resetsAt` from the unofficial `https://opencode.ai/zen/go/v1/usage`); `usage/api_keys.rs` (`/usage` windows, no local-spend row); `tui/provider_balance.rs` (per-window label with compact resets plus a no-reset fallback when the status line is narrow, amber at 70%, red at 100%). Endpoint is undocumented; a failed query clears the chip and retries after two minutes. | n/a | yes (new modules + thin hooks; additive provider branch) |
| C5 | `ask_user_question` branching TUI dialog | merged in `master`, installed | upstream `master` (ace3d721b, builds as 0.99.377-dev) | `jcode-tool-types`: `lib.rs`, `Cargo.toml` (serde, question types). `jcode-tool-core`: `lib.rs` (interactive input kind). `jcode-protocol`: `lib.rs`, `wire.rs`, `wire/events.rs`, `Cargo.toml`. `jcode-app-core`: `tool/mod.rs`, `tool/bash.rs`, `agent/turn_execution.rs`, `server/client_session.rs`, `server/client_lifecycle.rs`, `server/client_actions.rs`, protocol tests. `jcode-tui`: `tui/mod.rs`, `app.rs`, `app/tui_lifecycle.rs`, `app/tui_state.rs`, `app/input.rs`, `app/remote/key_handling.rs`, `app/remote/server_events.rs`, `app/remote.rs`, `tui/backend.rs`, `tui/ui.rs`. New: `tool/ask_user_question.rs`, `tui/question_overlay.rs`, `app/question_overlay.rs`, `docs/ASK_USER_QUESTION.md` | [plan](plans/ask-user-question.md) | yes (one new tool, one new TUI module, thin hooks) |
| C6 | Kitty graphics inside Herdr panes | merged in `master`, installed (d39541113, 0.99.382-dev) | upstream `master` (ace3d721b, builds as 0.99.377-dev) | `jcode-tui-mermaid`: `mermaid_runtime.rs` (`fast_path_assumed_protocol`, `herdr_kitty_images_enabled`, hook in `init_picker`), runtime tests. `docs/HERDR.md` compatibility bullet. Herdr rewrites `TERM=xterm-256color`, exports `TERM_PROGRAM=herdr`, and strips `KITTY_WINDOW_ID`, so the fast env path returned `None` and every image-capable surface (Mermaid, LaTeX, read images) fell back to text. Herdr 0.9.5 renders Kitty graphics for child apps (`terminal.kitty_graphics`, on by default), so the fork now assumes Kitty when the env probe misses inside Herdr, leaving env-detected protocols and probe answers untouched; `JCODE_HERDR_KITTY_IMAGES=0` restores the old fallback, and tmux/screen nested inside a pane (`TERM=tmux-*`/`screen*`) still falls back to text. | n/a | yes (small additive detection rule + env opt-out, mirrors `JCODE_ZELLIJ_KITTY_IMAGES`) |

Status: planned, in-progress, merged (in `master`), dropped, upstreamed
(delete from the series on next rebase). Add a row for every new change.

C1 verify: `e2e/run.py --binary <fork bin> --suite all` (13 `at_file` tests), plus
`cargo test -p jcode-at-file` (17) and `cargo test -p jcode-tui --lib at_file` (14).
C1 conflict hints: hooks sit beside `handle_command_suggestion_key` (two key paths: local
`input.rs` and remote `key_handling.rs`), `insert_input_text_with_undo` tail, the Backspace arms
in both paths, `autocomplete()` after `get_suggestions_for`, and the `draw_command_suggestions_overlay`
call in `ui.rs`. If upstream adds a third key path, the e2e suite fails first.

C2 best-way check: upstream behaviour/config and operator config cannot expose
repo-declared commands as native tools. Project-local MCP is the closest
primitive but forces a separate server and `mcp__<server>__` names. Chosen
layer: minimal additive fork code, one new module plus thin hooks.
C2 verify: `cargo test -p jcode-app-core project::` (27 tests: load/merge/validation,
invalid-entry isolation, unknown-field rejection, malformed `properties`/`required`,
oversized config/description/tool-count limits, non-regular config files,
global/project override, global-only when no working dir, global `session` cwd
default, repo `project` cwd default, real-path symlink resolution, global
config-root command resolution, `JCODE_HOME` path resolution, reload/removal,
existing-tool collision, stdin JSON, intent stripping and opt-in, non-zero exit,
stderr-first failure output, timeout including a large stdin write, graceful
cancellation, `cwd: session`, 128 KiB output cap; 11 execution tests are
Unix-only). `cargo test -p jcode --lib run_resume_uses_the_saved_session_working_dir`
covers the resumed-run directory.
Then a session rooted at a repo with `.jcode/tools.json` shows the declared tools
in `/context` and calls them.
`cargo test -p jcode-tool-core` covers the new trait default.
`cargo check -p jcode-app-core --profile selfdev` is the fast gate.
`e2e/run.py --binary <fork bin> --suite all` passes (23/23); the suite does not
yet exercise project tools. The ambient call sites are compile-covered; there is
no dedicated runner-level test for them yet.
C2 conflict hints: `tool/project.rs` is new and cannot conflict. `Tool::is_project_tool`
is an additive default. Project registration must stay *after* the fixed MCP tools at
each session site so `mcp`, `mcp_search`, and `mcp_call` cannot be shadowed. The call
sites are the subscribe and resume paths in `server/client_session.rs`, plus
`server/headless.rs`, `server.rs`, `server/worker_revive.rs`, the ambient runner
(resume-dead and scheduled-child paths, plus global-only for a session-less cycle),
and `src/cli/commands.rs` (resumed runs use the saved session directory). If
upstream adds a new session setup path, add `register_project_tools_for_dir` there.
The 2026-10-29 fix added reload identity, stripped `intent`, honoured `JCODE_HOME`,
decoupled project tools from every MCP opt-out, bounded the timeout around stdin, and
made the global file default to session cwd. The second 2026-10-29 fix bounded config
reads and tool counts, rejected unknown fields and malformed schemas, preserved stderr
on failures, honoured graceful shutdown, warned when a late MCP tool replaces a project
tool, and covered resumed and ambient sessions.
C3 best-way check: upstream `new_terminal` and Alt+Space next-prompt routing
open another session; `/clear` plus `/rename` needs two non-atomic commands and
leaves the session briefly unnamed, and no config or skill can reset a session
in place. Chosen layer: minimal additive fork code in the existing command
handlers plus a shared reset helper.
C3 verify: `cargo test -p jcode-tui --lib -- local_new remote_new new_is_registered local_clear_preserves`
(12 tests: local named/unnamed, boundary rejection, name trimming, registry
discovery, local/remote side-panel reset, `/clear` working dir, local/remote
busy refusal, deferred Clear acknowledgement, Clear rejection with no rename,
stale skill/improve state, and the quiet `/new` rename). Runtime:
tmux harness confirms `/new` changes the session title/id, `/new named session`
sets the title, `/resume` lists the named session, and `clear_done` logs a new id
on every call. Busy `/new` is refused client-side before any request is sent.
The 2026-10-29 review fix defers the remote reset and rename until the Clear
`Done`; a rejected Clear keeps the old transcript and never sends
`RenameSession`, remote reset clears skill/improve/provider state, and a named
`/new` no longer prints a second rename notice.
C3 conflict hints: the local handler sits beside `/clear` in
`handle_session_command`; the remote handler sits beside `/clear` in
`submit_remote_enter_input` and shares `reset_remote_view_for_new_session`.
The deferred path keys off the `Clear` request id in `ServerEvent::Done` and
`ServerEvent::Error`; the pending fields are initialized in both
`app/tui_lifecycle.rs` constructors. The registry and help entries are one line
each.
C4 verify: `cargo test -p jcode-base --lib usage::balance` (6: `/usage` rows
asserted byte-identical for DeepSeek and Moonshot while the same response
exposes the chip amount, string and numeric balances, malformed payloads error
instead of reporting zero, provider key/label resolution, silence for providers
without a balance API) plus the manual live round-trip
`cargo test -p jcode-base --lib usage::balance::tests::live -- --ignored --nocapture`
(no network in the default run), and
`cargo test -p jcode-tui --lib provider_balance` (5: label/low/exhausted
formatting, disable clears, provider switch drops the stale number, completed
query schedules the next poll, failure renders nothing and backs off to 120s),
and `cargo test -p jcode-config-types provider_balance_chip` (defaults on,
absent key stays on, explicit false disables), and
`cargo test -p jcode-tui --lib provider_balance_chip` (3 rendered-frame checks:
the label lands at `width - label - 1` on the status row, nothing renders on a
frame too narrow for it, and no chip means no extra chrome).
Runtime: a DeepSeek session shows `DeepSeek $NN.NN` at the right edge of the
status line, dim normally, amber under $5, red at zero; `[display]
provider_balance = false` hides it; no chip for providers without a balance API.
C4 conflict hints: the chip hook is the last three lines of `draw_status`
(before the helper), gated by `TuiState::provider_balance_chip` whose default is
`None` in `tui/mod.rs`; the poller refresh rides beside `refresh_justrust_status`
in `app/local.rs` and `app/remote.rs`. If upstream changes those lines, the
unique `refresh_provider_balance` call is the anchor.
C5 best-way check: no upstream question tool exists, no config, skill, or MCP
path can render a native structured dialog, and the blocking tool-to-client
interactive-input primitive already exists. Chosen layer: minimal additive fork
code extending that primitive.
C5 verify: `cargo test -p jcode-tool-types`, `cargo test -p jcode-protocol`,
`cargo test -p jcode-app-core ask_user_question`, `cargo test -p jcode-app-core
user_question`, `cargo test -p jcode-tui --lib question_overlay`, plus
`e2e/run.py --binary <bin> --suite all`. Runtime: a TUI session asks the model
to use `ask_user_question`, follows a converging branch, types a custom answer,
and reviews the result.
C5 conflict hints: the tool registry gate sits beside `set_desktop_ui_enabled`;
the capability apply helper sits beside `apply_or_defer_desktop_response_capability`;
the forwarder match sits in the existing `stdin_req_rx` loop; the TUI hooks sit
beside the session-picker hooks in the two key paths, `draw_inner`, and the two
App constructors. Protocol test struct literals are the first compile failure
when upstream adds a `Subscribe` field.
Build env: a host without `openssl-devel` needs `OPENSSL_DIR` pointing at a user-local
extract, or `--features linux-compat-vendored-openssl` with the perl modules installed;
`~/dotfiles/scripts/jcode-install.sh` extracts the headers itself when they are missing.

## Per-change entry (copy into the ledger and the commit message)

- The best-way check: which layer in `AGENTS.md` "Feature-request prudence" this
  change uses, and why the earlier layers do not fully work.
- What and why, in two lines.
- For fork code: the existing primitive or pattern it extends, and why extending
  that primitive is not enough.
- Upstream files/functions touched, and any upstream behaviour we rely on.
- How to verify it still works (command or manual check).
- Conflict hints: what to look for when upstream changes those files.

## Test gates

| Gate | Command | Time | Use |
| ---- | ------- | ---- | --- |
| TUI e2e (binary-only) | `e2e/run.py --binary <bin> --suite all` | seconds | Every change and every upgrade. Works on stock and fork. |
| Targeted unit | `cargo test -p jcode-at-file`, `cargo test -p jcode-tui --lib at_file` | under 1 min | Fork code. |
| Upstream mock e2e | `cargo test --test e2e` | 1-2 min after build | After upgrades. |
| Full `jcode-tui` lib suite | not a gate | 25+ min, hangs | Known: `tui::ui::tests::*` stall under parallel load, 6 other tests fail on this host. Run `baseline` against stock to tell upstream failures from ours. |

See `e2e/README.md`.

## Upgrade runbook

1. Read the upstream release notes/diff between `v<pinned>` and `v<new>`
   (`git log --stat v<pinned>..v<new>`, focusing on files in the ledger).
2. `git fetch upstream --tags master`; `git rebase upstream/master` on `master`.
   Resolve conflicts per ledger hints. Drop any change upstream now ships (mark
   `upstreamed`).
3. Run `e2e/run.py --suite baseline` on the new stock binary, then build the fork
   and run `--suite all`. Run `cargo test --test e2e`.
4. Update the ledger Base column. If a diff is needed, generate it with
   `git format-patch v<new>..HEAD`; do not commit it.
5. Push `master`, then bump `~/dotfiles/jcode/UPSTREAM_VERSION` to the new base
   and run `~/dotfiles/scripts/jcode-install.sh` on each machine.
6. Rollback: `~/dotfiles/scripts/jcode-install.sh --rollback-to <version>`; every
   installed build stays under `~/.jcode/builds/versions/`.

## Status

- Fork: `github.com/dis446/jcode`, default branch `master` (our commits on top of
  upstream master; base recorded in `~/dotfiles/jcode/UPSTREAM_VERSION` = 0.99.1).
  Checkout `~/Code/Personal/jcode`, `origin` = fork, `upstream` = `1jehuang/jcode`.
- Install is owned by dotfiles: `~/dotfiles/scripts/jcode-install.sh` clones/updates
  this repo at `master`, builds with cargo, and installs
  `~/.jcode/builds/versions/<ver>/` (plus the `stable` and `shared-server` symlinks).
  `--rollback-to <version>` switches to any installed build.
- Config, prompt overlay, skills and `mcp.json` live in dotfiles under `jcode/`.
