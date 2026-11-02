# Feature request: herdr pane must show `working` while jcode subagents run

Status: implemented in `master` (unit tests and `e2e --suite all` green; manual herdr-pane check
pending). Owner: user. Target: local jcode (`v0.99.472-dev` and later).
Area: `crates/jcode-tui` (herdr integration + TUI state), no server/protocol change.

## Problem

Running a swarm from jcode inside a herdr pane shows the pane as **idle** in herdr's Agents
sidebar / `herdr agent list` for most of the wall-clock time, even while one or more swarm workers
are actively running and the parent session is only waiting for them.

Observed 2026-10-30 on `w68:p2` (features/mobile-api-openapi-docs): dev worker `ladybug` was burning
tokens continuously for ~20 minutes, while `herdr agent list` alternated between `idle` and brief
`working` blips. The TUI's own swarm strip showed the worker as active the whole time.

Live confirmation: `herdr agent list` shows `w68:p2 jcode working` only while the parent is inside
a turn (e.g. this analysis), and `idle` seconds later while the worker's token count on
`swarm list` keeps climbing. State tracks the parent turn, not the delegated work.

Root cause: jcode reports herdr state from **one signal only** — `App::is_processing()`, which is
true only during a model turn / stream (`is_processing || pending_queued_dispatch ||
split_launch_in_flight`). A turn that ends to await a spawn/await/background result is *not*
processing, so jcode sends `idle` while delegated work continues. Nothing in the herdr path
consults the live swarm members, background tasks, or pending wake state that the TUI already
tracks for its widgets.

## Evidence (current source)

| Fact | Where |
|---|---|
| herdr report is driven only by `is_processing()` | `crates/jcode-tui/src/tui/app/terminal_title.rs:67-79` (`sync_herdr_agent_state`) |
| `is_processing()` = turn/stream/queued-dispatch/split-launch only | `crates/jcode-tui/src/tui/app/state_ui_runtime.rs:221-223` |
| `sync` dedupes identical `(state, session_id)` and is called on idle tick and every full draw | `crates/jcode-tui/src/tui/herdr.rs:126-155`; callers `app/local.rs:84`, `app/remote.rs:102`, `app/run_shell.rs:478` |
| Live swarm members + active status are already in TUI state | `crates/jcode-tui/src/tui/app/tui_state.rs:2038-2055` (`inline_swarm_members`), `jcode-tui-render/src/swarm_gallery.rs:54` (`is_active_status` = running/streaming/thinking) |
| Running background tasks are already in TUI state | `crates/jcode-tui/src/tui/app/tui_state.rs:1746-1758` (`bg_manager.running_snapshot()`), `background_task_rows` |
| Pending todo awaits / background dependencies are already tracked | `crates/jcode-tui/src/tui/app/state_ui_messages.rs:233-270` |
| Report payload has no message field used by jcode | `crates/jcode-tui/src/tui/herdr.rs:264-296` (`report_args`); herdr CLI supports `--message` |

The herdr contract (`https://herdr.dev/docs/add-herdr-support/`) is: report `working` when work
starts, `idle` when the agent is ready for input, `blocked` when it needs a decision; `--seq` must
increase; only the latest state matters.

## Goal

While a jcode pane's session has **delegated work in flight**, herdr must show that pane as
`working`. "Delegated work" = any of:

1. a swarm member whose status is active (`running` / `streaming` / `thinking`),
2. a running background task (`background::global().running_snapshot()` count > 0 or a
   `BackgroundTaskRow` in `Running`),
3. the session is parked on a wake/await (todo `waiting_on` an await member or a background task),
4. a running shell command launched through the TUI (already covered by processing in most paths).

When none hold, report `idle` as today. Optional: attach a short human message
(`--message "3 workers running"`) so the sidebar explains *why* it is working.

## Design

### 1. One derivation function, pure and cheap

Add to `App` in `terminal_title.rs`:

```rust
fn active_swarm_member_count(&self) -> usize   // tui_state.rs, inherent impl App
fn running_background_task_count(&self) -> usize
fn herdr_agent_status(&self) -> (AgentState, Option<String>) {
    let workers = self.active_swarm_member_count();
    let tasks = self.running_background_task_count();
    let state = if self.is_processing() || workers > 0 || tasks > 0 { Working } else { Idle };
    (state, herdr_work_message(workers, tasks))
}
```

`sync_herdr_agent_state` calls `herdr_agent_status()` instead of `is_processing()` directly.
`active_swarm_member_count` uses `inline_swarm_members_unticked()`, so only this session's own
swarm subtree counts, not every worker in a shared repository swarm.

