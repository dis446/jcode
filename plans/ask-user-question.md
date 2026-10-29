# Plan: `ask_user_question` with a branching question tree

Status: implemented on `feat/ask-user-question`, in verification.
Owner: jcode fork (ledger row C5).
Scope: one new tool (`ask_user_question`) plus its TUI dialog. Supporting
protocol and server plumbing only, no second tool.
Target: this fork's `master` on the pinned upstream base.

## Goal

The model can ask the user a decision question instead of guessing. The question
arrives as an interactive TUI dialog with authored options, and the user's
answers return to the model as structured data.

Hard requirements:

1. **One and only one new tool.** No companion tools, no note tool, no preview
   tool, no confirmation tool.
2. **No hardcoded limit on the number of questions.** A single call may carry as
   many question nodes as the model sends. No `maxItems`, no fixed array, no
   `MAX_QUESTIONS` constant, no `take(n)` truncation.
3. **No hardcoded limit on the number of options per question.** A question may
   carry as many authored options as the model sends. The dialog virtualizes and
   scrolls instead of truncating.
4. **Tree model with convergence.** The next question depends on the answer to
   the current one. Two different answers may lead to different next questions,
   or to the same next question.
5. **TUI rendering is part of the change.** The dialog must be usable without
   reading the transcript, and it must not wrap UI chrome.

Non-goals (v1):

- Notes on answers, per-question or global.
- Per-option Markdown preview panes.
- Desktop rendering. Desktop has its own repository and capability flag.
- Persisting a partially answered questionnaire across reconnect. A disconnect
  ends the pending tool call.
- Replacing plain chat questions. Prose questions still work.

## Reference behavior

`@juicesharp/rpiv-ask-user-question` for Pi gives the model one tool that opens a
terminal questionnaire. The useful parts to copy are the typed options with
descriptions, a custom-answer row on every question, one interruption per call,
and structured answers back to the model.

Deliberate differences:

| rpiv | This plan |
| --- | --- |
| Up to four questions in tabs | Unlimited sequential questions following a tree |
| Two to four options per question | Unlimited options, virtualized list |
| Flat question list | Question nodes with `next` edges, shared nodes allowed |
| Notes and previews | Out of scope for v1 |

## What jcode has today (the seams)

All paths are relative to the repository root.

- **Tool trait and context**: `crates/jcode-tool-core/src/lib.rs` defines `Tool`,
  `ToolContext`, `ToolExecutionMode`, and `StdinInputRequest`. `ToolContext`
  already carries an optional `stdin_request_tx` for tools that need something
  from the client, and a `graceful_shutdown_signal` for cancellation.
- **Tool registration and gating**: `crates/jcode-app-core/src/tool/mod.rs`.
  `Registry::set_desktop_ui_enabled` adds and removes `user_response` and
  `applet` based on a client capability. `base_tools()` holds stateless tools.
- **Capability application**: `crates/jcode-app-core/src/agent/turn_execution.rs`
  has `set_desktop_response_capability`, and
  `crates/jcode-app-core/src/server/client_session.rs` has
  `apply_or_defer_desktop_response_capability`, which applies or defers the
  update while a turn holds the agent lock.
- **Subscribe capabilities**: `crates/jcode-protocol/src/wire.rs` sends
  `supports_user_response`, `supports_pdf_panels`, and similar flags.
  `crates/jcode-app-core/src/server/client_lifecycle.rs` consumes them and has
  two apply sites (fresh subscribe and target attach).
- **Tool to client request path**: `client_lifecycle.rs` creates
  `stdin_req_tx`/`stdin_req_rx`, stores the response sender in `stdin_responses`,
  and emits `ServerEvent::StdinRequest`. `Request::StdinResponse` resolves it in
  `crates/jcode-app-core/src/server/client_actions.rs`
  (`handle_stdin_response`).
- **TUI event loop**: `crates/jcode-tui/src/tui/app/remote/server_events.rs`
  (`handle_server_event`) and `crates/jcode-tui/src/tui/app/remote.rs` (the
  `RemoteRead::Event` loop, where responses can be awaited).
