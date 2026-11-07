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
| C7 | End-to-end todo task list doc | merged in `master` (docs only) | upstream `master` (b61e58b32) | `docs/README.md` (entry-point link). New: `docs/TODO_TASK_LIST.md`. Documents the existing tool schema, persisted todo state, TUI surfaces, turn-end poke and quality-gate order, pause conditions, budgets, configuration, and the headless/SDK entry points. No runtime behavior changed. | n/a | yes (docs only) |
| C8 | Project-defined slash commands (`.jcode/commands.json`) | merged in `master`, installed | upstream `master` (b61e58b32, builds as 0.99.414-dev) | `jcode-protocol`: `wire.rs` (`InputShell` gains optional `cwd` and `timeout_secs`), `protocol_tests/misc_events.rs`. `jcode-app-core`: `lib.rs` (2 module declarations), `server/client_actions.rs` (`handle_input_shell` + `run_input_shell`, timeout/cancel), `server/client_lifecycle.rs` (request fields + shell cancel hook), `tool/project.rs` (shared config readers), `protocol_tests/misc_events.rs`. `jcode-tui`: `app.rs` (module + 2 fields + candidate cache key), `app/tui_lifecycle.rs` (2 inits, `prefill_input`), `app/state_ui_runtime.rs` (registry + `known_command_names`), `app/state_ui_input_helpers.rs` (completion), `app/commands_dispatch.rs` (names + local hook), `app/remote/key_handling.rs` + `app/remote/input_dispatch.rs` (remote hook, `run_remote_shell`), `app/remote.rs` (re-export), `app/input.rs` (`run_local_project_shell`), `app/input/shell_commands.rs` (local timeout), `app/tui_state.rs` + `tui/mod.rs` + `tui/ui_tests/mod.rs` (`project_command_entries`), `tui/ui_overlays.rs` (help section), `tui/backend.rs` (`send_input_shell_with`). Root: `src/cli/args.rs`, `dispatch.rs`, `ssh.rs`, `selfdev.rs`, `tui_launch.rs`. New: `project_config.rs`, `input_shell_registry.rs`, `command/{mod.rs,project.rs}`, `app/commands_project.rs`, `app/tests/project_commands.rs`, `docs/PROJECT_COMMANDS.md` | [plan](plans/native-project-slash-commands.md) | yes (new modules + thin hooks) |
| C9 | Markdown prompt templates as slash commands | merged in `master`, installed (0.99.414-dev) | upstream `master` (cedbffc9f, builds as 0.99.414-dev) | `jcode-base`: `lib.rs` (`pub mod prompt_template`). New: `prompt_template.rs` (four source roots, frontmatter with 60-char body fallback, name/size/limit validation, pi argv splitter, one-pass substitution). `jcode-tui`: `app.rs` (`mod commands_prompt` + `PromptTemplateCache` + field), `app/tui_lifecycle.rs` (2 inits), `app/state_ui_runtime.rs` (`prompt_templates` cache keyed on working dir, `reload_prompt_templates`, shadow filtering, `known_command_names`), `app/state_ui_input_helpers.rs` (completion), `app/input.rs` (expand after the `!cmd` branch, before skill resolution), `app/remote/input_dispatch.rs` (`submit_remote_template_prompt`, expansion in `submit_remote_slash_input`), `app/tui_state.rs` + `tui/mod.rs` + `tui/ui_tests/mod.rs` (`prompt_template_entries`), `tui/ui_overlays.rs` (/help section). New: `app/commands_prompt.rs`, `app/tests/prompt_templates.rs`. Root: `src/cli/acp.rs` (`acp_prompt_template_commands`, expansion in `run_prompt`), `src/cli/acp/tests.rs`. `e2e/run.py` + new `e2e/suite_prompt_templates.py`. New: `docs/PROMPT_TEMPLATES.md`. | [plan](plans/prompt-templates.md), [doc](docs/PROMPT_TEMPLATES.md) | yes (new module + thin hooks) |
| C10 | Model tool surface inventory doc | merged in `master` (docs only) | upstream `master` (3d16d77db, 0.99.1) | `docs/README.md` (entry-point link). New: `docs/TOOL_INVENTORY.md`. Lists the advertised tool surface for the current `[tools]` config, the runtime gates that withhold registered tools, and the four tool sources. No runtime behavior changed. | n/a | yes (docs only) |
| C11 | `ask_user_question` inline Pi-style dialog | merged in `master` | upstream `master` (3d16d77db, 0.99.1) | `jcode-tui`: `tui/ui.rs` (`draw_inner` paints the normal frame first, then composites the dialog in a 70%-height panel above the last status row; recursive draw flag skips the question pass), `tui/question_overlay.rs` (render takes a `Rect`; horizontal rules, intent plus answered/current tab strip, numbered options with description rows, review heading/Submit tab, dotted key hints; tests for panel bounds, tabs, descriptions). `docs/ASK_USER_QUESTION.md` layout section. Behavior follows `@juicesharp/rpiv-ask-user-question` in Pi: transcript and input stay visible instead of a full-frame takeover. | n/a | yes (UI-only refinement of C5) |
| C12 | Question dialog ignores stray keypad Escape | merged in `master` | upstream `master` (3d16d77db, 0.99.1) | `jcode-tui`: `tui/question_overlay.rs` (Esc arms cancellation and requires a second press within 2s; bursts under 300ms are one stray key; footer shows `esc again to cancel`; `KeypadBegin` selects like Enter). `docs/ASK_USER_QUESTION.md` key table and note. A numpad sequence that degrades to a double Escape previously closed the dialog and interrupted the turn. Ctrl+C still cancels immediately. | n/a | yes (small input-safety refinement of C5) |
| C13 | KV cache watcher doc | merged in `master` (docs only) | upstream `master` (3d16d77db, 0.99.1) | `docs/README.md` (entry-point link), `docs/HARNESS.md` (KV section links the new doc), `docs/INFO_WIDGET.md` (removes the KV cache panel from the catalog and merge list, records that the renderer is not placed). New: `docs/KV_CACHE_WATCHER.md`. Documents the counters and hit/yield ratios, the request fingerprints and baseline lifecycle, reusable-boundary accounting, provider noise floors, the full classification order, `[prefix unchanged]`, documented invalidations, daemon/client reconciliation, retention policy, `/cache stats`, transcript notices, status-line warnings, configuration, log fields, limits, and the code map. No runtime behavior changed. | n/a | yes (docs only) |
| C14 | `/upstream-bump` prompt template | merged in `master` (operator prompt) | upstream `master` (3d16d77db, 0.99.1) | `.gitignore` (unignore `/.jcode/prompts/` so repo-owned templates are trackable). New: `.jcode/prompts/upstream-bump.md`. Read-only workflow: establish the pinned/target state, summarize upstream changes touching every ledger row, state the gains, risk-rate each row and call site, then give a GO/NO-GO/DEFER verdict with an ordered rebase and verification plan. It follows the `FORK.md` upgrade runbook and never runs `jcode update`. Rule 7 keeps operator prompts in dotfiles; this one is fork-workflow-specific and was requested in-repo. No runtime behavior changed. | n/a | yes (operator config, no code) |
| C15 | Prompt templates appear without a restart | merged in `master` | upstream `master` (3d16d77db, 0.99.1) | `jcode-base`: `prompt_template.rs` (`load_for_working_dir` now walks `source_dirs`; new `source_dirs` and `source_stamp`, with a unit test for add/edit/remove). `jcode-tui`: `app.rs` (`PromptTemplateCache` and `CommandCandidatesCache` gain a stamp), `app/state_ui_runtime.rs` (`prompt_templates` reloads when the source stamp changes), `app/state_ui_input_helpers.rs` (`command_candidates` compares the same stamp before serving its cache), `app/tests/prompt_templates.rs` (new-file test warming both caches first). `docs/PROMPT_TEMPLATES.md` freshness wording. Before this, completion and `/help` cached the template list for the working dir, so a file added mid-session was only usable by typing its exact name (invocation reloaded) and never appeared as a slash command until a directory change. Fingerprint is dir path plus each `.md` file's path, length and mtime; 4 dir stats on a cache lookup. | n/a | yes (small additive freshness check) |
| C16 | `ask_user_question` stays in the foreground | merged in `master` | upstream `master` (3d16d77db, builds as 0.99.469-dev) | `jcode-app-core`: `agent/tool_handoff.rs` (`blocks_on_user_input`, early return in `auto_background_allowed`), `agent/turn_streaming_mpsc_handoff_tests.rs` (2 tests). `docs/ASK_USER_QUESTION.md` (foreground-only note). The 3 s foreground budget used to adopt every slow tool into the background; for a question that ended the tool call from the model's view while the TUI dialog still owned the client, so the TUI dismissed the dialog on `ToolDone` and the model continued without an answer. Questions now keep the turn in the foreground until answered, cancelled, or `timeout_secs` elapses. | [doc](docs/ASK_USER_QUESTION.md) | yes (one predicate branch) |
| C17 | herdr reports `working` while delegated work runs | merged in `master` | upstream `master` (3d16d77db, builds as 0.99.484-dev) | `jcode-tui`: `app/terminal_title.rs` (`herdr_agent_status` combines `is_processing()` with active swarm-subtree members and running background-task rows, and composes a compact sidebar message; `sync_herdr_agent_state` skips the derivation outside herdr), `app/tui_state.rs` (`active_swarm_member_count`, own subtree only), `tui/herdr.rs` (`StateReport.message`, `--message` in `report_args`, dedupe includes it, `MESSAGE_UNSUPPORTED` retry). No server/protocol/config change. herdr used to follow only `is_processing()`, so the pane went `idle` while a turn waited on swarm workers or background tasks that were still running. | [plan](plans/herdr-swarm-working-state.md) | yes (additive derivation + optional CLI flag) |
| C18 | ACP sessions advertise the full model catalog | merged in `master` | upstream `master` (3d16d77db, builds as 0.99.476-dev) | Root `src/cli/acp.rs` (`fill_model_catalog` + `SESSION_MODEL_CATALOG_TIMEOUT`, called at the end of `create_new_session_connected` and `attach_existing_session_connected`), `src/cli/acp/tests.rs` (mock daemon with a cold History). Layer 4 bug fix: the daemon gives History a 5 ms catalog budget (`HISTORY_MODEL_CATALOG_BUDGET`) and ships `available_models = []` when cold, expecting a follow-up `GetModelCatalog`. The TUI sends it; ACP never did, so `session/new` intermittently advertised only the current model and t3-code cached that. ACP now reuses the existing `request_model_catalog` (the `/models` path) before answering, bounded at 10 s, and keeps the History state on error or timeout. No protocol or daemon change. | [plan](plans/acp-full-model-catalog.md) | yes (client-side bug fix, send upstream) |
| C19 | ACP model switch routes to the native provider | merged in `master` | upstream `master` (3d16d77db, builds as 0.99.476-dev) | `jcode-base`: `provider/mod.rs` (new `active_catalog_profile_lacks_model`; the custom-endpoint short-circuit now yields for a known catalog profile that does not serve the id; the real-OpenRouter rebind rejects a bare native id with the `anthropic/<id>` alternative). `jcode-base/src/provider/tests/native_model_switch.rs` (new, 5 tests). Root `src/cli/acp.rs` (`initial_model_spec` / `apply_initial_model`, applied to new sessions so `-m/-p` work against a shared daemon), `src/cli/acp/tests.rs` (spec construction + mock-daemon SetModel). Root cause: the OpenCode Go slot is an OpenAI-compatible runtime with provider routing features off, so `set_model` kept `claude-opus-5-5` on the slot, then `set_model_on_provider(OpenRouter)` rebound to the real aggregator with a bare id. The switch succeeded and the first turn got a 400. Generic custom endpoints still own opaque ids, and a switch that cannot be routed now fails in `set_model`. | [plan](plans/acp-model-switch-provider-routing.md) | yes (small additive routing predicate + ACP initial-model hook) |
| C20 | ACP model catalog lists native ids first | merged in `master` | upstream `master` (3d16d77db, builds as 0.99.476-dev) | Root `src/cli/acp/session_configuration.rs` (new `ordered_available_models`, shared by `session_models` and `session_config_options`; inserts the current model when missing, then stable-sorts bare ids ahead of `vendor/model` ids), `src/cli/acp/tests.rs` (new ordering test). ACP hosts cap the catalog (t3-code keeps 256 entries plus current); OpenRouter publishes 411 `vendor/model` ids, so native/subscription ids such as `deepseek-v4.1-flash` and `claude-sonnet-5-5` fell past the cap and t3 rejected them as unavailable. The full catalog is still advertised, with every bare id inside the first entries. | [plan](plans/acp-catalog-order.md) | yes (one shared ordering helper) |

