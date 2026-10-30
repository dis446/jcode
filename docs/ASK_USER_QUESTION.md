# Interactive questions

`ask_user_question` lets the model ask the user a branching questionnaire
instead of guessing. The TUI renders an inline dialog panel above the status
line with authored options, an optional typed answer on every question, and a
review screen. Answers return to the model as structured JSON.

The tool exists only in the interactive TUI. It is absent from headless runs,
the SDK, the harness bridge, Desktop, and old clients.

## Input

The model-visible input is an object with these properties:

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `questions` | array | yes | Question nodes. No maximum count. |
| `root` | string | no | Id of the first node. Defaults to the first array entry. |
| `timeout_secs` | integer | no | Optional wait bound. Omitted means wait indefinitely. |
| `intent` | string | yes | Standard shared tool UI label |
| `accept_large_output` | boolean | no | Standard shared context-budget opt-in |

Each question node has:

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `id` | string | yes | Unique id referenced by `root` and `next` |
| `question` | string | yes | Question text |
| `header` | string | no | Short label for breadcrumbs and review |
| `options` | array | no | Authored choices. No maximum count. |
| `multi_select` | boolean | no | Allow more than one choice. Defaults to false. |
| `allow_custom` | boolean | no | Offer a typed answer row. Defaults to true. |
| `next` | string | no | Node to ask when no option supplies a branch |

Each option has `value` (required, stable id), `label` (required), optional
`description`, and optional `next`.

There is no `maxItems` or `maxLength` anywhere. The schema, validation, and
renderer impose no count limit on questions or options. The only bound is the
existing protocol frame limit, and the dialog scrolls instead of truncating.

## Branch semantics

The next node is chosen after each answer:

1. Multi-select questions use the node-level `next`.
2. A single-select answer uses the chosen option's `next`.
3. Otherwise the node-level `next` is used.
4. Otherwise the questionnaire ends and the review screen opens.

Several options may point at the same node. That is how different branches
converge on a shared follow-up question. Cycles are rejected, and every node
must be reachable from `root`, so the questionnaire always terminates.

A custom answer has no option branch, so it follows the node-level `next` or
ends the questionnaire.

## Validation

The tool validates the graph before sending anything to the client:

- at least one node, a resolvable root, unique non-empty ids;
- unique non-empty option values within each question;
- every `next` references an existing node;
- no cycle, reported with its path;
- every node reachable, reported by id;
- multi-select questions do not put `next` on options;
- a question with no options must allow a custom answer.

Validation is iterative, so a very deep chain cannot overflow the stack.

## Result

Answered:

```json
{
  "status": "answered",
  "answers": [
    {"question_id": "task", "selected": ["bug"]},
    {"question_id": "testing", "selected": ["unit", "e2e"], "custom": null}
  ],
  "path": ["task", "testing"]
}
```

Cancelled or timed out:

```json
{"status": "cancelled", "reason": "user cancelled the questionnaire"}
```

Cancellation is a normal tool result, not an error, so the model can respond
gracefully. Missing client support is an error that tells the model to ask in
prose.

## Layout

The dialog follows the `@juicesharp/rpiv-ask-user-question` layout used by Pi:

- It is composited over the transcript, not painted as a full-frame takeover.
  The normal status line, notification, input, and anything above the panel stay
  visible.
- The panel is as tall as its current content needs, capped at 70% of terminal
  rows (at least 12), and grows upward from the status line. Very short
  terminals fall back to the full frame.
- A horizontal rule opens the panel, a tab strip names the answered questions
  and the current one (`■` answered, `□` active), and a second rule separates
  the content from the key hints.
- Options are numbered and carry their description on a dim second row, so the
  selected choice is readable without a separate detail pane.
- The review phase uses the same frame with a Review heading and Submit tab.

## TUI keys

| Key | Asking | Review |
| --- | --- | --- |
| Up / Down, j / k | move | scroll |
| PageUp / PageDown, Home / End | page / jump | page / jump |
| Enter | select or confirm | submit on Submit row |
| Space | toggle multi-select | |
| Left / Backspace | previous question | previous question |
| Esc, Ctrl+C | cancel | back to the last question |

Selecting the `Type something.` row opens an inline editor. Enter submits the
typed answer, Esc returns to the option list without losing it, and Shift+Enter
(or Ctrl+J) inserts a newline.

## Capability gating

The tool lives outside the base registry. The TUI sets
`supports_user_question: true` in `Subscribe`. The daemon registers the tool for
that session and forwards requests as `user_question_request` events. Responses
return as `user_question_response`.

Like `user_response`, capability is applied through the existing
apply-or-defer path so a busy session updates its tool catalog without blocking
attachment, and locked tool lists are invalidated when the capability changes.
Multi-client sessions keep the existing last-subscribe-wins limitation.

## Runtime delivery

The registration, validation, and waiting happen in the daemon. The TUI only
renders the dialog and sends the result. Rebuilding the TUI alone does not
activate the tool: the daemon must run a binary containing this change, and the
client must reconnect and subscribe to it.