- **TUI input routing**: local `crates/jcode-tui/src/tui/app/input.rs`
  (`handle_key_core`, `handle_modal_key`) and daemon path
  `crates/jcode-tui/src/tui/app/remote/key_handling.rs`
  (`handle_remote_key_internal`, already async).
- **Overlay pattern**: `crates/jcode-tui-session-picker`, wired through
  `App::session_picker_overlay`, `TuiState::session_picker_overlay`, the early
  branches in `handle_modal_key` and `handle_remote_key_internal`, the
  `draw_inner` early return in `crates/jcode-tui/src/tui/ui.rs`, and the two App
  constructors in `crates/jcode-tui/src/tui/app/tui_lifecycle.rs`.
- **Backend command**: `crates/jcode-tui/src/tui/backend.rs` builds `Subscribe`
  and already has `send_stdin_response`.
- **Sanitizer**: `crates/jcode-tui/src/tui/app/input/terminal_reports.rs`
  (`strip_terminal_control_sequences`, re-exported from `app/input.rs`).
- **Tool docs precedent**: `docs/user-response.md` documents a
  capability-gated, client-rendered tool.

Gaps to fill: structured question payloads on the wire, a capability-gated
registry entry, a response request, a queued TUI dialog, and the renderer.

## Prudence ladder (why this is fork code)

1. **Upstream behaviour or config**: jcode has no model-facing question tool. It
   only has unstructured prose questions.
2. **Existing primitive**: the blocking tool-to-client request path exists for
   `stdin_request_tx`, `StdinRequest`, and `StdinResponse`. This plan extends
   that primitive with a second payload kind instead of inventing a parallel
   request system.
3. **Operator config**: a skill, MCP server, or prompt cannot render a native
   dialog with options and return structured answers.
4. **Minimal additive fork code**: one new tool module, one new TUI module, and
   thin hooks into the existing protocol, server, registry, and overlay paths.
5. **New subsystem**: not needed.

Chosen layer: **4**, extending the existing interactive-input primitive.

## Data model

Shared serde types live in `crates/jcode-tool-types/src/lib.rs` so both
`jcode-tool-core` and `jcode-protocol` can use them without a cycle.
`jcode-protocol` gains a dependency on `jcode-tool-types`.

```rust
pub struct QuestionOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
    pub next: Option<String>,
}

pub struct QuestionNode {
    pub id: String,
    pub header: Option<String>,
    pub question: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,   // default false
    pub allow_custom: bool,   // default true
    pub next: Option<String>,
}

pub struct QuestionAnswer {
    pub question_id: String,
    pub selected: Vec<String>,  // option values, empty for custom-only
    pub custom: Option<String>,
}

pub enum UserQuestionResult {
    Answered { answers: Vec<QuestionAnswer> },
    Cancelled { reason: Option<String> },
}
```

The tool input shape:

```json
{
  "intent": "Choose the implementation path",
  "root": "task",
  "questions": [
    {
      "id": "task",
      "header": "Task",
      "question": "Which development task are we planning?",
      "options": [
        {"value": "bug", "label": "Bug fix", "description": "Fix a defect in existing behaviour.", "next": "bug_area"},
        {"value": "feature", "label": "New feature", "description": "Add a capability that does not exist yet.", "next": "feature_area"},
        {"value": "refactor", "label": "Refactor", "description": "Change structure without changing behaviour.", "next": "testing"}
      ],
      "next": "testing"
    },
    {
      "id": "bug_area",
      "question": "Which area is broken?",
      "options": [
        {"value": "tui", "label": "TUI", "next": "testing"},
        {"value": "daemon", "label": "Daemon", "next": "testing"}
      ]
    },
    {
      "id": "feature_area",
      "question": "Which area grows?",
      "options": [
        {"value": "tui", "label": "TUI", "next": "testing"},
        {"value": "daemon", "label": "Daemon", "next": "testing"}
      ]
    },
    {
      "id": "testing",
      "header": "Testing",
      "question": "How much testing do you want?",
      "multi_select": true,
      "options": [
        {"value": "unit", "label": "Unit tests"},
        {"value": "integration", "label": "Integration tests"},
        {"value": "e2e", "label": "End-to-end tests"}
      ]
    }
  ]
}
```

