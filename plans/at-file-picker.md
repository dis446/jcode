# Plan: native `@file` picker in jcode (pi parity)

Status: draft, not started. Target: local jcode v0.99.1 (same version as the installed binary).
Owner: user. Scope: your own build only. Optionally upstream later.

## Goal

Typing `@` in the jcode composer opens a fuzzy file popup, exactly like pi. Same
trigger rules, same ranking, same keys, same inserted text. Nothing else changes.

## What pi does (the spec)

Source: `@earendil-works/pi-tui@1.4.0` (`dist/autocomplete.js`, `dist/components/editor.js`,
`dist/fuzzy.js`) and `pi-coding-agent@1.4.0` (`modes/interactive/interactive-mode.js`,
`core/tools/path-utils.js`).

### 1. Completion only, no expansion on submit

`@path` stays literal text in the message. pi does not read the file on submit.
The model reads it with its `read` tool, which strips a leading `@` (`stripAtPrefix`).
So jcode needs the **editor popup only**. No change to message building or the provider path.
(The `@file` CLI-argument path in `cli/file-processor.js` is a different feature, out of scope.)

### 2. Trigger

- Auto-trigger when typing `@` at a token start. Token start means start of line, after
  whitespace, or after an opening wrapper `( [ { < \``, or after CJK punctuation.
  Regex: `autocompleteBoundaryRegex` + `[([{<\`]*` + `@` + non-separator chars, anchored at the cursor.
- Keeps updating on every letter, digit, `.`, `-`, `_` typed while open.
- `@"` starts a quoted form. The token then runs to the closing quote and may contain spaces.
- `#` is also a default trigger char in pi (provider hook). Skip it unless you want it.
- Tab forces path completion on any token (not only `@`), see section 6.
- Not triggered while pasting (paste cancels autocomplete).
- Debounce 20 ms for the `@` case, 0 ms for Tab and slash commands.

### 3. Candidate source

- Spawns `fd` (`--base-directory <cwd> --max-results 100 --type f --type d --follow --hidden`,
  excludes `.git`, `--full-path` when the query has `/`).
- Two passes merged and de-duplicated: depth-1 entries first, then recursive.
- The query is turned into a regex: no `/` means the escaped query. With `/`, segments are
  escaped and joined with `[\\/]` (trailing `/` kept).
- Scoped queries: `@src/foo` splits at the last `/`. The base dir is resolved (`~/`, absolute, or
  relative to cwd) and must exist, else no scoping.
- Respects `.gitignore` (fd default). Hidden files included.

### 4. Ranking (`scoreEntry`, not the fuzzy module)

On the file name (case-insensitive): exact 100, starts-with 80, contains 50. Else full-path
contains 30. Else dropped. Directories get +10 when score > 0. Empty query scores 1.
Sort: score desc, then path depth asc, then path length asc, then alphabetical. Keep top 20.

Note: `fuzzy.js` (subsequence scorer) is used for slash commands, not for `@` files. The `@` list is
substring-ranked on top of fd's regex filter.

### 5. List UI

- Max 5 rows visible by default (clamped 3 to 20). Row = label (name, `/` suffix for dirs)
  plus dim description (display path).
- Best-match row preselected. Up/Down move. Mouse click supported.
- Empty result closes the popup.

### 6. Keys and completion

| Key | Behavior |
| --- | --- |
| Up / Down | move selection |
| Tab | accept selected |
| Enter | accept selected (for `@`, do not submit) |
| Esc | close |
| Tab with no popup | force file completion on the current token. One match: apply immediately |

Apply rules:
- Value is `@path`, quoted as `@"path with spaces"` when it has a separator or the prefix was quoted.
- Directory: no trailing space, so you can keep drilling (`@src/` then more letters).
- File: append one space.
- Quoted dir: cursor lands before the closing quote.
- Push an undo snapshot before applying.

### 7. Races

