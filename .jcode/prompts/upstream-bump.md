---
description: Assess an upstream bump for changes, gains, risks, and a rebase recommendation
argument-hint: "[target-ref] [new-base-version]"
---
Upstream has moved. Assess the pending upstream bump for this fork.

**Read-only analysis.** Do not rebase, commit, push, change the pin, or run the
installer in this turn unless I explicitly approve. Recommend first.

Target ref: `${1:-upstream/master}`. Optional new base version for the ledger:
`${2:-unset}`.

Fork context:

- `origin` = `dis446/jcode`, `upstream` = `1jehuang/jcode`.
- This checkout is the fork. `master` is a small rebased series on upstream.
- Custom changes, their base, upstream files touched, and conflict hints:
  `FORK.md`.
- Pinned upstream base: `~/dotfiles/jcode/UPSTREAM_VERSION`.
- Upgrade procedure: `FORK.md` section "Upgrade runbook".
- `jcode update` is banned. Never use it.

## 1. Establish state

- Read the pin and the ledger's current series base.
- `git fetch upstream --tags --prune` if the target is not present locally.
- Report `git status`, current branch/HEAD, the merge-base with the target,
  commits ahead and behind, upstream tags/releases in range, and the target diffstat.
- Confirm the target exists. If it does not, stop and say so.

## 2. What upstream changed

- `git log --oneline --no-merges <base>..<target>`, grouped by area (TUI,
  protocol, providers, tools, config, storage, build/release).
- Diffstat, largest changed files, new and deleted crates/modules.
- For **every** ledger row, list upstream commits touching the files that row
  names, and quote the row's conflict hints. Flag rows upstream now ships.
- Call out config or schema migrations, wire/protocol changes, renames and
  deletions, dependency and MSRV changes, provider/model discovery changes,
  security fixes, and default behavior changes.
- Use release notes when tags are available; otherwise say the analysis is from
  the commit range only.

## 3. What the fork gains by bumping

Be concrete, not promotional:

- new features and fixes relevant to how this fork is used;
- performance, correctness, and security fixes;
- new upstream primitives that reduce fork code or let a ledger row be dropped;
- maintenance reduction and conflict risk removed by catching up now;
- anything that gets harder if the bump is deferred further.

## 4. Risks

- Per ledger row: conflict likelihood (none / low / medium / high), the exact
  files and call sites, and why.
- Semantic risks beyond textual conflicts: changed signatures, moved call sites,
  protocol compatibility, config defaults, behavior contracts, tests, and
  install/rollback.
- Upstream changes that obsolete or duplicate fork behavior.
- Test gaps: what the gates below do not cover, especially TUI, protocol,
  provider discovery, and project tools/commands/prompt templates.
- Deferral risk: how much the next rebase grows if this one is skipped.

## 5. Recommendation

Give a clear verdict: **GO**, **NO-GO**, or **DEFER**, with the reason.

If GO or DEFER-with-plan, include:

- ordered rebase steps and the order to resolve conflicts, saying what to keep,
  drop, or mark upstreamed;
- the exact verification gates: `cargo check --workspace --all-targets`,
  `e2e/run.py --binary <fork bin> --suite baseline` on stock then `--suite all`
  on the fork, `cargo test --test e2e`, and the targeted commands from `FORK.md`
  for every touched row;
- ledger updates: `Base` column, dropped/upstreamed rows, refreshed conflict
  hints, current series base;
- pin and install updates (`~/dotfiles/jcode/UPSTREAM_VERSION`,
  `~/dotfiles/scripts/jcode-install.sh`) and the rollback command;
- a copy-paste command block, clearly marked as not yet executed.

## 6. Report format

Sections: **State**, **Upstream changes**, **Gains**, **Risks**, **Verdict**,
**Rebase plan**, **Verification**.

Use hashes and file paths. Say `unknown` rather than guessing. End with the one
decision you recommend and what you need from me to proceed.