`root` is optional and defaults to the first entry. `questions` has no maximum.
`options` has no maximum. Both branches above converge on `testing`, which is
allowed. An optional sibling `timeout_secs` bounds the wait. Omitting it means
wait indefinitely, and no default is imposed.

The TUI gets `QuestionNode`, `QuestionOption`, and `UserQuestionResult` through
the existing `crate::protocol` re-export (`jcode-base/src/protocol.rs` forwards
`jcode_protocol`), so `jcode-tui` needs no new dependency.

### Branch semantics

At runtime the dialog starts at `root`, asks that node, records one answer, then
selects the next node:

1. If the question is `multi_select`, use the node's `next`.
2. Otherwise, if the chosen option has `next`, use it.
3. Otherwise use the node's `next`.
4. Otherwise the questionnaire ends.

A custom answer on a single-select question follows rules 3 and 4, so a model
that wants custom answers to branch must set a node-level `next`.

A node may be reached from several different options. That is required for the
"may be the same" case. A node must not be reachable from itself, because a
cycle would never end.

### Validation rules

Validation is iterative, not recursive, so a very deep chain cannot overflow the
stack. It returns an `anyhow` error to the model before any UI request is sent.

- `questions` must not be empty.
- `root` must resolve to a node.
- Every `id` is non-empty and unique.
- Every option `value` is non-empty and unique within its question.
- Every option-level and node-level `next` must reference an existing node.
- A node with no options must set `allow_custom: true` and must not be
  `multi_select`, otherwise it has no way to be answered.
- `multi_select: true` must not use option-level `next`.
- The graph must be acyclic. A gray node encountered during DFS is a cycle and
  the error names the path.
- Every node must be reachable from `root`. Unreachable nodes are reported by id.
  This is a correctness check, not a cap.

No `maxItems`, `maxLength`, `maxProperties`, or count checks are added anywhere
in the schema.

## Protocol changes

`crates/jcode-protocol/src/wire.rs`:

- Add `supports_user_question: bool` to `Request::Subscribe`, defaulting to
  false and skipped when false, matching the other capability flags.
- Add `Request::UserQuestionResponse { id, request_id, result: UserQuestionResult }`.

`crates/jcode-protocol/src/wire/events.rs`:

- Add `ServerEvent::UserQuestionRequest { request_id, tool_call_id, intent, root,
  questions: Vec<QuestionNode> }`.

No cancellation event is added. The TUI closes the dialog on the matching
`ToolDone` (`id` is the tool call id), on `Interrupted`, on `Done`, on `Error`,
and on disconnect. That avoids a second event type and keeps one source of truth
for the tool lifecycle.

Compatibility: the server only emits `UserQuestionRequest` to a client that
subscribed with `supports_user_question: true`. Old clients never see it. Old
servers never receive `UserQuestionResponse` because they do not have the tool.

## Reusing the interactive-input primitive

`StdinInputRequest` is already the "tool asks the client" primitive. Extend it
rather than adding a new `ToolContext` field. A new field would force an edit to
every `ToolContext` literal in the workspace (dozens of test constructors) and
guarantee conflicts on every upstream merge.

In `crates/jcode-tool-core/src/lib.rs`:

```rust
pub enum InteractiveInputKind {
    Text,                    // existing stdin prompt
    Questions(QuestionRequest),
}

pub struct QuestionRequest {
    pub intent: String,
    pub root: String,
    pub questions: Vec<QuestionNode>,
}

pub enum InteractiveInputResponse {
    Text(String),
    Questions(UserQuestionResult),
}

pub struct InteractiveInputRequest {
    pub request_id: String,
    pub prompt: String,       // retained for Text compatibility
    pub is_password: bool,
    pub kind: InteractiveInputKind,
    pub response_tx: oneshot::Sender<InteractiveInputResponse>,
}
```

Rename `StdinInputRequest` to `InteractiveInputRequest` and update the few
construction and test sites (`crates/jcode-app-core/src/tool/bash.rs`,
`bash_tests.rs`, `server/client_lifecycle.rs`). Re-export the new names from
`crate::tool` exactly as `StdinInputRequest` is exported today. Keep
`ToolContext.stdin_request_tx` as the single channel.

