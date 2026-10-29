# jcode fork governance

This is the team-of-one fork of [`1jehuang/jcode`](https://github.com/1jehuang/jcode)
behind `github.com/dis446/jcode`. We run a **pinned upstream plus our own custom
work**; upgrading upstream is a deliberate, planned act. Nothing here updates
jcode automatically.

Custom work is deliberate and minimal. The default answer to a feature request
is an existing upstream feature, an existing codebase primitive, or operator
configuration, not new Rust. Adding Rust to the fork is a high bar.

Everything about the fork lives **in this repo**: the ledger, the plans, the
exported patches and the TUI regression suite. The dotfiles repo only holds the
pin, the config and the install entry point.

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
4. **Every custom change is tracked** in the ledger below *before* it is merged,
   with its patch exported to `patches/`. No untracked local divergence.
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

| ID | Title | Status | Base | Upstream files touched | Patch | Plan | Upstreamable |
| -- | ----- | ------ | ---- | ---------------------- | ----- | ---- | ------------ |
| C1 | Native `@file` picker (pi parity) | merged in `master`, installed | upstream `master` (b61e58b32, builds as 0.99.339-dev) | `jcode-tui`: `app.rs`, `app/input.rs`, `app/local.rs`, `app/remote.rs`, `app/remote/key_handling.rs`, `app/state_ui_input_helpers.rs` (`autocomplete`), `app/tui_lifecycle.rs`, `app/tui_state.rs`, `tui/mod.rs` (`TuiState::at_file_view`), `tui/ui.rs`, `tui/ui_input.rs`. New: `crates/jcode-at-file`, `app/at_file.rs`, `ui_input/at_file.rs`. Root `Cargo.toml` (workspace member) | [patches/](patches/) (`git format-patch upstream/master..HEAD`) | [plan](plans/at-file-picker.md) | yes (additive, 1 new crate + ~14 one-line hooks) |
| C2 | Project-defined tools (`.jcode/tools.json`) | merged in `master`, installed | upstream `master` (b61e58b32, builds as 0.99.339-dev) | `jcode-tool-core`: `lib.rs` (`Tool::is_project_tool` default). `jcode-app-core`: `tool/mod.rs` (`mod project;`, `Registry::register_project_tools_for_dir` + `_with_global`), `server/client_session.rs` (subscribe + two resume paths), `server/headless.rs`, `server.rs`, `server/worker_revive.rs`. Root `src/cli/commands.rs`. New: `tool/project.rs`, `docs/PROJECT_TOOLS.md`. Project registration is independent of MCP and runs after the fixed MCP tools at each session site. | [patches/](patches/) | [doc](docs/PROJECT_TOOLS.md) | yes (new module + thin hooks) |
| C3 | `/new [name]` slash command (pi parity) | merged in `master`, installed | upstream `master` (b61e58b32, builds as 0.99.345-dev) | `jcode-tui`: `app/commands.rs` (`handle_session_command`), `app/commands_review.rs` (`reset_current_session` keeps working dir), `app/remote/key_handling.rs` (shared reset helper + `/new`), `app/state_ui_input_helpers.rs` (registry), `ui_overlays.rs` (help). New: `app/tests/new_session_command.rs` | [patches/](patches/) | n/a | yes (additive slash command) |
| C4 | Live provider credit chip (DeepSeek/Moonshot balance) | merged in `master`, installed | upstream `master` (b61e58b32, builds as 0.99.356-dev) | `jcode-base`: `usage.rs` (`mod balance;` + re-export), `usage/api_keys.rs` (`configured_key` becomes `pub(super)`; the DeepSeek/Moonshot balance fetch+parse bodies move into `usage/balance.rs`, and `/usage` calls the same functions the chip does). `jcode-config-types`: `display.rs` (`provider_balance`). `jcode-tui`: `tui/mod.rs` (module + `TuiState::provider_balance_chip`), `app.rs` (module + field), `app/tui_lifecycle.rs` (2 inits), `app/tui_state.rs`, `app/local.rs`, `app/remote.rs`, `ui_input.rs` (`draw_status` right-aligned chip). New: `jcode-base/src/usage/balance.rs`, `tui/provider_balance.rs`, `app/provider_balance.rs` | [patches/](patches/) | n/a | yes (new modules + thin hooks) |

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
C2 verify: `cargo test -p jcode-app-core project::` (19 tests: load/merge/validation,
invalid-entry isolation, global/project override, global-only when no working dir,
global `session` cwd default, repo `project` cwd default, real-path symlink resolution,
global config-root command resolution, `JCODE_HOME` path resolution, reload/removal,
existing-tool collision, stdin JSON, intent stripping and opt-in, non-zero exit,
timeout including a large stdin write, `cwd: session`, 128 KiB output cap; 9
execution tests are Unix-only), then a session rooted at a repo with
`.jcode/tools.json` shows the declared tools in `/context` and calls them.
`cargo test -p jcode-tool-core` covers the new trait default.
`cargo check -p jcode-app-core --profile selfdev` is the fast gate.
C2 conflict hints: `tool/project.rs` is new and cannot conflict. `Tool::is_project_tool`
is an additive default. Project registration must stay *after* the fixed MCP tools at
each session site so `mcp`, `mcp_search`, and `mcp_call` cannot be shadowed. The call
sites are the subscribe and resume paths in `server/client_session.rs`, plus
`server/headless.rs`, `server.rs`, `server/worker_revive.rs`, and `src/cli/commands.rs`.
If upstream adds a new session setup path, add `register_project_tools_for_dir` there.
The 2026-10-29 fix added reload identity, stripped `intent`, honoured `JCODE_HOME`,
decoupled project tools from every MCP opt-out, bounded the timeout around stdin, and
made the global file default to session cwd.
C3 verify: `cargo test -p jcode-tui --lib -- local_new remote_new new_is_registered`
(10 tests: local `/new` and named, boundary rejection, name trimming, registry
discovery, local/remote side-panel reset, local/remote busy refusal, and remote
`Clear` then `RenameSession`). Runtime:
tmux harness confirms `/new` changes the session title/id, `/new named session`
sets the title, `/resume` lists the named session, and `clear_done` logs a new id
on every call. Busy `/new` is refused client-side before any request is sent.
C3 conflict hints: the local handler sits beside `/clear` in
`handle_session_command`; the remote handler sits beside `/clear` in
`submit_remote_enter_input` and shares `reset_remote_view_for_new_session`. The
registry and help entries are one line each.
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
Build env: a host without `openssl-devel` needs `OPENSSL_DIR` pointing at a user-local
extract, or `--features linux-compat-vendored-openssl` with the perl modules installed;
`~/dotfiles/scripts/jcode-install.sh` extracts the headers itself when they are missing.

## Per-change entry (copy into the ledger and the patch header)

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
4. Re-export `patches/*.patch` (`git format-patch v<new>..HEAD`), update the
   ledger Base column.
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