Status: planned, in-progress, merged (in `master`), dropped, upstreamed
(delete from the series on next rebase). Add a row for every new change.

Current series base: upstream tag `v1.0.0-preview.199` (`7fd42339c`,
2026-11-07 rebase; 238 upstream commits after `d33832674`). The `Base` column
above records the base at which each change was developed, not the current tip.
All 114 fork commits replayed. Upstream split several files for its size
ratchets, so fork hooks were re-applied by hand at the new sites:
`app/remote/key_handling.rs` -> `key_handling/key_events.rs` (C1 at-file key
and Backspace hooks, C5 question key hook), `ui_input::draw_status` ->
`stable_status::draw_status` (C4 chip, see C4 hints), `client_lifecycle.rs` ->
`client_processing.rs` (C8 shell cancel hook in `cancel_processing_message`),
new `client_lifecycle_tests/control_requests.rs` (C8 cancel test; the call now
takes a third `None` argument) and `mermaid_runtime/protocol_tests.rs` (C6
tests). Positional conflicts: `client_lifecycle.rs` (C5 capability apply kept
before upstream's `requested_continuation` init), `client_session.rs` (C2,
kept upstream's logged stub-load fallback), `usage.rs` (C4 `mod balance;`
beside upstream `mod banked;`), `misc_events.rs` (C5, both test blocks), safety
ratchet JSON (rebaselined in a final commit). No ledger row was dropped or
upstreamed. Upstream ACP still sends no `GetModelCatalog`, so C18 stays.