Alternative considered and rejected: adding `user_question_tx` to `ToolContext`.
It is semantically cleaner but touches every context literal and creates a
second tool-to-client path, which violates the existing-primitive rule.

## Server flow

`crates/jcode-app-core/src/server/client_lifecycle.rs`:

1. On connection setup, keep the existing `stdin_req_tx` channel.
2. Wrap the client's question capability in `Arc<AtomicBool>` so the forwarder
   sees Subscribe updates.
3. The forwarder inserts `req.response_tx` into `stdin_responses` as today, and
   matches `req.kind`:
   - `Text`: emit `ServerEvent::StdinRequest` unchanged.
   - `Questions(payload)`:
     - If the capability is false, immediately resolve the request with
       `Cancelled { reason: Some("client cannot render questions") }`.
     - Otherwise emit `ServerEvent::UserQuestionRequest` with the tool call id.
4. `Request::StdinResponse` wraps the input in `InteractiveInputResponse::Text`.
5. Add `Request::UserQuestionResponse` handling next to `handle_stdin_response`,
   resolving the sender with the result and replying `Done { id }`.
6. Change the `stdin_responses` map type to
   `HashMap<String, oneshot::Sender<InteractiveInputResponse>>`.

`crates/jcode-app-core/src/server/client_actions.rs`:

- Update `handle_stdin_response` to send `InteractiveInputResponse::Text`.
- Add `handle_user_question_response` with the same shape.

## Capability gating

Mirror `user_response` exactly.

- `Registry::set_interactive_questions_enabled(bool)` registers or unregisters
  `ask_user_question`. It is not in `base_tools()`.
- `Agent::set_interactive_questions_capability(bool)` invalidates `locked_tools`
  and calls the registry, following `set_desktop_response_capability`.
- `apply_or_defer_interactive_questions_capability(agent, enabled)` in
  `server/client_session.rs`, called at both existing capability apply sites in
  `client_lifecycle.rs`.
- TUI sets `supports_user_question: true` in its `Subscribe` in
  `crates/jcode-tui/src/tui/backend.rs`.
- SDK, harness bridge, Desktop, and old clients default to false.

Consequence: `jcode run`, `-p`, MCP-only clients, and Desktop never see the tool.
If the tool is invoked with no channel anyway (direct execution or a stale
capability), it returns an error telling the model to ask in prose.

Known limitation, inherited from `user_response`: the registry capability is
per session, so the last Subscribe wins when several clients attach. A Desktop
attach after a TUI attach would remove the tool. Fixing that requires per-client
capability tracking and is out of scope.

## Tool implementation

New file `crates/jcode-app-core/src/tool/ask_user_question.rs`, registered from
`Registry::set_interactive_questions_enabled`. It is stateless, like
`user_response`.

- `name`: `ask_user_question`.
- `description`: state when to ask (a decision that changes the work and cannot
  be discovered from the repo), and when not to ask (confirmation, facts you can
  check, questions with an obvious default). Mention that options are unlimited
  and that `next` branches, and that the user can always type a custom answer
  unless disabled.
- `parameters_schema`: the shape above. No caps.
- `execute`:
  1. Deserialize with `deny_unknown_fields`.
  2. Validate the graph with the rules above.
  3. Take `ctx.stdin_request_tx`. If absent, return a clear error: no interactive
     client is attached, ask the user in prose.
  4. Generate `request_id` as `format!("ask-{}-{}", ctx.tool_call_id, nonce)`,
     using a process-wide atomic counter.
  5. Send `InteractiveInputRequest { kind: Questions(...), response_tx, .. }`.
  6. Wait with `tokio::select!` on the response, an optional `timeout_secs`, and
     `ctx.graceful_shutdown_signal.notified()`. This mirrors
     `browser_fast::bounded` and `applet`'s wait loop.
  7. On `Answered`, return a compact JSON object with `status`, `answers`, and
     `path` (the ordered question ids) as `ToolOutput`, with title
     `Question answered`.
  8. On `Cancelled`, return `{"status":"cancelled","reason":"..."}` as a normal
     result, not an error, so the model can respond gracefully.
  9. On interrupt or client disconnect, return a normal cancelled result with the
     reason.
