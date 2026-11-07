# Feature request: ACP sessions advertise the full model catalog

Status: implemented (2026-11-07), FORK.md C18
Owner: jcode fork
Related: `src/cli/acp.rs`, `src/cli/acp/session_configuration.rs`; t3-code
`acpRegistry` driver (`jcode acp` registered as a custom local ACP command)

## Problem

t3-code drives jcode through `jcode acp`. t3 builds its model picker, and
validates `delegate_task` targets, from the `model` select in the ACP
`configOptions` that jcode returns from `session/new`. In practice t3 sees
only **one** model, the session's current one (`claude-opus-5-5`), so:

- a t3 child on jcode can run Opus, but
- `delegate_task` with `model: "deepseek-v4.1-flash"` is rejected by t3 before
  jcode is even asked: `model_unavailable — Model deepseek-v4.1-flash is not
  advertised by provider acpRegistry_jcode`.

That blocks the intended setup: a t3 lead dispatching jcode workers on
DeepSeek (opencode-go) and a reviewer on Opus (Claude OAuth), all through one
provider.

jcode itself can do it. Over raw ACP, `session/set_model` to
`deepseek-v4.1-flash` succeeds and the turn runs on opencode-go (its usage
"last used" moved from 6 days ago to "just now"). Only the advertised list is
wrong.

## Evidence (jcode `0.99.476-dev`, 2026-11-07)

Same handshake twice (`initialize`, wait 3 s, `session/new`), different
results:

| Run | `configOptions[model].options` | `models.availableModels` |
|---|---|---|
| 1 | 491 entries (incl. `deepseek-v4.1-flash`) | 491 |
| 2 | **1** entry (`claude-opus-5-5`) | — |

t3's cached probe (`~/.t3/caches/acpRegistry_jcode.json`) holds the 1-entry
variant, so every t3 session since sees one model.

Confirmed cause: the daemon bounds the History catalog build at 5 ms
(`HISTORY_MODEL_CATALOG_BUDGET` in `jcode-app-core/src/server/client_state.rs`)
and ships an empty list when cold, expecting a follow-up `GetModelCatalog`,
which the TUI sends and ACP did not. Original inference: `create_new_session_connected` builds
`SessionUiState` from the `History` event's `available_models`. When the
daemon's model catalog is not warm yet, that list is empty, and
`session_config_options` falls back to `[current]`
(`session_configuration.rs`, the `if !models.iter().any(...)` insert). Nothing
refreshes it afterwards: `request_model_catalog` (`GetModelCatalog`) is only
called by the `/models` slash command. `ModelChanged` and
`wait_for_model_changed` keep the stale list.

## Desired behavior

1. `session/new`, `session/load` and `session/resume` return the **full**
   catalog in both `configOptions[model].options` and
   `models.availableModels`, every time, warm daemon or cold.
2. If the catalog genuinely is not available within a bounded wait, respond
   anyway and emit a `config_option_update` (and the `models` equivalent) once
   it arrives. That fallback is not enough for t3, which probes once and
   caches; the blocking path in (1) is the one that matters.
3. `session/set_model` with any listed id keeps working as today.

## Implemented change (smallest)

In `create_new_session_connected` and `attach_existing_session_connected`,
after building `ui_state`, always call `request_model_catalog` (cheap when
warm) with a 10 s timeout (cold builds measured up to ~7 s) and take its
`available_models`. On error or timeout keep the History state. That reuses the
`/models` path that already works. No protocol change and no new request.

## Acceptance

- Unit test in `src/cli/acp/tests.rs`: a `History` with an empty
  `available_models`, followed by a catalog `History` with N models, makes
  `session/new` return N model options.
- Manual: run the 2-step handshake 10 times against a freshly restarted
  daemon. Every `session/new` returns the full list.
- t3: after a provider refresh, `orchestrator_capabilities` lists
  `deepseek-v4.1-flash` under `acpRegistry_jcode`, and a `delegate_task` with
  that model runs on opencode-go.

## Out of scope / follow-ups (separate requests if wanted)

- **Ambiguous plain ids.** `deepseek-v4.1-flash` exists under opencode-go and
  the direct `deepseek` provider, while openrouter uses `deepseek/...`. The
  plain id routed to opencode-go here, but the rule is not visible to the ACP
  client. Consider provider-qualified ids (`opencode-go/deepseek-v4.1-flash`),
  or a `provider` field per option.
- **`jcode acp -p/-m` are ignored.** Fixed by C19: new sessions now apply the
  flags through `SetModel`, so this works against a shared daemon too.
- **t3's MCP tools don't reach jcode ACP sessions.** A t3-spawned jcode child
  reported no t3 tools (`delegate_task` etc.). `initialize` advertises
  `mcpCapabilities: {http: false, sse: false}`, so only stdio MCP servers in
  `session/new.mcpServers` could work. Check whether jcode honours those. This
  matters only if a jcode *lead* in t3 should dispatch through t3 rather than
  `swarm`.
