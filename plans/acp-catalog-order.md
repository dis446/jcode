# Bug: ACP model catalog order hides native models behind OpenRouter ids

Status: implemented (2026-11-07), FORK.md C20
Owner: jcode fork
Related: `plans/acp-full-model-catalog.md` (C18),
`plans/acp-model-switch-provider-routing.md` (C19),
`src/cli/acp/session_configuration.rs` (`session_models`,
`session_config_options`)

## Problem

Since C18, `session/new` lists the full catalog: 491 model ids in
`configOptions[model].options`. t3-code keeps only the **first 256**, plus the
session's current model. The catalog order puts most native/subscription ids
after the cut:

| Id | Index in the 491 list | Seen by t3 |
|---|---|---|
| `claude-opus-5-5` | 0 | yes |
| `deepseek-v4.1-flash` (opencode-go) | 457 | only while it is the current model |
| `claude-sonnet-5-5` | 490 | no |

411 of the 491 ids are OpenRouter-style `vendor/model` ids.

Effect, with jcode `0.99.886-dev (2e2217d09)`: t3 `delegate_task` to jcode on
`claude-opus-5-5` works (C19 fixed the routing). The next `delegate_task` on
`deepseek-v4.1-flash` is rejected by t3 (`model_unavailable`), because t3's
cached catalog was refreshed from a session whose current model was Opus. It
worked earlier only because `default_model = "deepseek-v4.1-flash"` made it the
current model when t3 first probed. Whether a model is usable from t3
therefore depends on which session t3 last saw.

## Desired behavior

- Every model a configured native/subscription provider serves (claude,
  opencode-go, deepseek, ...) is inside the first 256 entries, whatever the
  current model is.
- OpenRouter `vendor/model` ids still appear, after the native ones.

## Proposed change (smallest)

When building the ACP model list (both `models.availableModels` and the
`configOptions` select), stable-sort bare ids (no `/`) ahead of `vendor/model`
ids. Today there are 80 bare ids, well under 256. One sort, in the shared spot
both builders read from. No protocol change.

Alternatively: list native providers first, then aggregators. Same effect, if
the catalog already carries the provider per id.

## Implemented change

`src/cli/acp/session_configuration.rs`: new `ordered_available_models` is the
single list builder behind both `session_models` (the legacy
`models.availableModels` field) and the `configOptions` model select. It
inserts the current model when missing, then applies a stable sort on
`model.contains('/')`. Bare native/subscription ids keep their catalog order
ahead of every `vendor/model` id. No protocol or catalog change.

Test: `src/cli/acp/tests.rs`
`native_model_ids_precede_aggregator_ids_in_both_catalogs` builds 300 vendor
ids followed by `deepseek-v4.1-flash` and `claude-sonnet-5-5`, then asserts
both bare ids are before every vendor id and below index 256 in the
`configOptions` select and in `models.availableModels`.

Manual raw `session/new` on the rebuilt binary: 491 models, first vendor id at
index 80, `deepseek-v4.1-flash` at 46, `claude-sonnet-5-5` at 79,
`claude-opus-5-5` at 0, and the legacy catalog order matches the
`configOptions` order. The fork e2e suite passes.

## Acceptance

- Test in `src/cli/acp/tests.rs`: given a catalog with `vendor/x` ids before
  `deepseek-v4.1-flash` and `claude-sonnet-5-5`, the built options list both
  bare ids before every `vendor/` id.
- Manual: raw `session/new` shows `deepseek-v4.1-flash` and `claude-sonnet-5-5`
  at an index below 256.
- t3: after a provider refresh, `delegate_task` to jcode on `claude-opus-5-5`,
  then `deepseek-v4.1-flash`, then `claude-sonnet-5-5`, in that order, all
  start.