Each request carries a token. A result is dropped if the text or cursor changed, the request was
aborted, or a newer request started. In-flight `fd` is killed on abort. Requests are serialized.

## What jcode has today (the seams)

All paths relative to the jcode source root. Verified in the v0.99.1 checkout.

- `crates/jcode-tui/src/tui/app/state_ui_input_helpers.rs`
  - `command_suggestions()` (memoized per frame, epoch + signature) returns `Vec<(String, &'static str)>`.
  - `accept_selected_command_suggestion()`, `move_command_suggestion_selection()`,
    `handle_command_suggestion_key()` (Up/Down, Ctrl+J/K, Enter).
  - `autocomplete()` is the Tab handler (slash-token cycling).
- `crates/jcode-tui/src/tui/app/input.rs`
  - `handle_key_core()` routes keys. `handle_command_suggestion_key` runs before the control shortcuts.
  - `insert_input_text_with_undo()` is the single insertion point (typing and paste share it).
  - `handle_paste()` at line ~338 and ~3423. `remember_input_undo_state()` for undo.
  - `KeyCode::Tab` calls `app.autocomplete()`.
- `crates/jcode-tui/src/tui/ui_input.rs`
  - `draw_command_suggestions_overlay()`, `command_suggestions_overlay_rect()` (below input, flips above),
    `command_suggestion_lines()` in `ui_input/command_suggestions.rs`. Overlay reserves no layout height.
- `crates/jcode-fuzzy/src/lib.rs`: `fuzzy_match`, `fuzzy_score`, `PreparedTokenQuery`, highlight positions.
- Working dir: `app.session.working_dir` (`Option<String>`).
- Tests live under `crates/jcode-tui/src/tui/app/tests/` (e.g. `command_suggestions_cache.rs`).
- No `ignore`/`walkdir` crate in the TUI crates. `fd` is on PATH here, but do not depend on it (see Design).

Gaps to fill: a file-candidate provider, an `@`-token parser, a second overlay state (the existing one
is slash-only and `&'static str` descriptions), and the async/stale-result handling.

## Design

### Do not overload `command_suggestions()`

Its type (`(String, &'static str)`), its per-frame memo, and its guards (`pending_login`, picker
preview) are slash-specific. Add a parallel, small state instead:

```text
struct AtFileState {
    token_range: (usize, usize),   // byte range of the @token in app.input
    quoted: bool,
    query: String,
    items: Vec<AtFileItem>,        // { value, label, description, is_dir }
    selected: usize,
    request_id: u64,
}
App { at_file: Option<AtFileState>, at_file_task: Option<...> }
```

### Candidate provider: pure Rust, no `fd` dependency

Add a new crate `crates/jcode-fuzzy-files` (or a module in `jcode-tui`) using the `ignore` crate
(same walker as ripgrep: gitignore aware, hidden support, `follow_links`). Reasons: no runtime
dependency on `fd`, no process spawn per keystroke, cancellable via an `AtomicBool`.
Fallback is not needed. Trade-off: one new direct dependency, but `ignore` 0.4.25 is already in
`Cargo.lock` (transitive, verified), so no new crate is downloaded. Versus shelling out to `fd`.
Recommend `ignore`.

Port exactly: the two-pass depth-1 then recursive merge, `scoreEntry` ranking, the sort tiebreakers,
top 20, `.git` exclusion, scoped `~/` / absolute / relative bases, quoted `@"..."` handling.
Pure functions, unit-testable without a terminal.

### Async

Run the walk on a blocking task, send results over the existing TUI event channel, drop stale results
by `request_id` and by comparing the input snapshot (same rule as pi section 7). Debounce 20 ms for
the typing path. Walk cap: `max_results` 100 per pass.

### Keys

Add `handle_at_file_key()` and call it right before `handle_command_suggestion_key` in
`handle_key_core`. Active only when `at_file.is_some()`. Map: Up/Down (and Ctrl+J/K like slash),
Tab and Enter accept, Esc close. This must **not** conflict with the user's remaps in
`~/.jcode/config.toml` (`ctrl+alt+...`, `alt+...`). Plain Up/Down/Tab/Enter/Esc are not rebound there.
Enter while the popup is open accepts and does not submit. Esc closes the popup before it clears input.