- Optional input `timeout_secs` has no default. Omission means wait indefinitely.
  Codemode scripts are capped at 300 seconds, so the description tells the model
  to pass a smaller timeout when calling from codemode.

Single question per call? No. One call carries the whole tree, and the UI walks
it. That keeps one tool call and one interruption.

## TUI dialog

New file `crates/jcode-tui/src/tui/question_overlay.rs`, following the
session-picker pattern. Keep the pure flow state and the ratatui rendering in
one module, with unit tests that do not need a terminal.

### State

```rust
pub struct QuestionOverlay {
    request_id: String,
    tool_call_id: String,
    intent: String,
    nodes: HashMap<String, QuestionNode>,
    root: String,
    current: String,
    path: Vec<AnswerFrame>,       // one entry per answered node on this path
    phase: Phase,                 // Asking | Review
    selected: usize,
    scroll: usize,
    multi_selected: BTreeSet<usize>,
    editing_custom: bool,
    custom: String,
    custom_cursor: usize,
    answers: Vec<QuestionAnswer>,
}

struct AnswerFrame {
    node_id: String,
    selected: usize,
    multi_selected: BTreeSet<usize>,
    custom: Option<String>,
}
```

`App` gains:

```rust
question_overlay: Option<RefCell<QuestionOverlay>>,
pending_questions: VecDeque<QuestionRequestEnvelope>,
```

`QuestionRequestEnvelope` holds the request id, tool call id, intent, root, and
nodes. If a second request arrives while one is open, it is queued, never
dropped. The next one opens after submit, cancel, or tool completion.

### Layout

Full-frame overlay, like the session picker (`draw_inner` returns early before
the transcript). One logical row per chrome row, never wrapped. All model text
is sanitized with `strip_terminal_control_sequences` before it reaches the
overlay.

```
Ask user question                                       1 answered
Which development task are we planning?

> Bug fix            Fix a defect in existing behaviour.
  New feature        Add a capability that does not exist yet.
  Refactor           Change structure without changing behaviour.
  Type something.

up/down move  enter select  left back  esc cancel
```

- Header row: intent, right-aligned answered count. Truncate with an ellipsis.
- Progress row: `Question N` plus a truncated breadcrumb such as
  `Task: Bug fix > Area: TUI`. No tabs and no total count, because the tree
  makes the total unknown.
- Prompt: wrapped prose, maximum three rows with internal scroll for the rest.
- Options: computed from the frame height. Virtualized scrolling with the
  selected row always visible. No `take(n)`, no truncation of the option vector.
  Show `↑ N more` and `↓ N more` markers when scrolled.
- Each option row: marker, label, dim description. Descriptions wrap within the
  row and the row height is computed in display cells, not characters.
- Custom row: always last unless `allow_custom` is false. When selected, Enter
  opens an inline editor. Enter submits that question. Esc returns to the list
  without losing typed text. Shift+Enter inserts a newline where the terminal
  reports it; Ctrl+J is the fallback.
- Multi-select: `Space` toggles, `[x]` markers, Enter confirms the set.
- Footer: non-wrapping key hints, truncated with an ellipsis at narrow widths.
  This is chrome, not prose.
- Review phase: virtualized list of every answer, `Submit` and `Back` rows.
  Back from review returns to the last question with its selection restored.
  Back from a question pops one frame and truncates later answers.

No hardcoded limits appear in rendering. The only bound is the visible row
count, which is derived from terminal height. Review scrolls the same way.

### Keys

| Key | Asking | Review |
| --- | --- | --- |
| Up / Down, j / k | move selection | scroll |
| PageUp / PageDown, Home / End | move by page / jump | scroll |
| Enter | select or confirm option | submit |
| Space | toggle multi-select | |
| Left / Backspace | previous question | previous question |
| Esc | cancel questionnaire | back to last question |
| Ctrl+C | cancel questionnaire | back to last question |
| Typed text | only while editing custom | |

