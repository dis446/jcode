# ACP adapter

`jcode acp` exposes a jcode session to an Agent Client Protocol (ACP) host over
stdio JSON-RPC. The adapter is a client of the daemon: it creates or attaches a
session, forwards prompts, and turns daemon events into ACP updates.

Related code: `src/cli/acp.rs`, `src/cli/acp/session_configuration.rs`.

## Session setup

- `session/new` creates a daemon session in the client's working directory.
- `session/load` (or `--resume`) attaches an existing session.
- `-m <model>` and `-p <provider>` select the starting model. The adapter sends
  `SetModel` after `session/new`, so the flags also work against a shared daemon.
  An explicit provider prefixes the model as `provider:model`; a model that
  already contains `:` is sent unchanged.
- `[acp] tool_profile` sets the tool profile when the adapter starts its own
  daemon. An existing daemon keeps its tool configuration.
- `[acp] profile` is `standard` (spec-compatible only), `extended`, or `full`.
  The latter two add ignorable `_jcode/*` extension notifications.

## Model catalog

`session/new` returns two views of the same catalog:

- `models.availableModels` and `models.currentModelId`
- `configOptions[id=model]` with the current value and selectable options

The daemon gives `History` only a few milliseconds to build the catalog and
returns an empty list when it is cold. The adapter follows up with
`GetModelCatalog`, bounded at 10 seconds. On error or timeout it keeps the
`History` state so session setup still succeeds.

Catalog order puts bare native ids before `vendor/model` aggregator ids and
inserts the current model when it is missing. ACP hosts commonly cap the list
(t3-code keeps the first 256 plus the current model); OpenRouter alone can
publish 400 or more vendor-prefixed ids. The full catalog is still sent, but
configured native providers remain visible under a cap.

## Model switching

`SetModel` accepts a bare model id or `provider:model`. The switch resolves and
validates the target route before replying with `ModelChanged`; a switch that
cannot be routed fails there instead of on the first prompt.

- Native providers own their bare ids (`claude-opus-5-5`, `deepseek-v4.1-flash`).
- Aggregators require their own ids (`anthropic/claude-opus-5-5` on OpenRouter).
- A bare native id that would bind to a real aggregator is rejected with the
  expected prefixed alternative.
- Generic custom endpoints still own their opaque ids.

The same rules apply to `jcode acp -m`: an invalid provider/model combination
fails during session setup.

## Reasoning effort

When the provider and model report supported efforts, `configOptions` also
contains `id=reasoning_effort` with `category=thought_level`. Values come from
provider inference and exclude the TUI-only `swarm` sentinel. The current value
is the session effort when supported, otherwise `medium` when available, then
the first level. Selecting an effort persists for the session. Providers that
report no efforts get no selector.

## Commands and skills

The adapter handles `model`, `models`, and `effort` itself, so a project command
or skill with the same name is not advertised. Other slash commands, prompt
templates, and skills are advertised from the active session's project and
global sources. `--initial-message` prefills the session without sending a turn.

## Limits

- The adapter never upgrades a running daemon; build and reload the runtime
  separately.
- Cold catalog fetches are best-effort, not a guarantee that another client's
  cache is fresh. Use the provider refresh path for explicit discovery.
- ACP hosts may still truncate a large catalog. Native ids are ordered first as
  a mitigation, not as a replacement for host-side filtering.
