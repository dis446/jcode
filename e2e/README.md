# jcode TUI e2e regression suite

Deterministic, fast, binary-only. It drives a real `jcode` binary in a private tmux
server and asserts on the rendered screen. It needs no source tree and no cargo,
so the **same suite runs against stock upstream and against our fork build**.

```bash
e2e/run.py --binary "$(command -v jcode)" --suite baseline       # stock: must be green
e2e/run.py --binary ~/Code/Personal/jcode/target/selfdev/jcode --suite all
e2e/run.py --binary ... --suite baseline --repeat 8 -j 12         # flake hunt
```

| Suite | Runs on | Meaning |
| ----- | ------- | ------- |
| `baseline` | stock **and** fork | Behavior the fork must never change (composer, slash palette, paste, resize, undo). |
| `at_file` | fork only (C1) | The `@file` picker. Fails on stock by design: proves the tests detect the feature. |
| `project_commands` | fork only (C5) | `.jcode/commands.json` dispatch, completion, help, arguments, and confirm. Fails on stock by design. |

## Why it is deterministic

- Each test gets its own `HOME`, `JCODE_HOME`, `JCODE_RUNTIME_DIR`, `XDG_RUNTIME_DIR`,
  tmux server socket and work tree. Scrubbed env (`env -i`), telemetry and updates off.
  Tests therefore run in parallel safely and never touch `~/.jcode` or your daemon.
- Each sandbox seeds `setup_hints.json` with an established-install launch count, so
  first-run onboarding never opens and swallows the keystrokes a test types.
- No network, no provider: only the composer is exercised, nothing is sent.
- No fixed sleeps. Every step polls the screen for a predicate with a hard timeout and
  prints the last screen on failure.
- Teardown kills the tmux server and any jcode server that inherited the sandbox
  runtime dir, then removes the sandbox.
- Verified: 80/80 on `--repeat 8 -j 12` against stock 0.99.1 (6.7 s), no leaked processes.

## Rules

1. Every custom change (ledger row in ../FORK.md) ships with tests in a suite here.
2. A fork change that breaks `baseline` is a regression, full stop.
3. On every upstream upgrade: run `baseline` on the **new stock** binary first (catches
   upstream UI changes that invalidate our assertions), then on the rebased fork.
4. When a test breaks because upstream changed legitimately, fix the assertion in the same
   commit as the upgrade and say so in the ledger.

## Not covered (and why)

Model/tool turns, streaming, swarm, reload handoff. Those need a provider. Upstream's
`tests/e2e` (mock provider, run with `cargo test --test e2e`) covers them from source and
should be run after each upgrade in the fork checkout. The slow Rust `jcode-tui` lib suite
is not a gate: see `../FORK.md` "Test gates".