`Esc` sends `Cancelled` and closes the dialog. `Ctrl+C` is intercepted before
the global interrupt handler so the tool call resolves cleanly instead of
leaving the dialog open.

### Wiring

- `crates/jcode-tui/src/tui/mod.rs`: `pub mod question_overlay;` and
  `TuiState::question_overlay()`.
- `crates/jcode-tui/src/tui/app.rs`: the two fields above.
- `crates/jcode-tui/src/tui/app/tui_lifecycle.rs`: initialize both fields at the
  two App constructor sites (around the existing `session_picker_overlay: None`).
- `crates/jcode-tui/src/tui/app/tui_state.rs`: implement the accessor.
- `crates/jcode-tui/src/tui/ui.rs`: render the overlay in `draw_inner` before the
  transcript, after the existing modal overlays, following the session-picker
  branch.
- `crates/jcode-tui/src/tui/app/input.rs`: `handle_modal_key` checks the
  question overlay first.
- `crates/jcode-tui/src/tui/app/remote/key_handling.rs`: the same check at the
  top of `handle_remote_key_internal`. This path is async, so it sends the result
  immediately with `remote.send_user_question_response(...)`.
- `crates/jcode-tui/src/tui/app/remote/server_events.rs`: handle
  `ServerEvent::UserQuestionRequest` by sanitizing and enqueueing. Handle
  `ToolDone` with a matching `tool_call_id`, `Interrupted`, `Done`, `Error`, and
  disconnect by clearing the overlay and opening the next queued request.
- `crates/jcode-tui/src/tui/backend.rs`: add `supports_user_question: true` to
  `Subscribe`, and `send_user_question_response(request_id, result)`.
- Local `App` mode (`run_turn_interactive`) is not wired. The interactive TUI
  uses `App::new_for_remote` and the daemon path. Local mode never enables the
  capability, so the tool is absent there.

### Response sending failure

If sending the response fails, keep the overlay open and set a status notice.
The connection-disconnect path will clear it. Do not silently drop the answer.

## Security and robustness

- Sanitize every model-authored string before rendering it. Question prompts,
  labels, descriptions, headers, and intent are provider output and must not be
  able to emit terminal control sequences.
- Labels and descriptions are plain text. Do not feed them through Markdown
  rendering in v1.
- The answer payload is user input. Return it as data in the tool result. Do not
  interpret it as instructions inside the tool.
- Request ids are unique per invocation with a process-wide atomic counter.
- Unknown or stale response ids are ignored, matching `handle_stdin_response`.
- A response after the tool has returned is harmless: the map entry is gone and
  the request is ignored.
- The only size bound is the existing 256 MiB protocol frame cap. No new count
  bound is added. A model that sends an enormous tree pays for it in tokens and
  transport before the UI ever sees it.

## File-by-file change list

New files:

- `crates/jcode-app-core/src/tool/ask_user_question.rs`
- `crates/jcode-tui/src/tui/question_overlay.rs`
- `docs/ASK_USER_QUESTION.md`
- `plans/ask-user-question.md` (this file)

Modified files:

- `crates/jcode-tool-types/src/lib.rs`: question types.
- `crates/jcode-tool-types/Cargo.toml`: add `serde`.
- `crates/jcode-tool-core/src/lib.rs`: interactive input enum, rename, re-export.
- `crates/jcode-tool-core/src/lib.rs` tests: rename.
- `crates/jcode-protocol/Cargo.toml`: depend on `jcode-tool-types`.
- `crates/jcode-protocol/src/wire.rs`: Subscribe flag, response request.
- `crates/jcode-protocol/src/wire/events.rs`: question request event.
- `crates/jcode-app-core/src/tool/mod.rs`: module, gated registration, re-export.
- `crates/jcode-app-core/src/tool/bash.rs`: `Text` kind and response type.
- `crates/jcode-app-core/src/tool/bash_tests.rs`: same.
- `crates/jcode-app-core/src/agent/turn_execution.rs`: capability setter.
- `crates/jcode-app-core/src/server/client_session.rs`: apply or defer helper.
- `crates/jcode-app-core/src/server/client_lifecycle.rs`: capability flag,
  forwarder, response map, two apply sites.
