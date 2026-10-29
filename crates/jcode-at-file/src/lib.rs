//! `@file` completion for the composer (pi parity).
//!
//! Spec source: `@earendil-works/pi-tui` `autocomplete.js`. Three pure pieces:
//!
//! * [`extract_at_prefix`]: find the `@token` ending at the cursor.
//! * [`search`]: gitignore-aware candidate walk, scored and sorted like pi.
//! * [`apply_completion`]: build the replacement text and cursor.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Max rows kept after ranking.
pub const MAX_RESULTS: usize = 20;
/// Max raw matches per walk pass (mirrors `fd --max-results 100`).
const PASS_LIMIT: usize = 100;

/// One completion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Display path as typed after `@` (scoped prefix + relative path). Dirs end with `/`.
    pub path: String,
    /// File or directory name (no trailing slash).
    pub name: String,
    pub is_dir: bool,
}

/// The `@token` before the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtPrefix {
    /// Byte offset of the `@` in the text.
    pub start: usize,
    /// Everything after `@` up to the cursor (without the opening quote).
    pub query: String,
    /// Prefix used the `@"..."` form.
    pub quoted: bool,
    /// False for a Tab-forced plain path token (no `@` is inserted).
    pub at: bool,
}

fn is_wrapper(c: char) -> bool {
    matches!(c, '(' | '[' | '{' | '<' | '`')
}

fn is_cjk_punct(c: char) -> bool {
    matches!(c, '\u{3000}'..='\u{303F}' | '\u{FF00}'..='\u{FF0F}' | '\u{FF1A}'..='\u{FF20}')
}

fn is_boundary(c: char) -> bool {
    c.is_whitespace() || is_cjk_punct(c)
}

/// Find the `@token` ending at `cursor` (byte offset). `None` when the cursor
/// is not inside one (for example `email@host`).
pub fn extract_at_prefix(text: &str, cursor: usize) -> Option<AtPrefix> {
    let cursor = cursor.min(text.len());
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let before = &text[..cursor];

    // Quoted form: last `@"` at a token start with no closing quote after it.
    if let Some(pos) = before.rfind("@\"") {
        let after_open = &before[pos + 2..];
        if !after_open.contains('"') && starts_token(before, pos) {
            return Some(AtPrefix {
                start: pos,
                query: after_open.to_string(),
                quoted: true,
                at: true,
            });
        }
    }

    // Plain form: the token after the last boundary char, wrappers stripped.
    let token_start = before
        .char_indices()
        .rev()
        .find(|(_, c)| is_boundary(*c))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let token = &before[token_start..];
    let unwrapped = token.trim_start_matches(is_wrapper);
    let rest = unwrapped.strip_prefix('@')?;
    if rest.starts_with('"') {
        return None;
    }
    Some(AtPrefix {
        start: cursor - rest.len() - 1,
        query: rest.to_string(),
        quoted: false,
        at: true,
    })
}

/// Tab-forced completion: the plain token ending at `cursor` (no `@` needed).
/// Prefers the `@` form when the token has one. `None` for an empty token.
pub fn extract_path_token(text: &str, cursor: usize) -> Option<AtPrefix> {
    if let Some(p) = extract_at_prefix(text, cursor) {
        return Some(p);
    }
    let cursor = cursor.min(text.len());
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let before = &text[..cursor];
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| is_boundary(*c))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let token = &before[start..];
    if token.is_empty() || token.contains('"') {
        return None;
    }
    Some(AtPrefix {
        start,
        query: token.to_string(),
        quoted: false,
        at: false,
    })
}

/// Is byte `pos` at a token start: start of text, after a boundary, or after wrappers.
fn starts_token(text: &str, pos: usize) -> bool {
    let head = text[..pos].trim_end_matches(is_wrapper);
    head.is_empty() || head.chars().next_back().is_some_and(is_boundary)
}

/// Score an entry like pi `scoreEntry`. `None` drops the entry.
pub fn score_entry(c: &Candidate, query: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(1);
    }
    let q = query.to_lowercase();
    let name = c.name.to_lowercase();
    let path = c.path.to_lowercase();
    let mut score = if name == q {
        100
    } else if name.starts_with(&q) {
        80
    } else if name.contains(&q) {
        50
    } else if path.contains(&q) {
        30
    } else {
        return None;
    };
    if c.is_dir && score > 0 {
        score += 10;
    }
    Some(score)
}

fn depth(c: &Candidate) -> usize {
    c.path.trim_end_matches('/').matches('/').count()
}

