# jcode fork governance

This is the team-of-one fork of [`1jehuang/jcode`](https://github.com/1jehuang/jcode)
behind `github.com/dis446/jcode`. We run a **pinned upstream plus our own custom
work**; upgrading upstream is a deliberate, planned act. Nothing here updates
jcode automatically.

Everything about the fork lives **in this repo**: the ledger, the plans, the
exported patches and the TUI regression suite. The dotfiles repo only holds the
pin, the config and the install entry point.

## Rules

1. **Pin.** `~/dotfiles/jcode/UPSTREAM_VERSION` (dotfiles) is the single source of
   truth for the base release (no `v`); the branch is `custom/<pin>`, cut from
   upstream tag `v<pin>`. `~/dotfiles/scripts/jcode-install.sh` clones/updates this
   repo at that branch and installs the build. Pushing a branch here is what makes
   a version installable.
2. **`jcode update` is banned.** Upgrades follow the runbook below. In-app update
   checks are off (`~/dotfiles/jcode/config.toml`: `check_updates = false`), and
   `up` in the shell never touches jcode.
3. **Every custom change is tracked** in the ledger below *before* it is merged,
   with its patch exported to `patches/`. No untracked local divergence.
4. **Remotes:** `origin` = this fork, `upstream` = `1jehuang/jcode`.
   Branch `custom/<base>` is upstream tag `v<base>` plus our commits, kept as a
   small, rebased series.
5. **Keep patches small and isolated.** Prefer new files/modules and thin hooks
   into upstream code over editing existing functions. Smaller touch surface =
   fewer conflicts. Note the upstream files each change touches.
6. **Config/prompt/skills stay in dotfiles**, not here, wherever possible.

## Ledger of custom work

| ID | Title | Status | Base | Upstream files touched | Patch | Plan | Upstreamable |
| -- | ----- | ------ | ---- | ---------------------- | ----- | ---- | ------------ |
| C1 | Native `@file` picker (pi parity) | merged in `custom/0.99.1`, installed | 0.99.1 | `jcode-tui`: `app.rs`, `app/input.rs`, `app/local.rs`, `app/remote.rs`, `app/remote/key_handling.rs`, `app/state_ui_input_helpers.rs` (`autocomplete`), `app/tui_lifecycle.rs`, `app/tui_state.rs`, `tui/mod.rs` (`TuiState::at_file_view`), `tui/ui.rs`, `tui/ui_input.rs`. New: `crates/jcode-at-file`, `app/at_file.rs`, `ui_input/at_file.rs`. Root `Cargo.toml` (workspace member) | [patches/](patches/) (4, `git format-patch v0.99.1..`) | [plan](plans/at-file-picker.md) | yes (additive, 1 new crate + ~14 one-line hooks) |

Status: planned, in-progress, merged (in `custom/<base>`), dropped, upstreamed
(delete from the series on next rebase). Add a row for every new change.

C1 verify: `e2e/run.py --binary <fork bin> --suite all` (13 `at_file` tests), plus
`cargo test -p jcode-at-file` (17) and `cargo test -p jcode-tui --lib at_file` (14).
C1 conflict hints: hooks sit beside `handle_command_suggestion_key` (two key paths: local
`input.rs` and remote `key_handling.rs`), `insert_input_text_with_undo` tail, the Backspace arms
in both paths, `autocomplete()` after `get_suggestions_for`, and the `draw_command_suggestions_overlay`
call in `ui.rs`. If upstream adds a third key path, the e2e suite fails first.
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
2. `git fetch upstream --tags`; `git checkout -b custom/<new> custom/<pinned>`;
   `git rebase --onto v<new> v<pinned>`. Resolve conflicts per ledger hints.
   Drop any change upstream now ships (mark `upstreamed`).
3. Run `e2e/run.py --suite baseline` on the new stock binary, then build the fork
   and run `--suite all`. Run `cargo test --test e2e`.
4. Re-export `patches/*.patch` (`git format-patch v<new>..HEAD`), update the
   ledger Base column.
5. Push `custom/<new>`, then bump `~/dotfiles/jcode/UPSTREAM_VERSION` to `<new>`
   and run `~/dotfiles/scripts/jcode-install.sh` on each machine.
6. Rollback: `~/dotfiles/scripts/jcode-install.sh --rollback-to <version>`; every
   installed build stays under `~/.jcode/builds/versions/`.

## Status

- Fork: `github.com/dis446/jcode`, branch `custom/0.99.1` (4 commits on `v0.99.1`).
  Checkout `~/Code/Personal/jcode`, `origin` = fork, `upstream` = `1jehuang/jcode`.
- Install is owned by dotfiles: `~/dotfiles/scripts/jcode-install.sh` clones/updates
  this repo at `custom/<pin>`, builds with cargo, and installs
  `~/.jcode/builds/versions/<ver>/` (plus the `stable` and `shared-server` symlinks).
  `--rollback-to <version>` switches to any installed build.
- Config, prompt overlay, skills and `mcp.json` live in dotfiles under `jcode/`.