- `crates/jcode-app-core/src/server/client_actions.rs`: question response
  handler.
- `crates/jcode-app-core/src/protocol_tests/*`: Subscribe literal fields.
- `crates/jcode-tui/src/tui/mod.rs`: module, trait accessor.
- `crates/jcode-tui/src/tui/app.rs`: overlay fields.
- `crates/jcode-tui/src/tui/app/tui_lifecycle.rs`: two constructor sites.
- `crates/jcode-tui/src/tui/app/tui_state.rs`: accessor.
- `crates/jcode-tui/src/tui/app/input.rs`: modal routing.
- `crates/jcode-tui/src/tui/app/remote/key_handling.rs`: async modal routing.
- `crates/jcode-tui/src/tui/app/remote/server_events.rs`: request and cleanup.
- `crates/jcode-tui/src/tui/backend.rs`: Subscribe flag, response sender.
- `crates/jcode-tui/src/tui/ui.rs`: draw branch.
- `crates/jcode-tui/src/tui/app/input/terminal_reports.rs`: visibility only if
  the sanitizer needs to be reachable from the overlay module.
- `FORK.md`: ledger row C5.

## Phases

### Phase 1: types and protocol

1. Add the serde question types and their unit tests (roundtrip, unknown field
   rejection, defaults).
2. Add `supports_user_question` to `Subscribe` and the three protocol tests.
3. Add the request and response wire variants and roundtrip tests.
4. Add the `jcode-protocol` to `jcode-tool-types` dependency.

Exit: `cargo test -p jcode-tool-types -p jcode-protocol`.

### Phase 2: tool and validation

1. Extend `StdinInputRequest` to `InteractiveInputRequest` with the kind and
   response enums. Update bash and its tests.
2. Implement `ask_user_question` with iterative validation, traversal, waiting,
   and result formatting.
3. Unit tests: schema has no caps, 200 nodes and 500 options accepted, duplicate
   ids, duplicate values, unknown next, cycle with path, unreachable node, empty
   options with custom disabled, multi-select with option next, custom-only
   branch, deep chain without stack overflow, cancel, timeout, interrupt,
   no-channel error.

Exit: `cargo test -p jcode-tool-core -p jcode-app-core ask_user_question`.

### Phase 3: server and capability

1. Add the registry gate and agent capability setter.
2. Add the apply-or-defer helper and both call sites.
3. Extend the forwarder and response map.
4. Tests: capability false means the tool is absent; capability true means it is
   present; a request emits the event; a response resolves the tool; an
   unsupported client gets a cancelled result; a duplicate response is ignored;
   locked tools invalidate on capability change.

Exit: `cargo test -p jcode-app-core user_question`.

### Phase 4: TUI state and rendering

1. Add the overlay state machine and tests for branch traversal, convergence,
   back, multi-select, custom text, review, and cancel.
2. Add the renderer with a `TestBackend` snapshot test for long lists, narrow
   widths, and Unicode widths.
3. Wire App fields, constructors, `TuiState`, and `draw_inner`.

Exit: `cargo test -p jcode-tui --lib question_overlay`.

### Phase 5: TUI input and response

1. Route keys in local and remote paths.
2. Send responses from the async remote path.
3. Clear on `ToolDone`, interrupt, done, error, and disconnect.
4. Queue a second request instead of dropping it.
5. Manual check with a real turn.

Exit: `cargo test -p jcode-tui --lib question` and a scripted dialog run.

### Phase 6: docs, ledger, full verification

1. Write `docs/ASK_USER_QUESTION.md`.
2. Add the C5 ledger row and its verification line.
3. Run `e2e/run.py --binary <bin> --suite all`.
4. Build and reload with `selfdev build-reload`, then exercise a branching
   question in a real session.
5. Capture frames with `debug_socket` for the ask, scrolled, custom, and review
   states.

## Tests

Tool:

- The advertised schema contains no `maxItems` or `maxLength` under
  `questions` or `options`.