### Rendering

Reuse `command_suggestions_overlay_rect()` for placement (floats, no layout shift), and the same
row styling. New `draw_at_file_overlay()` called next to `draw_command_suggestions_overlay()`.
5 visible rows, selected row marker, dim description. Highlight matched chars with
`jcode_fuzzy::fuzzy_match_positions` against the name (small nicety, not in pi, skip if it differs
visibly from pi).
Respect the repo's TUI rule: rows must not wrap, truncate with an ellipsis by display width.

### Tab behavior

Today Tab cycles slash suggestions. Add: when the cursor token is not a slash command and no
popup is open, Tab forces path completion on that token (pi parity), one match applies at once.
Keep slash behavior untouched.

### Triggers and insertion

In `insert_input_text_with_undo`, after the insert, run `maybe_open_at_file()` when the typed text is
a single char in the trigger set or a token char. Skip when the insert came from paste. Reuse the
existing `typed` flag. Pi's boundary regex becomes a small hand-written scanner over `app.input[..cursor]`.
Use char boundaries (`next_char_boundary`) like the rest of the file.

### Out of scope

`#` provider trigger, `@file` CLI arguments, expansion on submit, image attach by `@`.

## Phases

1. **Spec tests first (no TUI).** New crate or module with `at_prefix`, `parse_path_prefix`,
   `build_fd_path_query` equivalent, `score_entry`, `apply_completion`. Port pi's cases as table tests.
   Gate: `cargo test -p <crate>` green. This is the whole behavioral spec, so do it first.
2. **Provider.** `ignore`-based walker with cancel flag, two-pass merge, top 20. Tests on a temp tree:
   gitignore respected, hidden included, `.git` excluded, symlinked dir followed, scoped `src/` base,
   `~/` expansion.
3. **State + keys.** `AtFileState`, open/close/update on typing, Up/Down/Tab/Enter/Esc,
   Enter-accepts-not-submits, directory vs file suffix, undo snapshot. Tests alongside
   `app/tests/` (mirror `command_suggestions_cache.rs`).
4. **Render.** `draw_at_file_overlay()`, truncation by display width, flip above at the bottom.
   Snapshot or buffer tests like `ui_input/status_tests.rs`.
5. **Async + races.** Blocking task, request ids, stale drop, debounce. Test with a slow fake provider.
6. **Tab force + polish.** Tab on non-slash tokens, single-match auto-apply, quoted `@"..."` flow,
   paste cancels the popup.
7. **Manual parity check** against pi in the same repo (see Verification).

## Self-dev workflow (from the repo's AGENTS.md)

- Source: a v0.99.1 tree exists at `~/.jcode/scratch/jsrc` (no git repo, no `target/`). Scratch is
  not durable and has no history. **Clone the real repo into a stable place first**
  (`git clone https://github.com/1jehuang/jcode ~/Code/Personal/jcode`, checkout the `v0.99.1` tag),
  work on a branch, commit as you go. Then `diff -r` against the scratch copy once to confirm it is the
  same tree.
- AGENTS.md says to work on master and not branch unless needed. For a local fork that you will rebase
  onto upstream releases, a branch `feat/at-file-picker` is the concrete reason, so keep it.
- Build: `cargo build --profile selfdev` (28 cores, Rust 1.99, edition 2024, ~775 GB free).
  No `mold`, `sccache` or `clang` installed. Link time will be the slow part. Installing `mold`
  (dnf) is optional and would speed linking, not required.
- **Test without disturbing your daemon:** run the build against its own socket:
  `./target/selfdev/jcode run --no-update --socket /run/user/1000/jcode-atfile.sock '<prompt>'`.
  A fresh binary is inert for real sessions until `jcode self-dev --build` repoints
  `~/.jcode/builds/shared-server/jcode` and restarts the daemon. Do that last, not first.