Pin: `~/dotfiles/jcode/UPSTREAM_VERSION` is `1.0.0-preview.199`, the tag the
series sits on, so the pin and the series base agree again. The rollback tag
`fork-pre-preview199` marks the previous series on `d33832674`.

2026-11-07 verify (preview.199): `cargo check --workspace --all-targets`; stock
baseline `e2e/run.py --suite baseline` 11/11; fork `--suite all` 38/38;
`cargo test --test e2e` 62 passed / 7 ignored; `cargo test --test provider_matrix`
10 passed; `scripts/test_ssh_slash_workflows.sh` passed. Every C1-C9 and C15-C18
targeted gate passed. The same three paused-clock `handoff_tests` fail
identically on stock preview.199. `chip_position` in the C4 frame test now
counts display cells, because the stable status row puts `◼`/`│` before the
chip. Size, panic and swallowed-error ratchets were rebaselined for the
combined tree. Relocated hooks are pinned by tests that go red when the hook
is removed (checked by temporarily disabling each one): `at_file_t_remote_*`
(C1), `remote_keys_reach_the_dialog_and_send_the_response` (C5),
`cancel_processing_message_cancels_registered_input_shells` (C8), and
`chip_keeps_the_stable_status_phase_visible_at_every_width` (C4).

2026-11-02 verify: `cargo check --workspace --all-targets`; stock baseline
`e2e/run.py --suite baseline` 11/11; fork `--suite all` 38/38;
`cargo test --test e2e` 62 passed / 7 ignored; `cargo test --test provider_matrix`
10 passed; `scripts/test_ssh_slash_workflows.sh` passed. Every C1-C9 and
C15-C17 targeted gate passed. Three paused-clock `handoff_tests`
(`nonstreaming_background_wait_returns_original_result_after_budget`,
`ask_user_waits_past_budget_and_returns_its_answer`,
`callback_paths_honor_custom_and_disabled_budgets`) fail identically on stock
`d33832674`. Clippy `-D warnings` is red on stock rustc 1.99.1: 20 app-core and
17 jcode-tui upstream lints. The fork has the same 20 app-core lints and 14 TUI
lints, so it adds none and still fixes three older TUI lints. Leave the clippy
gate disabled until upstream clears its rustc-1.99 lints.