- A 200-question chain validates and traverses without truncation.
- A question with 500 options validates, and the TUI can select the last one.
- Duplicate ids, duplicate option values, unknown `next`, cycles, unreachable
  nodes, and multi-select with option-level `next` all return specific errors.
- Converging branches (`bug_area -> testing` and `feature_area -> testing`)
  validate and produce the correct path.
- Custom answers are returned in `QuestionAnswer.custom`, with an empty selected
  list.
- Cancel, timeout, interrupt, and client disconnect return a cancelled result.
- No channel returns an instructional error.

Protocol and server:

- `Subscribe` defaults `supports_user_question` to false.
- A `UserQuestionRequest` roundtrips with a large node list.
- A `UserQuestionResponse` roundtrips for both answered and cancelled results.
- Capability false: tool absent from the registry and definitions.
- Capability true: tool present, including after a locked-tool refresh.
- A tool request reaches the client event stream with the same request id and
  tool call id.
- A response resolves the waiting tool exactly once.
- An unsupported client resolves the request as cancelled instead of hanging.

TUI:

- Branch selection follows the tree.
- Back pops one frame and truncates later answers.
- Changing an answer changes the subsequent branch.
- Option scrolling keeps the selection visible with 500 options.
- Review scrolls with unlimited answers.
- Narrow widths do not wrap chrome rows.
- Unicode labels count display cells.
- ANSI control sequences in model text are stripped.
- A second request queues and opens after the first completes.

Verification commands:

```sh
cargo test -p jcode-tool-types
cargo test -p jcode-protocol
cargo test -p jcode-app-core ask_user_question
cargo test -p jcode-app-core user_question
cargo test -p jcode-tui --lib question_overlay
cargo test -p jcode-tui --lib question
e2e/run.py --binary <fork-bin> --suite all
```

## Risks and upstream conflict hints

- `StdinInputRequest` rename touches `bash.rs`, `bash_tests.rs`, and
  `client_lifecycle.rs`. If upstream adds a new stdin consumer, the compiler
  finds it.
- The two capability apply sites in `client_lifecycle.rs` are the same ones
  `user_response` uses. An upstream refactor of capability application will
  conflict there. The doc `docs/user-response.md` is the canary.
- `Subscribe` gains one flag next to `supports_user_response`. Protocol tests
  with exhaustive struct literals will fail first.
- The TUI overlay hooks are the same shape as the session-picker hooks. If
  upstream adds a third key path or a new `draw_inner` early return, the TUI
  tests and manual frame checks catch it.
- `App` constructor sites are easy to miss. Search for
  `session_picker_overlay: None` and add the new fields beside each.
- Multi-client capability remains last-subscribe-wins, inherited from the
  existing `user_response` design. Do not "fix" it in this change.

## Ledger row (add to `FORK.md` when implementation starts)

| ID | Title | Status | Base | Upstream files touched | Plan | Upstreamable |
| -- | ----- | ------ | ---- | ---------------------- | ---- | ------------ |
| C5 | `ask_user_question` branching dialog | planned | upstream `master` | `jcode-tool-types`: `lib.rs`, `Cargo.toml`. `jcode-tool-core`: `lib.rs` (interactive input kind). `jcode-protocol`: `wire.rs`, `wire/events.rs`, `Cargo.toml`. `jcode-app-core`: `tool/mod.rs`, `tool/bash.rs`, `agent/turn_execution.rs`, `server/client_session.rs`, `server/client_lifecycle.rs`, `server/client_actions.rs`. `jcode-tui`: `tui/mod.rs`, `app.rs`, `app/tui_lifecycle.rs`, `app/tui_state.rs`, `app/input.rs`, `app/remote/key_handling.rs`, `app/remote/server_events.rs`, `tui/backend.rs`, `tui/ui.rs`. New: `tool/ask_user_question.rs`, `tui/question_overlay.rs`, `docs/ASK_USER_QUESTION.md` | this plan | yes (one new tool, one new TUI module, thin hooks) |

C5 best-way check: no upstream question tool exists, no config or skill can
render a native structured dialog, and the blocking interactive-input primitive
already exists. Chosen layer: minimal additive fork code.

C5 verify: the test commands above, plus a manual branching dialog run after
`selfdev build-reload`.