### 2. Cost

`sync_herdr_agent_state` runs every idle tick and on every full draw, so the derivation must
not scan the filesystem. Use only in-memory state:

- `remote_swarm_members` via `inline_swarm_members_unticked()` with `is_active_status`;
- `background_task_rows.iter().filter(|r| r.status == Running).count()`. Rows are populated from
  task lifecycle events in both local and remote clients. `running_snapshot()` is not used: it
  reads a status file per running task, which is too much for a per-tick path;
- signal 3 (todo `waiting_on` an await member) is dropped from v1. It needs disk
  (`await_dependency::running`), and signals 1 and 2 already cover the reported scenario: a parent
  waiting on a swarm worker or background task is waiting on an active signal 1 or 2 entry.

The existing `sync()` dedupe means the extra call costs one comparison per tick. The scan itself
only runs inside a herdr pane (`herdr::enabled()`), so other terminals pay one environment check.

### 3. Message (optional, same change)

Add `message: Option<String>` to `StateReport` and pass `--message <text>` in `report_args`
(`crates/jcode-tui/src/tui/herdr.rs`). Compose it once, only when the state changes:
`"3 workers running"`, `"1 background task"`, etc. herdr normalizes/truncates it.

### 4. Workers do not report themselves

Do **not** make each headless swarm worker call `pane report-agent` for the same pane. herdr keys
agent state on `(pane, source, agent)` and the pane's resume command belongs to the interactive
session; multiple reporters from the same source would fight over one pane's state and `--seq`
ordering. Aggregation in the parent is the correct model, and it matches what the user sees (one
jcode agent per pane).

If per-worker visibility is later wanted, it belongs in a follow-up that tests herdr's multi-agent
per-pane behaviour with a distinct stable `--source` per worker, and only after confirming the
sidebar renders several agents on one pane. Out of scope here.

### 5. `blocked` stays unreported

No change: the interactive TUI never blocks a turn on a user decision (existing module doc,
`herdr.rs:1-10`). Delegated work is `working`, not `blocked`.

## Files to touch

- `crates/jcode-tui/src/tui/app/terminal_title.rs` — state derivation
  (`herdr_agent_status`, `running_background_task_count`, `herdr_work_message`) and tests.
- `crates/jcode-tui/src/tui/herdr.rs` — optional `message` field, `--message` in `report_args`,
  dedupe and unsupported-message fallback, tests.
- `crates/jcode-tui/src/tui/app/tui_state.rs` — `active_swarm_member_count()` using the existing
  own-subtree filter.

No server, protocol, or config change. No user-visible TUI change except the herdr sidebar.

## Verification

1. Unit tests in `herdr.rs`: `report_args` includes `--message` only when set; existing
   `valid_resume_argv` / seq tests still pass.
2. TUI unit tests in `app/terminal_title.rs`: construct an `App` with one synthetic
   `SwarmMemberStatus { status: "running" }` and assert `herdr_agent_status() == (Working, ...)`;
   with terminal statuses (`completed` / `failed` / `stopped` / `ready`), assert `Idle`; with a
   running background row, assert `Working` and the message; switch the row to completed and assert
   `Idle`.
3. Manual in a herdr pane: spawn a long worker (`swarm spawn`), then
   `herdr agent list | jq '.[] | select(.pane_id=="<pane>")'` — status stays `working` for the whole
   worker run, flips to `idle` only after the last member completes, and the sidebar shows the
   message.
4. Regression: a normal conversational turn still shows `working` while streaming and `idle` after.
5. Done: `cargo test -p jcode-tui --lib -- tui::herdr tui::app::terminal_title` (20 passed),
   `cargo fmt -p jcode-tui`, `cargo clippy -p jcode-tui --all-targets -- -D warnings`.

## Risks / notes

- Over-reporting: a stale active swarm member (server restart, lost completion event) would pin the
  pane to `working`. The member list already ages out via server status events, and herdr clears
  state on a new session report; still, verify with a stopped/cancelled worker that the state goes
  `idle`.
- `sync` dedupe now compares `(state, message, session_id)`, so a changing message is not dropped.
- `--message` is newer than the resume command. If a herdr version rejects it with a usage error,
  `send_report` drops resume first, then the message, retries each time, and remembers the
  rejection in `MESSAGE_UNSUPPORTED` so later reports skip the flag. State reporting therefore
  never depends on the message being understood.
- The current gate `!self.is_remote_client()` (`terminal_title.rs:68`) stays as is; this plan does
  not change which runtime modes report, only what they report.
