# Feature request: per-spawn reasoning effort for swarm workers

Status: implemented (2026-11-07), FORK.md C21
Owner: jcode fork
Related: `crates/jcode-app-core/src/server/spawn_policy.rs` (`resolve_swarm_spawn_effort`,
`DEFAULT_SWARM_WORKER_EFFORT = "low"`), `crates/jcode-app-core/src/server/comm_session.rs` (~line 357,
upstream #1165), `crates/jcode-app-core/src/tool/swarm_model_schema.rs` (`apply_swarm_model_policy`),
`crates/jcode-app-core/src/tool/communicate_schema_tests.rs:66` (asserts no `effort` property),
`plans/acp-forward-wake-turns.md`

## Problem

A jcode lead (in t3-code over ACP, or in the TUI) picks each swarm worker's model
with `swarm spawn model=...` (when `agents.swarm_model_selection` is on). It
can't pick the worker's reasoning effort. Upstream removed that on purpose
(#1165): `requested_effort` is only honoured from SDK clients. Otherwise a worker
gets the global `agents.swarm_effort` pin or, failing that, `low`.
Verified 2026-11-07 in the daemon log: `requested_effort=None configured_swarm_effort=None`
for two Sonnet workers, so both ran at `low`.

The globalSim feature loop sets effort per slice: `low` for mechanical work
(UI wiring, DTO fields, spec-only QA), `high` for migrations, concurrency and
money/security. Under a t3 Claude lead that is the `reasoning_effort` option on
each `delegate_task`. A jcode lead has no equivalent, so it either overpays on
mechanical slices or underthinks risky ones.

## Desired behavior

Opt-in, so upstream's default (#1165) stays intact: with
`agents.swarm_effort_selection = true`, `swarm spawn` accepts an optional `effort`
(`none|low|medium|high|xhigh|max`, the same set ACP's `reasoning_effort` config
option exposes). The spawned session starts with that effort for its provider.
If the field is omitted, the worker gets today's default.

## Sketch

- In `apply_swarm_model_policy` (or a sibling), add the `effort` enum property
  only when `swarm_effort_selection` is on. Keep the schema test's "no effort"
  assertion for the default config and add one for the opt-in.
- Thread the tool's value into the existing `requested_effort` path, which
  `resolve_swarm_spawn_effort` already ranks first.
- Show it in `swarm status` next to the model, so the lead can verify it.

## Acceptance

- Schema tests: no `effort` by default; with the opt-in, `effort` appears with the enum.
- A spawn with `effort: "high"` logs `requested_effort=Some("high")`, and the
  worker runs at `high`.
- Manual check: a t3 jcode lead spawns two workers, one with `low` and one with
  `high`. `jcode usage` and the log show the requested levels.

## Implementation

`agents.swarm_effort_selection` (default false) gates both the tool schema and
the server. With it on, `swarm spawn` accepts `effort` and every spawn-capable
path forwards it to the existing `requested_effort` input of
`resolve_swarm_spawn_effort`; reused workers keep their own effort. Requests
with the opt-in off, an unknown level, or uppercase/blank values fail before
dispatch and again in `spawn_swarm_agent`. `AgentStatusSnapshot.provider_effort`
now feeds `swarm status` as `Provider: name / model (effort)`.

Verified with `target/selfdev/jcode` against an isolated daemon:

```
Swarm spawn model resolution: requested_effort=Some("high") ... -> spawn_model=Some("claude-opus-5-5")
Swarm spawn model resolution: requested_effort=Some("low")  ... -> spawn_model=Some("claude-opus-5-5")
```

The two worker session files recorded `"reasoning_effort":"high"` and
`"reasoning_effort":"low"`, each with a `set_reasoning_effort` env snapshot.
