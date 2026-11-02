# Feature request: herdr pane must show `working` while jcode subagents run

Status: draft, not started. Owner: user. Target: local jcode (`v0.99.472-dev` and later).
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

Add to `App` (terminal_title.rs or a small `herdr_state.rs`):

```rust
fn has_delegated_work(&self) -> bool
fn herdr_agent_state(&self) -> AgentState {
    if self.is_processing() || self.has_delegated_work() { Working } else { Idle }
}
```

`sync_herdr_agent_state` calls `herdr_agent_state()` instead of `is_processing()` directly. Nothing
else in `herdr.rs` changes except the new message field.

### 2. Cost

`sync_herdr_agent_state` runs every idle tick and on every full draw, so `has_delegated_work` must
not scan the filesystem. Use only in-memory state:

- `remote_swarm_members` / `inline_swarm_members_unticked()` with `is_active_status`;
- `crate::background::global().running_snapshot()` (already used for the info widget, cheap: counts);
- `background_task_rows.iter().any(|r| r.status == Running)`;
- the await dependency needs disk (`await_dependency::running`), so cache a boolean that is
  refreshed when the corresponding state changes (swarm member update, background task event,
  await dependency resolution) rather than reading it per frame. If a cache is not worth it,
  drop signal 3 from v1: signals 1 and 2 cover the reported scenario.

The existing `sync()` dedupe means the extra call costs one comparison per tick.

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

- `crates/jcode-tui/src/tui/app/terminal_title.rs` — state derivation (`sync_herdr_agent_state`).
- `crates/jcode-tui/src/tui/herdr.rs` — optional `message` field + `report_args` + tests.
- Possibly `crates/jcode-tui/src/tui/app/tui_state.rs` — expose a cheap
  `active_swarm_member_count()` / `running_background_count()` helper if the fields are private to
  another module.

No server, protocol, or config change. No user-visible TUI change except the herdr sidebar.

## Verification

1. Unit tests in `herdr.rs`: `report_args` includes `--message` only when set; existing
   `valid_resume_argv` / seq tests still pass.
2. TUI unit test (same style as existing `app/tests`): construct an `App` with one synthetic
   `SwarmMemberStatus { status: "running" }` and assert `herdr_agent_state() == Working`; with all
   members `completed`, assert `Idle`; with a running background row, assert `Working`.
3. Manual in a herdr pane: spawn a long worker (`swarm spawn`), then
   `herdr agent list | jq '.[] | select(.pane_id=="<pane>")'` — status stays `working` for the whole
   worker run, flips to `idle` only after the last member completes, and the sidebar shows the
   message.
4. Regression: a normal conversational turn still shows `working` while streaming and `idle` after.
5. `cargo test -p jcode-tui`, `cargo fmt`, `cargo clippy -p jcode-tui`.

## Risks / notes

- Over-reporting: a stale active swarm member (server restart, lost completion event) would pin the
  pane to `working`. The member list already ages out via server status events, and herdr clears
  state on a new session report; still, verify with a stopped/cancelled worker that the state goes
  `idle`.
- `sync` dedupe compares `(state, session_id)`; adding a message makes it compare the message too,
  otherwise a changing message with the same state would be dropped. Either include it in the
  comparison or only send the message with a state transition.
- The current gate `!self.is_remote_client()` (`terminal_title.rs:68`) stays as is; this plan does
  not change which runtime modes report, only what they report.
