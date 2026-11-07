# Bug: ACP `session/set_model` to a Claude model routes to OpenRouter

Status: implemented (2026-11-07), FORK.md C19
Owner: jcode fork
Related: `plans/acp-full-model-catalog.md` (C18; this is its "ambiguous plain ids"
follow-up, now hit in practice), `src/cli/acp.rs` (`session/set_model` →
`Request::SetModel`)

## Problem

After C18, t3-code sees the full jcode catalog and can start jcode children on
any model. DeepSeek works. Opus no longer does:

- t3 `delegate_task` → jcode, model `claude-opus-5-5`: the turn fails with
  `OpenAI-compatible chat request failed — endpoint:
  https://openrouter.ai/api/v1/chat/completions, model: claude-opus-5-5, auth:
  OPENROUTER_API_KEY, 400 "claude-opus-5-5 is not a valid model ID"`.
- Same result twice, once alongside a DeepSeek child and once alone.
- Earlier the same day (before any DeepSeek session) the same request ran on
  native Claude OAuth and succeeded.

So switching from a non-Claude model to `claude-opus-5-5` keeps an
OpenAI-compatible transport (OpenRouter) instead of moving to the `claude`
provider. OpenRouter would also need its own id (`anthropic/...`), so the
request can never succeed there.

## Repro (raw ACP, jcode `0.99.885-dev (d8502459e)`)

1. `initialize`, then `session/new`. `models.currentModelId` is
   **`deepseek-v4.1-flash`**: the new session inherited the last model used
   on the daemon, not the configured default.
2. `session/set_model` `claude-opus-5-5`: succeeds, and `configOptions`
   shows `currentValue: claude-opus-5-5`.
3. `session/prompt` "Reply with exactly: OK": error `-32000`, the OpenRouter
   400 above.

`jcode usage` afterwards: OpenRouter "last used: just now"; Anthropic not used.

## Two defects

1. **Provider resolution on model switch.** A plain id that a native provider
   owns (`claude-opus-5-5` → `claude`) must switch to that provider. The id
   appears once in the catalog, so it isn't ambiguous there; the resolver
   falls back to the current OpenAI-compatible route instead. If a plain id
   really is offered by several providers, prefer the native/subscription one
   (claude, opencode-go) over pay-per-token aggregators (openrouter), or reject
   the switch with an error. Never accept the switch and fail on the first turn.
2. **New ACP sessions inherit the last-used model.** A `session/new` started
   on DeepSeek because an earlier session had switched to it. For an ACP host
   that dispatches many children, the starting model has to be predictable
   (the configured default, or `jcode acp -m/-p`, which is still ignored; see
   C18 follow-ups). Hosts that skip `set_model` when the requested model
   equals the advertised current one would otherwise run on whatever the last
   child chose.

   Re-check 2026-11-07: the observed DeepSeek start came from
   `default_provider = "opencode-go"` / `default_model =
   "deepseek-v4.1-flash"` in the operator config, not from inheritance. A new
   session rebuilds the orchestrator (`fork_for_new_session`) instead of
   copying the template's current selection. The remaining, real gap was that
   `jcode acp -m/-p` were only forwarded to a daemon this process spawned, so
   a shared daemon ignored them. That is fixed below.

## Implemented change

Root cause of defect 1: the active slot was OpenRouter executing the OpenCode
Go profile with provider routing features disabled. `set_model`'s "a custom
OpenAI-compatible endpoint owns opaque ids" shortcut sent every bare id to
`set_model_on_provider(OpenRouter)`. The profile does not serve
`claude-opus-5-5`, so the OpenRouter branch rebound the slot to the real
aggregator with a bare native id. `set_model` returned success and the first
turn failed. Logs confirmed `server_model_changed ... provider=OpenRouter`.

Smallest fix, in `crates/jcode-base/src/provider/mod.rs`:

- `active_catalog_profile_lacks_model` distinguishes a *known catalog profile*
  (opencode-go, deepseek, ...) from a generic custom endpoint. A catalog
  profile publishes its ids, so a bare id it does not serve belongs to another
  provider and must leave the shortcut. A generic endpoint still owns opaque
  ids, and an empty catalog keeps the old behaviour.
- When the OpenRouter branch would bind the real aggregator, a bare native id
  is rejected at switch time with the qualified alternative
  (`anthropic/claude-opus-5-5`). The switch now fails before the turn.

Defect 2 fix, in `src/cli/acp.rs`: after a new session is created, ACP applies
`-m <model>` (with `-p <provider>` as a routing prefix, or bare under `auto`)
via the normal `SetModel` request. This works against a shared daemon too.
Session start remains the configured default when no flags are passed. There
was no last-used inheritance to remove.

Tests:

- `crates/jcode-base/src/provider/tests/native_model_switch.rs`: switch from
  opencode-go to Claude, switch back to DeepSeek, missing Claude login fails at
  switch time, real OpenRouter rejects a bare native id, generic custom
  endpoint keeps opaque ids.
- `src/cli/acp/tests.rs`: model-spec construction for explicit/auto/prefixed
  values, and a new session that sends `opencode-go:<model>` and applies the
  resolved model.

Manual raw ACP repro against an isolated daemon on the rebuilt binary:
`session/new` started on `deepseek-v4.1-flash` and answered `OK`. Then
`session/set_model claude-opus-5-5` plus `session/prompt` reached the
**Anthropic** API and surfaced its account rate-limit error (or waited through
its retries). It no longer reached OpenRouter. The Claude answer itself was
blocked by the operator account's 429, not by routing.

## Desired behavior

- `session/set_model claude-opus-5-5` from any starting model runs the next
  turn on the `claude` provider (Claude OAuth).
- `session/set_model deepseek-v4.1-flash` keeps routing to opencode-go, as it
  does today. Keep this case covered.
- `session/new` starts on a stable default that doesn't depend on what other
  sessions did.
- A switch that can't be routed fails in `set_model`, not at the first prompt.

## Acceptance

- Test in `src/cli/acp/tests.rs`: session on an OpenAI-compatible model →
  `set_model claude-opus-5-5` → the provider recorded for the session is
  `claude`.
- Manual: the 3-step repro above answers `OK`, and Anthropic usage moves while
  OpenRouter's doesn't.
- t3: `delegate_task` to jcode on `claude-opus-5-5` and on
  `deepseek-v4.1-flash`, started in either order, both succeed on their own
  subscriptions.
