# jcode fork governance

This is the team-of-one fork of [`1jehuang/jcode`](https://github.com/1jehuang/jcode)
behind `github.com/dis446/jcode`. We run a **pinned upstream plus our own custom
work**; upgrading upstream is a deliberate, planned act. Nothing here updates
jcode automatically.

Everything about the fork lives **in this repo**: the ledger, the plans, the
exported patches and the TUI regression suite. The dotfiles repo only holds the
pin, the config and the install entry point.

## Rules

1. **This repo is the source of truth.** `~/dotfiles/scripts/jcode-install.sh`
   clones/updates this repo at its default branch (`master`) and builds it, so
   pushing here is what makes a version installable. `~/dotfiles/jcode/UPSTREAM_VERSION`
   records the upstream base release (no `v`) that `master` sits on, for planning
   the next upgrade.
2. **`jcode update` is banned.** Upgrades follow the runbook below. In-app update
   checks are off (`~/dotfiles/jcode/config.toml`: `check_updates = false`), and
   `up` in the shell never touches jcode.
3. **Every custom change is tracked** in the ledger below *before* it is merged,
   with its patch exported to `patches/`. No untracked local divergence.
4. **Remotes:** `origin` = this fork, `upstream` = `1jehuang/jcode`. `master`
   carries our commits on top of upstream `master`, kept as a small, rebased
   series; no long-lived feature branches.
5. **Keep patches small and isolated.** Prefer new files/modules and thin hooks
   into upstream code over editing existing functions. Smaller touch surface =
   fewer conflicts. Note the upstream files each change touches.
6. **Config/prompt/skills stay in dotfiles**, not here, wherever possible.

## Ledger of custom work

| ID | Title | Status | Base | Upstream files touched | Patch | Plan | Upstreamable |
| -- | ----- | ------ | ---- | ---------------------- | ----- | ---- | ------------ |
| C1 | Native `@file` picker (pi parity) | merged in `master`, installed | upstream `master` (b61e58b32, builds as 0.99.339-dev) | `jcode-tui`: `app.rs`, `app/input.rs`, `app/local.rs`, `app/remote.rs`, `app/remote/key_handling.rs`, `app/state_ui_input_helpers.rs` (`autocomplete`), `app/tui_lifecycle.rs`, `app/tui_state.rs`, `tui/mod.rs` (`TuiState::at_file_view`), `tui/ui.rs`, `tui/ui_input.rs`. New: `crates/jcode-at-file`, `app/at_file.rs`, `ui_input/at_file.rs`. Root `Cargo.toml` (workspace member) | [patches/](patches/) (`git format-patch upstream/master..HEAD`) | [plan](plans/at-file-picker.md) | yes (additive, 1 new crate + ~14 one-line hooks) |
| C2 | Project-defined tools (`.jcode/tools.json`) | merged in `master`, installed | upstream `master` (b61e58b32, builds as 0.99.339-dev) | `jcode-app-core`: `tool/mod.rs` (`mod project;`, `Registry::register_project_tools_for_dir`, one call at the top of `register_mcp_tools_for_dir`). New: `tool/project.rs`, `docs/PROJECT_TOOLS.md` | [patches/](patches/) | [doc](docs/PROJECT_TOOLS.md) | yes (new module + 2 thin hooks) |
| C3 | `/new [name]` slash command (pi parity) | merged in `master`, installed | upstream `master` (b61e58b32, builds as 0.99.345-dev) | `jcode-tui`: `app/commands.rs` (`handle_session_command`), `app/commands_review.rs` (`reset_current_session` keeps working dir), `app/remote/key_handling.rs` (shared reset helper + `/new`), `app/state_ui_input_helpers.rs` (registry), `ui_overlays.rs` (help). New: `app/tests/new_session_command.rs` | [patches/](patches/) | n/a | yes (additive slash command) |

Status: planned, in-progress, merged (in `master`), dropped, upstreamed
(delete from the series on next rebase). Add a row for every new change.

C1 verify: `e2e/run.py --binary <fork bin> --suite all` (13 `at_file` tests), plus
`cargo test -p jcode-at-file` (17) and `cargo test -p jcode-tui --lib at_file` (14).
C1 conflict hints: hooks sit beside `handle_command_suggestion_key` (two key paths: local
`input.rs` and remote `key_handling.rs`), `insert_input_text_with_undo` tail, the Backspace arms
in both paths, `autocomplete()` after `get_suggestions_for`, and the `draw_command_suggestions_overlay`
call in `ui.rs`. If upstream adds a third key path, the e2e suite fails first.

C2 verify: `cargo test -p jcode-app-core project::` (7 tests: load/merge/validation,
stdin JSON, non-zero exit, timeout, `cwd: session`, relative command resolution), then a
session rooted at a repo with `.jcode/tools.json` shows the declared tools in `/context`
and calls them. `cargo check -p jcode-app-core --profile selfdev` is the fast gate.
C2 conflict hints: the single hook is the first statement of
`Registry::register_mcp_tools_for_dir` in `tool/mod.rs`; if upstream splits MCP
registration per transport, add the project-tools call to each path that receives a
session `working_dir`. `tool/project.rs` is new and cannot conflict.
C3 verify: `cargo test -p jcode-tui --lib -- local_new remote_new new_is_registered`
(8 tests: local `/new`, local `/new <name>`, boundary rejection, name trimming,
registry discovery, local side-panel reset, remote side-panel reset, and remote
`Clear` then `RenameSession`). Runtime:
tmux harness confirms `/new` changes the session title/id, `/new named session`
sets the title, and `/resume` lists the named session.
C3 conflict hints: the local handler sits beside `/clear` in
`handle_session_command`; the remote handler sits beside `/clear` in
`submit_remote_enter_input` and shares `reset_remote_view_for_new_session`. The
registry and help entries are one line each.
Build env: a host without `openssl-devel` needs `OPENSSL_DIR` pointing at a user-local
extract, or `--features linux-compat-vendored-openssl` with the perl modules installed;
`~/dotfiles/scripts/jcode-install.sh` extracts the headers itself when they are missing.

## Per-change entry (copy into the ledger and the patch header)

- What and why, in two lines.
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