C7 verify: docs only; no runtime gate applies. Repo-relative links in
`docs/TODO_TASK_LIST.md` and the `docs/README.md` entry resolve inside `docs/`.

C1 verify: `e2e/run.py --binary <fork bin> --suite all` (13 `at_file` tests), plus
`cargo test -p jcode-at-file` (17) and `cargo test -p jcode-tui --lib at_file` (16,
including `at_file_t_remote_*`, which drive `handle_remote_key` and fail if the
hooks in `key_handling/key_events.rs` go missing).
C1 conflict hints: hooks sit beside `handle_command_suggestion_key` (two key paths: local
`input.rs` and remote `key_handling/key_events.rs` since preview.199), `insert_input_text_with_undo` tail, the Backspace arms
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
After the 2026-11-02 rebase `worker_revive.rs` revives through
`SessionManager::get_or_start`, so keep project registration inside that closure
immediately after `register_mcp_tools_for_dir`. `SessionAgents` is an `Arc`
manager, not a `RwLock<HashMap>`, so direct `sessions.write()` code will not compile.
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
C4 verify: `cargo test -p jcode-base --lib usage::balance` (8: `/usage` rows
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
C4 conflict hints: since preview.199 upstream's `ui_input::draw_status` only
delegates to `stable_status::draw_status`, whose row pads to the full width. The
fork wraps that call: `provider_balance_chip_line` picks the label or a
fallback that leaves at least `PROVIDER_BALANCE_MIN_STATUS_WIDTH` (16) cells,
the stable row is drawn into the narrowed area, and the chip is drawn in the
reserved right edge. The chip is gated by `TuiState::provider_balance_chip`, whose default is
`None` in `tui/mod.rs`; the poller refresh rides beside `refresh_justrust_status`
in `app/local.rs` and `app/remote.rs`. If upstream changes those lines, the
unique `refresh_provider_balance` call is the anchor.
C8 best-way check: upstream has no repo-declared slash command surface. A skill
starts a model turn; a project tool needs the model to choose it and is absent
from a frozen tool list; operator config cannot add a native slash name. Chosen
layer: minimal additive fork code, two new app-core modules plus thin hooks on
the existing `Request::InputShell` path and the slash registry.
C8 verify: `cargo test -p jcode-app-core --lib command::project` (14 loader
tests), `cargo test -p jcode-app-core --lib input_shell` (9: shell timeout,
cancel, cwd, stderr/exit, clamped wire deadline, and Ctrl-C on a cancelled
turn reaching a running shell), `cargo test -p jcode-protocol --lib
input_shell`, and
`cargo test -p jcode-tui --lib -- project_command confirm_requires
initial_message_prefills built_in_command_wins resolver_builds`, and
`e2e/run.py --binary <fork bin> --suite all` (including the
`project_commands` suite, 7/7, which fails on stock by design; one test primes
completion with no commands file, adds `.jcode/commands.json`, and expects the
next use to resolve it without a client restart). Runtime: a
session rooted at a repo with `.jcode/commands.json` lists the commands in
`/help` and completion; invoking one prints its shell card with exit code and
duration and logs no provider stream event; `confirm: true` waits for `y`;
Ctrl-C kills a running command. An empty command list is never cached, so a
commands file that appears mid-session is picked up on the next use.
C8 conflict hints: the client registry hook is
`App::project_commands` in `app/state_ui_runtime.rs`; dispatch hooks are the
last `||` arm of `dispatch_single_local_command` and the top of
`submit_remote_enter_input` / `submit_remote_slash_input`. `TuiState` gains
`project_command_entries` next to `available_skills` (3 implementations). The
wire change is additive (`#[serde(default)]` on both new `InputShell` fields), so
an older peer still decodes the request. `InputShell` has one other call site:
`RemoteConnection::send_input_shell_with`.
After the 2026-11-02 rebase `run_tui_client` gains upstream's turn-envelope
handling and keeps the fork's `initial_message` argument. Both `src/cli/ssh.rs`
and `src/cli/tui_launch.rs` change. Upstream host-owned SSH slash workflows now
own more of the remote dispatch, so re-check project commands on local and SSH
paths rather than assuming the old `input_dispatch` route is unchanged.
C8 known gap (2026-10-30 rebase): upstream now snapshots per-session subprocess
environment (`session_env`) for bash tool launches, but `run_input_shell` has no
session id, so project command shells and the TUI local `!cmd` path do not
inherit it. Project commands still see the daemon environment. Wiring the
session id through those two spawn sites is a small follow-up if the override
must apply there.
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
beside the session-picker hooks in the two key paths (remote: top of
`handle_remote_key_internal` in `key_handling/key_events.rs`), `draw_inner`, and the two
App constructors. Protocol test struct literals are the first compile failure
when upstream adds a `Subscribe` field. The question-wire tests appended at the
end of `jcode-protocol/src/protocol_tests/misc_events.rs` conflict positionally
with any upstream test added there; keep both blocks and restore the closing
`Ok(())`/`}` of the upstream test.
After the 2026-11-02 rebase the TUI negotiates `supports_turn_identity: true`.
The interactive forwarder still sends `UserQuestionRequest` flat, but a scoped
copy can also arrive inside `TurnEvent`. Keep the dialog arm after the unwrap
boundary. TUI and ACP `Subscribe` literals now include `supports_turn_identity`.
C9 best-way check: upstream has no prompt-template concept. A skill injects a
standing system reminder and cannot substitute arguments into a user message;
a project command runs a shell line and starts no turn; prompt-overlay.md is
session-wide. Chosen layer: minimal additive fork code, one loader module plus
thin hooks on the existing slash dispatch, completion, help and ACP paths.
C9 verify: `cargo test -p jcode-base --lib prompt_template` (13 loader, argv and
substitution tests), `cargo test -p jcode-tui --lib -- template_ built_in_and_project
unknown_slash_text` (13), `cargo test -p jcode --lib acp` (25, including
template advertisement), and `e2e/run.py --binary <fork bin> --suite all`
(37/37, including the 7 `prompt_templates` scenarios that fail on stock by
design). Runtime: a template in `~/.agents/prompts` or the repo's
`.jcode/prompts` completes as `/name`, expands with `$1`/`$@`/`${1:-default}`,
shows under /help "Prompt templates", and a file added mid-session needs no
restart. Substitution is pi's exact table (checked against
`dist/core/prompt-templates.js` in the installed package): missing positions
become empty, `${@:N}`/`${@:N:L}` are supported, and there is no `\$` escape.
C9 conflict hints: the TUI cache mirrors `project_commands_cache` in
`app/state_ui_runtime.rs`; expansion hooks sit immediately before skill
resolution in `app/input.rs::submit_input` and at the top of
`app/remote/input_dispatch.rs::submit_remote_slash_input`. The local `!cmd`
branch above the expansion hook is what keeps a bang-leading body from running
as a shell line; the remote path uses `submit_remote_template_prompt` for the
same reason. `TuiState` gains `prompt_template_entries` next to
`project_command_entries` (2 impls). ACP templates resolve in `run_prompt`
before `resolve_acp_skill_invocation` and are advertised by
`acp_prompt_template_commands`, filtered against the `ACP_BUILTIN_COMMAND_NAMES`
const and same-named skills.
After the 2026-11-02 rebase both ACP `Subscribe` literals also carry
`supports_turn_identity`; keep `supports_user_question` beside it.
C16 best-way check: `ask_user_question` is fork code (C5); its blocking
semantics are decided by the upstream foreground-budget policy in
`auto_background_allowed`, which no config or skill can change for one tool.
Chosen layer: one predicate in the existing policy, no new mechanism.
C16 verify: `cargo test -p jcode-app-core --lib handoff_tests` (the new
`interactive_question_waits_in_the_foreground` passes; the pre-existing
`nonstreaming_background_wait_returns_original_result_after_budget` is red on
stock too, a paused-clock offset unrelated to this change),
`cargo test -p jcode-tui --lib -- tui::app::tests::question` (dialog opens,
another tool finishing keeps it, the question's own completion dismisses it and
reveals the queued one), `cargo test -p jcode-app-core --lib -- tool::batch`
and `tool::codemode` (a nested question is rejected; both tests red without the
guard), `cargo test --test e2e
ask_user_question_round_trip_through_server` (holds the question 4 s and fails
if a `ToolDone` arrives first; red on stock, green here),
`reload_interrupts_pending_question_without_hanging` (same red/green, then a
reload while the tool is still waiting must reach a terminal client state),
plus `cargo test -p jcode-app-core ask_user_question` and
`e2e/run.py --binary <bin> --suite all`. Runtime: ask the model a question in
the TUI; the dialog stays up past 3 s, the model waits, and the answer returns.
C16 full-suite baseline: `cargo test -p jcode-app-core --lib` is 1844 passed /
4 failed / 17 ignored with the change. All four are load- or host-dependent and
unrelated (accepted reload recovery, provider failover model id, post-tool
feedback hook, todo schema token cap). Reverting the product files gives 1839
passed / 9 failed: the same unrelated set (a slightly different subset under
load) plus the 4 new red-on-stock tests, so the change adds no failure.
C16 conflict hints: `blocks_on_user_input` sits above `auto_background_allowed`
in `agent/tool_handoff.rs`; both the streaming turn (`turn_streaming_mpsc.rs`)
and the blocking/SDK paths (`turn_loops.rs`, `execute_with_background`) go
through that predicate, so a new tool-registry entry point that forks the
policy must call it too. Any new interactive tool must also be added to
`tool/codemode.rs` `EXCLUDED` and `tool/batch.rs` `INTERACTIVE_SUBCALLS`, or a
script/batch wrapper can still be backgrounded around it.
After the 2026-11-02 rebase upstream also guards `name != "ask_user"` in the same
predicate. Keep both guards. `TOOL_FOREGROUND_BUDGET` is now
`foreground_budget()` from `[tools] auto_background_timeout_ms`, so tests and
call sites must use the configured value.
C17 verify: `cargo test -p jcode-tui --lib -- tui::herdr
tui::app::terminal_title` (20 passed; active and terminal swarm statuses,
running and completed background rows, message formatting, `--message` argument
order, and both unsupported-field fallbacks), `cargo fmt -p jcode-tui`,
`cargo clippy -p jcode-tui --all-targets -- -D warnings`. Runtime in a herdr
pane: spawn a long worker and `herdr agent list` keeps `working` while its
token count climbs, then flips to `idle` after it completes; a normal streamed
turn still reports `working` and then `idle`.
C17 conflict hints: the derivation is `App::herdr_agent_status` in
`app/terminal_title.rs`, called from the two `handle_tick` paths and
`draw_full` through `herdr::sync(state, message, session)`. Any new
delegated-work source (await/wake, shell) must be added to that derivation, and
any new `StateReport` constructor must set `message`.
C18 verify: `cargo test -p jcode --lib cli::acp` (includes
`acp_sessions_fetch_full_model_catalog_when_history_is_cold`: new and resumed
sessions with an empty History list get the full catalog, and a failing
catalog request still returns a session). Runtime: restart the daemon, then
run `initialize` + `session/new` over `jcode acp` repeatedly; every response
lists the full catalog in `configOptions[model]` and `models.availableModels`.
C18 conflict hints: if upstream starts sending `GetModelCatalog` from ACP
itself, or makes History wait for the catalog, drop C18. If session setup in
`src/cli/acp.rs` gains another daemon-attach path, call `fill_model_catalog`
there too.
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
  upstream `v1.0.0-preview.199`; base release recorded in
  `~/dotfiles/jcode/UPSTREAM_VERSION` = 1.0.0-preview.199).
  Checkout `~/Code/Personal/jcode`, `origin` = fork, `upstream` = `1jehuang/jcode`.
- Install is owned by dotfiles: `~/dotfiles/scripts/jcode-install.sh` clones/updates
  this repo at `master`, builds with cargo, and installs
  `~/.jcode/builds/versions/<ver>/` (plus the `stable` and `shared-server` symlinks).
  `--rollback-to <version>` switches to any installed build.
- Config, prompt overlay, skills and `mcp.json` live in dotfiles under `jcode/`.
