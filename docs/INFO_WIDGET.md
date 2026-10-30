# Info widget (TUI side overview)

The info widget is the stack of small framed cards drawn in the free margins
beside the transcript: `Overview`, `Commits`, `Changes`, usage bars, memory,
Mermaid diagrams, SSH cloud details. It is display-only: nothing here has a tool
or a protocol request of its own, and it never edits the conversation.

This is not `docs/PANELS.md`. That document covers the model-facing `panel` tool
and desktop side panels. The info widget is a TUI margin overlay driven by state
the client already has.

## Where it sits

- Widgets are drawn **inside the messages rectangle**, in the empty width beside
  the rendered text, never in a reserved column. Long lines make the free width
  profile shrink, so a widget has to fit around the actual content.
- Width is clamped to **24-40 columns** (`MIN_WIDGET_WIDTH`,
  `MAX_WIDGET_WIDTH`), height to at least **5 rows** (`MIN_WIDGET_HEIGHT`) plus
  each widget's own minimum. Narrow terminals therefore show fewer widgets
  instead of wrapping chrome.
- In centered layout mode both margins are usable; in left-aligned mode only the
  right margin is.
- Only one widget pass runs per frame, after the transcript is drawn and before
  late overlays such as the command palette.

## Pipeline

1. `draw_messages` renders the transcript and returns `Margins`: free width per
   row for each side, an optional look-ahead ("reliable") profile, the current
   `scroll_top`, and whether content is still churning.
2. `app.info_widget_data()` (`crates/jcode-tui/src/tui/app/tui_state.rs`) builds
   the `InfoWidgetData` snapshot from session state.
3. `info_widget::calculate_placements` picks the kinds that currently have data,
   sorts them by effective priority, and docks them into free rectangles.
   Placement state (anchors) lives in a process-global `WidgetsState`, so it
   survives frames.
4. `info_widget::render_all` draws each placement; a placement overlapping the
   chat scrollbar gets its thumb redrawn on top so borders and thumb do not
   erase each other.
5. The pass is skipped entirely when onboarding is showing, the swarm page is
   active, the idle donut animation has taken the pane, a side panel is
   fullscreen, the terminal was just cleared, or `InfoWidgetData` is empty.

## Widget catalog

| Kind | Side | Priority | Min rows | What it shows |
| --- | --- | --- | --- | --- |
| `Diagrams` | right | 0 | 10 | Mermaid diagrams from the turn |
| `WorkspaceMap` | right | 1 | 1 | Niri-style workspace/session map |
| `Overview` | right | 2 | 8 | Joined detail sections (below) |
| `UsageLimits` | left | 5 | 3 | Subscription quota bars |
| `KvCache` | left | 6 | 3 | Prompt-cache hit ratio and misses |
| `MemoryActivity` | right | 7 | 3 | Memory graph/activity summary |
| `ModelInfo` | left | 8 | 1 | Service tier, route, transport, throughput, session |
| `Compaction` | left | 9 | 3 | Conversation compaction status |
| `BackgroundTasks` | left | 10 | 2 | Backgrounded tool progress |
| `GitStatus` ("Changes") | left | 11 | 1 | Which files are dirty, max 5 rows + `+N more` |
| `CloudVm` | left | 12 | 16 | Native SSH machine and workspace details |
| `Commits` | left | 12 | 1 | Recent commits on the branch, 5 rows + `+N more`, `↑N unpushed` |
| `SwarmStatus` | left | 12 | 3 | Not rendered as a dock today (see below) |
| `AmbientMode` | left | 13 | 3 | Disabled today |
| `Tips` | left | 14 | 3 | Disabled today |

Priority is a *sort key*, not a reservation: a lower-priority widget appears only
when its own rectangle still fits after higher-priority ones are placed.

Two kinds are permanently disabled by `InfoWidgetData::widget_disabled`:
`AmbientMode` and `Tips`. `SwarmStatus` reports no data on purpose — managed
agents are already shown by the pinned progress view and the inline swarm strip,
so the dock does not duplicate them. When a `SwarmStatus` dock *was* recently
placed, the inline strip stands down (`swarm_strip_stands_down_for_dock`) so the
same agents are not listed twice.

Status-line facts (model, branch, context %, dirty counts) belong to the status
line, not to widgets. `ModelInfo` and `Changes` explicitly omit what the status
line already shows.

## Overview merging

`Overview` is the join mode. When at least two of its detail sections have data,
it absorbs compatible widgets instead of letting them dock separately
(`is_overview_mergeable`): swarm, background tasks, compaction, model info, usage
limits, KV cache, git changes. Memory deliberately stays out, because the memory
renderer wants its own box and graph.

A single section does not trigger the overview — one section docks as its own
smaller widget.

## Pages

A widget can hold more than one page. The layout pass computes a compact page and,
when it fits, an expanded memory page; a taller-than-pocket overview falls back to
its compact page truncated from the tail, since sections are ordered by
importance.

- Pages rotate automatically every **30 s** (`PAGE_SWITCH_SECONDS`), per widget,
  whenever more than one page fits.
