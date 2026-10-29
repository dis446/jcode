<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/jcode-mark-inverse.svg">
  <img src="assets/brand/jcode-mark.svg" alt="jcode halftone donut logo" width="140">
</picture>

# jcode (personal fork)

My personal fork of [jcode](https://github.com/1jehuang/jcode)
([@dis446](https://github.com/dis446/jcode)), tracking upstream
`master` with a small set of local changes.

</div>

> This is **not** the official project. For installation, docs, releases,
> benchmarks, and everything else, use
> [github.com/1jehuang/jcode](https://github.com/1jehuang/jcode) and
> [jcode.sh](https://jcode.sh). This README only covers how the fork differs.

## Differences from upstream

### `@file` completion in the composer

Typing `@` in the TUI input opens an inline fuzzy file
picker, matching pi's behavior:

- Type `@` followed by a partial name/path, e.g. `@src/ma` or `@README`.
- Candidates come from a gitignore-aware walk of the working directory (`.git`
  itself is skipped; hidden files are included).
- Ranking: exact name > name prefix > name substring > path substring, with
  directories boosted, shallower paths first, 20 results max.
- `Up`/`Down` move the selection, `Enter` accepts, `Esc` dismisses. Accepting a
  directory keeps it open for drilling down.
- `@"..."` quoting is supported for paths with spaces, and `Tab` can force
  completion of a plain (non-`@`) path token.
- Implemented in `crates/jcode-at-file` (pure completion logic) plus the TUI
  overlay in `crates/jcode-tui/src/tui/ui_input/at_file.rs`.

### `/new [name]`

Starts a fresh session in place, like pi's `/new`, and optionally names it:

- `/new` replaces the current conversation with a new empty session in the same
  working directory.
- `/new refactor auth` starts the session with the display name
  `refactor auth`, so it is easy to find in `/resume`.
- The previous session is closed and saved as usual.
- `/new` is refused while a turn is running. Press Esc or run `/cancel` first.

Everything else in this repository is upstream code.