- `cargo build` alone proves nothing about behavior (AGENTS.md). The TUI needs a real pty check, below.
- Throwaway diagnostics use `eprintln!`, not `crate::logging::info` (that goes to a file). Remove before commit.
- Use your own git identity for commits, never an agent identity (AGENTS.md).

## Verification

Per phase: unit and table tests. End to end, in order:

1. `cargo test -p jcode-tui at_file` and the new crate's tests.
2. Headless sanity: `target/selfdev/jcode run --no-update --socket ...` still works (no regression).
3. Real TUI in a pty (run in a herdr pane or `script`/`tmux` harness), repo = a scratch tree:
   - type `@rea` shows `README.md` first (starts-with beats contains).
   - `@src/` lists the dir contents, dirs first by score bonus.
   - Tab on a dir keeps the popup flow (no trailing space), on a file adds a space.
   - `@"my fi` quoted flow with spaces.
   - Enter accepts and does not send. Esc closes. Esc again clears as before.
   - `(@x`, `` `@x `` wrapper cases. `email@x.com` must NOT open the popup (no token boundary).
   - Paste a path containing `@`: no popup.
   - Slash commands unchanged, `/mod` + Tab unchanged.
4. Side by side with pi in the same directory for 10 queries. Same top result and same order is the pass bar.
5. herdr: confirm `alt+h` toggle, resume and the sidebar status still work, since they depend on the TUI.
6. Only then `jcode self-dev --build` to repoint the shared daemon, and check `jcode --version` and that
   existing sessions reconnect.

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Key conflicts (Tab, Enter, Esc, Up/Down) | Handler runs only while the popup is open. Run `/keys` (conflict report) after. |
| Per-frame cost (suggestions are read up to 8x per frame) | Own state, no recompute in render. Render reads `at_file.items` only. |
| Large repos (`~/Code/and/alpha`) | `ignore` walker with result cap and cancel flag. Never walk on the render thread. |
| Race on fast typing | request id plus input snapshot compare, as in pi. |
| `email@host`, `@mentions` in prose | token-boundary scanner, same as pi's regex. Test cases above. |
| Local build drifts from upstream | Keep the change in one new crate plus small hooks (about 5 touch points) so rebases stay cheap. |
| The installed binary gets replaced by an update | `jcode self-dev --build` output lives under `builds/versions/`. Auto-update may swap it, so keep `check_updates` in mind (`features.check_updates = true` today). |
| Wide-char / CJK token boundaries | Port `autocompleteBoundaryRegex` semantics, test with CJK punctuation. |

## Open decisions for you

1. Branch from tag `v0.99.1`, or from current upstream master? Master may have moved the files above.
2. Pure-Rust `ignore` walker (recommended) or shell out to `fd` for exact pi behavior? `fd` is already on
   your PATH, but it adds a runtime dependency and a process per keystroke.
3. Include the `#` trigger? Pi ships it for providers, but there is no jcode provider for it, so it would be empty.
4. Upstream the change as a PR when done? jcode's CONTRIBUTING welcomes PRs, which would remove the
   rebase burden. The change is self-contained, so it is a reasonable candidate.
5. Highlight matched characters in rows (jcode already has the machinery) or match pi's plain rows?

## Estimate

Phases 1 to 2 are the bulk of the logic, but small and well-specified. 3 to 6 touch about five
existing functions. I would expect a working version in one focused session, plus time for the
real-pty parity check. The first full `cargo build --profile selfdev` of this workspace will be the
longest single wait.

## Corrections to my earlier answer

- The "Attachment not found or not a file" string I quoted comes from the Gmail tool, not the TUI.
  jcode's drag-and-drop file handling is in `tui/app/input/paste_guard.rs` and only attaches images.
- jcode does not support `@file` completion today. Confirmed by reading the source, not just the binary.