- Page dots are drawn on the bottom border and consume no row.

## Placement: residents, not floating cards

Widgets behave like HUD residents pinned to a transcript line rather than cards
re-solved from scratch each frame. That is what stops them from jumping while you
scroll.

- **Anchors.** Each placement records its absolute transcript line. When the
  transcript scrolls, the widget rides that line instead of holding a screen row.
- **Settled space only.** New widgets may dock only into rows whose free width
  has been unchanged for `SETTLE_AFTER_FRAMES` (3) consecutive frames
  (`info_widget_settle.rs`). The streaming tail and re-rendering markdown are
  never settled, so a widget cannot land where a line is about to grow.
- **Look-ahead widths.** Phase 2 docks against the reliable width profile when it
  admits a rectangle at all, falling back to instantaneous widths when long lines
  cover every candidate row (`dock_widths_with_fallback`).
- **Hysteresis.** A placed widget keeps its slot while the slot can still show it
  at >= 24 columns, shrinking instead of teleporting. It only re-homes after its
  slot has been unusable for `MAX_HIDDEN_FRAMES` (120) frames, a backstop for
  content that permanently grew over it.
- **Churn awareness.** While a turn is streaming or a scroll animation is in
  flight, a covered widget waits in place instead of re-homing into the middle of
  the churn.
- `info_widget_stability.rs` scores widget movement and flicker across synthetic
  and live scroll frames, so layout changes can be measured rather than eyeballed.

## Data sources

All of it comes from state the client already holds; no extra request is issued
for the widget.

- Context/model/transport: context snapshot, remote provider metadata, observed
  API-reported context tokens, tokens per second, connection type.
- Usage bars: subscription usage for the active route, with
  `display.usage_display = "used"` switching bars from remaining to consumed.
- Memory: `MemoryInfo` activity and last trace.
- Background tasks: `BackgroundInfo` progress.
- Git: `git_info` / `dual_git_info` snapshots (local and, over SSH, remote), plus
  paths the agent edited this session (`agent_edited`) to mark agent changes.
- Commits: recent branch history, hidden once the newest commit is older than 24 h.
- KV cache: cache-read ratio and miss attributions when the provider reports
  cache telemetry.
- Compaction: mode, message counts, summary size, in-progress flag.
- Cloud: machine telemetry, present only for native SSH attachments.
- Diagrams: Mermaid blocks when `features.mermaid` is on.

Values that arrive asynchronously keep their last good reading instead of
blinking away while a probe is in flight (`info_widget_cache.rs`,
`info_widget_settle.rs`).

## Toggle and configuration

- **Toggle:** `alt+i` by default. Config key
  `keybindings.info_widget_toggle`. Toggling shows a `Info widget: ON/OFF`
  status notice.
- The enabled flag is **in-memory only**: it resets to enabled on restart. There
  is no slash command and no config key that pins widgets, reorders them, or
  fixes the width.
- `display.usage_display` chooses used vs remaining quota bars.
- `features.mermaid` controls the `Diagrams` widget.

## Debugging

- Visual placement overlay: `visual_debug::overlay_enabled()` draws the chosen
  rectangles.
- Frame capture records `capture.layout.widget_placements` plus an
  `InfoWidgetCapture` summary, and flags anomalies: a widget intruding into
  content, out of bounds, overlapping the diagram area, or overlapping another
  widget.

## Code map

| File | Role |
| --- | --- |
| `crates/jcode-tui/src/tui/info_widget.rs` | Data struct, `WidgetKind`, priority/side/min height, placement entry point, page state, render dispatch |
| `info_widget_layout.rs` | Margin-aware placement, anchors, width/height clamps, hysteresis |
| `info_widget_overview.rs` | Page layout (compact vs expanded memory), overview height |
| `info_widget_settle.rs` | Per-line free-width settlement tracking |
| `info_widget_stability.rs` | Movement/flicker metrics and scroll simulation |
| `info_widget_cache.rs`, `info_widget_settle.rs` | Last-good values and flicker control |
| `info_widget_overview.rs`, `_usage`, `_memory_render`, `_model`, `_git`, `_commits`, `_graph`, `_cloud`, `_swarm_gallery`, `_swarm_background`, `_tips`, `_text`, `_frame` | Per-widget renderers |
| `crates/jcode-tui/src/tui/app/tui_state.rs` (`info_widget_data`) | Builds the snapshot from session state |
| `crates/jcode-tui/src/tui/ui.rs` | Calls the pass after the transcript and before late overlays |

## Related docs

- `docs/MULTI_SESSION_CLIENT_ARCHITECTURE.md` — agreed role of the workspace-map
  widget.
- `docs/SWARM_ARCHITECTURE.md` — swarm and plan widgets.
- `docs/AMBIENT_MODE.md` — ambient widget content (kind currently disabled).
- `docs/MEMORY_ARCHITECTURE.md`, `docs/MEMORY_BUDGET.md` — memory data behind the
  memory widget.
- `docs/TODO_TASK_LIST.md` — todo stages, which render in the transcript and
  status line, not in a margin widget.