/// Rank, sort and truncate: score desc, depth asc, length asc, alphabetical.
pub fn rank(mut items: Vec<Candidate>, name_query: &str) -> Vec<Candidate> {
    let mut scored: Vec<(u32, Candidate)> = items
        .drain(..)
        .filter_map(|c| score_entry(&c, name_query).map(|s| (s, c)))
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| {
        sb.cmp(sa)
            .then_with(|| depth(a).cmp(&depth(b)))
            .then_with(|| a.path.len().cmp(&b.path.len()))
            .then_with(|| a.path.cmp(&b.path))
    });
    scored.truncate(MAX_RESULTS);
    scored.into_iter().map(|(_, c)| c).collect()
}

/// Split the typed query into (scope prefix incl. trailing `/`, name query).
fn split_scope(query: &str) -> (&str, &str) {
    match query.rfind('/') {
        Some(i) => (&query[..=i], &query[i + 1..]),
        None => ("", query),
    }
}

fn resolve_base(cwd: &Path, scope: &str) -> Option<PathBuf> {
    if scope.is_empty() {
        return Some(cwd.to_path_buf());
    }
    let expanded = if let Some(rest) = scope.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME")?).join(rest)
    } else if scope == "~" {
        PathBuf::from(std::env::var_os("HOME")?)
    } else if Path::new(scope).is_absolute() {
        PathBuf::from(scope)
    } else {
        cwd.join(scope)
    };
    expanded.is_dir().then_some(expanded)
}

fn walk(
    base: &Path,
    scope_prefix: &str,
    needle: &str,
    full_path: bool,
    max_depth: Option<usize>,
    cancel: &AtomicBool,
) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut builder = ignore::WalkBuilder::new(base);
    builder.hidden(false).follow_links(true).max_depth(max_depth);
    builder.filter_entry(|e| e.file_name() != ".git");
    let needle = needle.to_lowercase();
    for entry in builder.build().flatten() {
        if cancel.load(Ordering::Relaxed) {
            return out;
        }
        if entry.depth() == 0 {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(base) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let name = entry.file_name().to_string_lossy().to_string();
        let haystack = if full_path { &rel } else { &name };
        if !needle.is_empty() && !haystack.to_lowercase().contains(&needle) {
            continue;
        }
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        out.push(Candidate {
            path: format!("{scope_prefix}{rel}{}", if is_dir { "/" } else { "" }),
            name,
            is_dir,
        });
        if out.len() >= PASS_LIMIT {
            break;
        }
    }
    out
}

/// Candidates for the typed `query` (text after `@`, no quote), relative to `cwd`.
/// Returns early with a partial result when `cancel` is set.
pub fn search(cwd: &Path, query: &str, cancel: &AtomicBool) -> Vec<Candidate> {
    let (scope, name_query) = split_scope(query);
    let (base, scope_prefix, full_path, needle) = match resolve_base(cwd, scope) {
        Some(base) => (base, scope, false, name_query),
        // Unresolvable scope: match the whole typed path against relative paths.
        None => (cwd.to_path_buf(), "", true, query),
    };
    let needle = needle.trim_end_matches('/');
    // Pass 1: depth-1 entries first. Pass 2: recursive. Merge, de-duplicate.
    let mut merged = walk(&base, scope_prefix, needle, full_path, Some(1), cancel);
    if !cancel.load(Ordering::Relaxed) {
        let deep = walk(&base, scope_prefix, needle, full_path, None, cancel);
        for c in deep {
            if !merged.iter().any(|m| m.path == c.path) {
                merged.push(c);
            }
        }
    }
    rank(merged, if full_path { needle } else { name_query })
}

/// Apply `candidate` to `text`, replacing the prefix through the cursor.
/// Returns `(new_text, new_cursor)`.
pub fn apply_completion(
    text: &str,
    cursor: usize,
    prefix: &AtPrefix,
    candidate: &Candidate,
) -> (String, usize) {
    let cursor = cursor.min(text.len());
    let needs_quotes = prefix.quoted || candidate.path.contains(' ');
    let at = if prefix.at { "@" } else { "" };
    let mut value = if needs_quotes {
        format!("{at}\"{}\"", candidate.path)
    } else {
        format!("{at}{}", candidate.path)
    };
    let mut after = &text[cursor..];
    // Typing inside an existing `@"..."` pair: reuse its closing quote.
    if prefix.quoted && after.starts_with('"') {
        after = &after[1..];
    }
    let mut new_cursor = prefix.start + value.len();
    if candidate.is_dir {
        if needs_quotes {
            new_cursor -= 1; // land before the closing quote, keep drilling
        }
    } else {
        value.push(' ');
        new_cursor = prefix.start + value.len();
    }
    let mut out = String::with_capacity(text.len() + value.len());
    out.push_str(&text[..prefix.start]);
    out.push_str(&value);
    out.push_str(after);
    (out, new_cursor)
}

#[cfg(test)]
mod tests;
