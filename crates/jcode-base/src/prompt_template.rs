//! Markdown prompt templates as slash commands.
//!
//! A markdown file becomes a `/name` command whose body is sent as an ordinary
//! user message. Unlike a skill, the body replaces the message rather than
//! becoming a standing system reminder, and arguments typed after the name are
//! substituted into it. The behavior and the substitution table follow pi's
//! prompt templates so a file written for pi works here unchanged.
//!
//! ```markdown
//! ---
//! description: Review staged git changes
//! argument-hint: "[focus]"
//! ---
//! Review the staged changes. Focus on ${1:-correctness, security, and error handling}.
//! ```
//!
//! Loaded later-wins from:
//!
//! 1. `$JCODE_HOME/prompts/*.md`
//! 2. `~/.agents/prompts/*.md` (the cross-tool `.agents` convention; sandboxed
//!    under `$JCODE_HOME/external` like the other external directories)
//! 3. `<working dir>/.jcode/prompts/*.md`
//! 4. `<working dir>/.agents/prompts/*.md`
//!
//! Direct `.md` children only. Nothing executes at load time; a malformed,
//! oversized, or badly named file is skipped with a warning.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Upper bound for one template body. The body is sent as a prompt, so an
/// accidental blob would be charged and compacted as one.
const MAX_BODY_BYTES: usize = 64 * 1024;
const MAX_DESCRIPTION_BYTES: usize = 4 * 1024;
const MAX_NAME_LEN: usize = 64;
const MAX_TEMPLATES: usize = 128;
/// pi truncates a description taken from the body to this many characters.
const FALLBACK_DESCRIPTION_LEN: usize = 60;

/// One markdown prompt template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplate {
    name: String,
    description: String,
    argument_hint: Option<String>,
    body: String,
    path: PathBuf,
}

impl PromptTemplate {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn argument_hint(&self) -> Option<&str> {
        self.argument_hint.as_deref()
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Body with `args` substituted, ready to send as a user message.
    pub fn expand(&self, args: &[String]) -> String {
        substitute(&self.body, args)
    }
}

/// The templates visible to one session working directory.
#[derive(Debug, Clone, Default)]
pub struct PromptTemplateRegistry {
    templates: Vec<PromptTemplate>,
}

impl PromptTemplateRegistry {
    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&PromptTemplate> {
        self.templates.iter().find(|t| t.name == name)
    }

    /// Drop templates the caller wants to shadow (built-ins, project commands).
    pub fn retain(&mut self, keep: impl FnMut(&PromptTemplate) -> bool) {
        self.templates.retain(keep);
    }

    /// Deterministic order for `/help` and completion.
    pub fn list(&self) -> Vec<&PromptTemplate> {
        let mut templates: Vec<&PromptTemplate> = self.templates.iter().collect();
        templates.sort_by(|a, b| a.name.cmp(&b.name));
        templates
    }

    /// Resolve `/name args` to the template name and its expanded body.
    ///
    /// `None` for anything that is not a known template name at the start of
    /// the input, including bare `/` and paths that merely start with `/`.
    pub fn expand_invocation(&self, input: &str) -> Option<(String, String)> {
        let input = input.trim_start();
        let rest = input.strip_prefix('/')?;
        let token_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let name = &rest[..token_end];
        let template = self.get(name)?;
        let args = parse_command_args(rest[token_end..].trim_start());
        Some((template.name.clone(), template.expand(&args)))
    }

    /// Load every template source for `working_dir`, later sources winning on a
    /// name clash.
    pub fn load_for_working_dir(working_dir: Option<&Path>) -> Self {
        let global = crate::storage::jcode_dir()
            .ok()
            .map(|dir| dir.join("prompts"));
        let agents = crate::storage::user_home_path(".agents/prompts").ok();
        Self::load_from_roots(working_dir, global.as_deref(), agents.as_deref())
    }

    /// Explicit roots, so tests and hosts that resolve the directories
    /// themselves never depend on the real home directory. The normal entry
    /// point is [`Self::load_for_working_dir`].
    pub fn load_from_roots(
        working_dir: Option<&Path>,
        global_prompts: Option<&Path>,
        agents_prompts: Option<&Path>,
    ) -> Self {
        let mut registry = Self::default();
        if let Some(dir) = global_prompts {
            registry.load_dir(dir);
        }
        if let Some(dir) = agents_prompts {
            registry.load_dir(dir);
        }
        let root = working_dir.unwrap_or_else(|| Path::new("."));
        registry.load_dir(&root.join(".jcode").join("prompts"));
        registry.load_dir(&root.join(".agents").join("prompts"));
        registry
    }

    fn load_dir(&mut self, dir: &Path) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => {
                crate::logging::warn(&format!(
                    "prompt templates: cannot read {}: {err}",
                    dir.display()
                ));
                return;
            }
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
            .filter(|path| is_regular_file(path))
            .collect();
        paths.sort();
        for path in paths {
            if let Some(template) = Self::load_file(&path) {
                self.merge(template);
            }
        }
    }

    fn load_file(path: &Path) -> Option<PromptTemplate> {
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            crate::logging::warn(&format!(
                "prompt templates: ignoring {}: name must be 1..={MAX_NAME_LEN} characters",
                path.display()
            ));
            return None;
        }
        if !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            crate::logging::warn(&format!(
                "prompt templates: ignoring {}: name may only contain letters, digits, '_' and '-'",
                path.display()
            ));
            return None;
        }
        let raw = match read_bounded(path) {
            Some(raw) => raw,
            None => return None,
        };
        if raw.len() > MAX_BODY_BYTES {
            crate::logging::warn(&format!(
                "prompt templates: ignoring {}: larger than {MAX_BODY_BYTES} bytes",
                path.display()
            ));
            return None;
        }
        let (frontmatter, body) = match split_frontmatter(&raw) {
            Ok(parsed) => parsed,
            Err(err) => {
                crate::logging::warn(&format!(
                    "prompt templates: ignoring {}: {err}",
                    path.display()
                ));
                return None;
            }
        };
        let body = body.trim().to_string();
        if body.is_empty() {
            crate::logging::warn(&format!(
                "prompt templates: ignoring {}: empty body",
                path.display()
            ));
            return None;
        }
        let mut description = frontmatter
            .description
            .as_deref()
            .map(sanitize_one_line)
            .unwrap_or_default();
        if description.is_empty() {
            description = fallback_description(&body);
        }
        if description.len() > MAX_DESCRIPTION_BYTES {
            description = truncate_chars(&description, MAX_DESCRIPTION_BYTES);
        }
        let argument_hint = frontmatter
            .argument_hint
            .as_deref()
            .map(sanitize_one_line)
            .filter(|hint| !hint.is_empty());
        Some(PromptTemplate {
            name,
            description,
            argument_hint,
            body,
            path: path.to_path_buf(),
        })
    }

    /// Later sources win. A replacement keeps its position so ordering stays
    /// stable across reloads, while `list()` sorts by name anyway.
    fn merge(&mut self, template: PromptTemplate) {
        if let Some(existing) = self.templates.iter_mut().find(|t| t.name == template.name) {
            *existing = template;
            return;
        }
        if self.templates.len() >= MAX_TEMPLATES {
            crate::logging::warn(&format!(
                "prompt templates: ignoring '{}': limit is {MAX_TEMPLATES} templates",
                template.name
            ));
            return;
        }
        self.templates.push(template);
    }
}

/// Frontmatter fields jcode understands. Unknown keys are ignored: this is
/// prose configuration, and a file written for a newer pi must still load.
#[derive(Debug, Default, Deserialize)]
struct TemplateFrontmatter {
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "argument-hint")]
    argument_hint: Option<String>,
}

fn is_regular_file(path: &Path) -> bool {
    // Follows symlinks, so a dotfiles-managed template still loads.
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

/// Read a config file with a hard size ceiling, never following special files.
fn read_bounded(path: &Path) -> Option<String> {
    use std::io::Read;
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) => {
            crate::logging::warn(&format!(
                "prompt templates: cannot inspect {}: {err}",
                path.display()
            ));
            return None;
        }
    };
    if !metadata.is_file() {
        crate::logging::warn(&format!(
            "prompt templates: ignoring {}: not a regular file",
            path.display()
        ));
        return None;
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) => {
            crate::logging::warn(&format!(
                "prompt templates: cannot read {}: {err}",
                path.display()
            ));
            return None;
        }
    };
    let limit = (MAX_BODY_BYTES + 1) as u64;
    let mut raw = String::new();
    if let Err(err) = file.take(limit).read_to_string(&mut raw) {
        crate::logging::warn(&format!(
            "prompt templates: cannot read {}: {err}",
            path.display()
        ));
        return None;
    }
    Some(raw)
}

/// Split optional `---` frontmatter from the body. A file without frontmatter
/// is valid; an unterminated block is not.
fn split_frontmatter(raw: &str) -> Result<(TemplateFrontmatter, &str), String> {
    let first_line_end = raw.find('\n').map(|index| index + 1).unwrap_or(raw.len());
    if raw[..first_line_end].trim_end_matches(['\r', '\n']) != "---" {
        return Ok((TemplateFrontmatter::default(), raw));
    }
    let mut offset = first_line_end;
    while offset < raw.len() {
        let line_end = raw[offset..]
            .find('\n')
            .map(|index| offset + index + 1)
            .unwrap_or(raw.len());
        if raw[offset..line_end].trim_end_matches(['\r', '\n']) == "---" {
            let yaml = &raw[first_line_end..offset];
            let body = raw.get(line_end..).unwrap_or_default();
            let frontmatter: TemplateFrontmatter =
                serde_yaml::from_str(yaml).map_err(|err| format!("invalid frontmatter: {err}"))?;
            return Ok((frontmatter, body));
        }
        offset = line_end;
    }
    Err("unterminated frontmatter block".to_string())
}

fn fallback_description(body: &str) -> String {
    let first = body
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim();
    let mut description = truncate_chars(first, FALLBACK_DESCRIPTION_LEN);
    if first.chars().count() > FALLBACK_DESCRIPTION_LEN {
        description.push_str("...");
    }
    description
}

/// Trim and drop control characters: descriptions are drawn directly into
/// `/help`, so another tool's file must not be able to inject escapes.
fn sanitize_one_line(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// Split the typed tail into arguments the way pi does: single or double quotes
/// group a token, quotes are removed, and there are no backslash escapes.
pub fn parse_command_args(args: &str) -> Vec<String> {
    let mut args_out = Vec::new();
    let mut current = String::new();
    let mut has_token = false;
    let mut quote: Option<char> = None;
    for ch in args.chars() {
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            } else {
                current.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
            has_token = true;
        } else if ch.is_whitespace() {
            if has_token {
                args_out.push(std::mem::take(&mut current));
                has_token = false;
            }
        } else {
            current.push(ch);
            has_token = true;
        }
    }
    if has_token {
        args_out.push(current);
    }
    args_out
}

fn argument_value(target: &str, args: &[String], all: &str) -> Option<String> {
    if target == "@" || target == "ARGUMENTS" {
        return Some(all.to_string());
    }
    let index: usize = target.parse().ok()?;
    if index == 0 {
        return Some(String::new());
    }
    Some(args.get(index - 1).cloned().unwrap_or_default())
}

fn slice_args(args: &[String], start: usize, length: Option<usize>) -> String {
    // The typed positions are 1-based; 0 and negative behave like 1.
    let start = start.saturating_sub(1);
    let slice = match length {
        Some(length) => args
            .get(start..start.saturating_add(length))
            .unwrap_or_default(),
        None => args.get(start..).unwrap_or_default(),
    };
    slice.join(" ")
}

/// Substitute pi's placeholders in one pass. Values and defaults are never
/// re-scanned, so an argument containing `$1` stays literal.
pub fn substitute(body: &str, args: &[String]) -> String {
    let all = args.join(" ");
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(index) = rest.find('$') {
        out.push_str(&rest[..index]);
        let after = &rest[index + 1..];
        if let Some(braced) = after.strip_prefix('{')
            && let Some(close) = braced.find('}')
        {
            let spec = &braced[..close];
            if let Some(replacement) = expand_braced(spec, args, &all) {
                out.push_str(&replacement);
                rest = &braced[close + 1..];
                continue;
            }
        }
        if let Some(simple) = expand_simple(after, args, &all) {
            out.push_str(&simple.0);
            rest = &after[simple.1..];
            continue;
        }
        out.push('$');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// `${target:-default}` and `${@:N[:L]}`.
fn expand_braced(spec: &str, args: &[String], all: &str) -> Option<String> {
    if let Some((target, default)) = spec.split_once(":-") {
        let value = argument_value(target, args, all)?;
        return Some(if value.is_empty() {
            default.to_string()
        } else {
            value
        });
    }
    let slice = spec.strip_prefix("@:")?;
    let (start, length) = match slice.split_once(':') {
        Some((start, length)) => (start, Some(length.parse().ok()?)),
        None => (slice, None),
    };
    Some(slice_args(args, start.parse().ok()?, length))
}

/// `$@`, `$ARGUMENTS`, and `$N`. Returns the replacement and the bytes consumed
/// after the `$`.
fn expand_simple(after: &str, args: &[String], all: &str) -> Option<(String, usize)> {
    if let Some(rest) = after.strip_prefix("ARGUMENTS") {
        return Some((all.to_string(), after.len() - rest.len()));
    }
    if after.starts_with('@') {
        return Some((all.to_string(), 1));
    }
    let digits: usize = after.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    // Overflowing positions behave like a missing argument, matching pi.
    let index: usize = after[..digits].parse().unwrap_or(usize::MAX);
    let value = if index == 0 {
        String::new()
    } else {
        args.get(index.saturating_sub(1))
            .cloned()
            .unwrap_or_default()
    };
    Some((value, digits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Project overlay: `<working dir>/.jcode/prompts`.
    fn write_template(dir: &Path, name: &str, content: &str) {
        let prompts = dir.join(".jcode").join("prompts");
        fs::create_dir_all(&prompts).unwrap();
        fs::write(prompts.join(name), content).unwrap();
    }

    /// Global root, passed to the loader explicitly.
    fn write_global_template(root: &Path, name: &str, content: &str) {
        let prompts = root.join("prompts");
        fs::create_dir_all(&prompts).unwrap();
        fs::write(prompts.join(name), content).unwrap();
    }

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn empty_registry(working_dir: &Path) -> PromptTemplateRegistry {
        // Explicit empty global roots: the tests must not discover the running
        // user's `~/.agents/prompts` or `$JCODE_HOME/prompts` files.
        PromptTemplateRegistry::load_from_roots(Some(working_dir), None, None)
    }

    #[test]
    fn loads_bodies_with_and_without_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        write_template(
            dir.path(),
            "review.md",
            "---\ndescription: Review staged changes\nargument-hint: \"[focus]\"\n---\nReview the staged changes.\n",
        );
        write_template(dir.path(), "plain.md", "Explain this code.\n");
        let registry = empty_registry(dir.path());
        let review = registry.get("review").unwrap();
        assert_eq!(review.description(), "Review staged changes");
        assert_eq!(review.argument_hint(), Some("[focus]"));
        assert_eq!(review.body(), "Review the staged changes.");
        assert_eq!(
            registry.get("plain").unwrap().description(),
            "Explain this code."
        );
        assert_eq!(registry.list().len(), 2);
    }

    #[test]
    fn project_templates_override_global_ones() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        write_global_template(home.path(), "review.md", "global body");
        write_template(project.path(), "review.md", "project body");
        let registry = PromptTemplateRegistry::load_from_roots(
            Some(project.path()),
            Some(&home.path().join("prompts")),
            None,
        );
        assert_eq!(registry.get("review").unwrap().body(), "project body");
        assert_eq!(registry.list().len(), 1);
    }

    #[test]
    fn fallback_description_uses_first_body_line_and_truncates() {
        let dir = tempfile::tempdir().unwrap();
        let long = "x".repeat(80);
        write_template(dir.path(), "long.md", &format!("\n\n{long}\n\nmore"));
        let registry = empty_registry(dir.path());
        let template = registry.get("long").unwrap();
        assert_eq!(template.description().chars().count(), 63);
        assert!(template.description().ends_with("..."));
    }

    #[test]
    fn skips_bad_names_empty_bodies_and_broken_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "has space.md", "body");
        write_template(dir.path(), "empty.md", "---\ndescription: x\n---\n\n");
        write_template(
            dir.path(),
            "broken.md",
            "---\ndescription: [unclosed\n---\nbody",
        );
        write_template(dir.path(), "ok.md", "fine");
        let registry = empty_registry(dir.path());
        assert_eq!(registry.list().len(), 1);
        assert!(registry.get("ok").is_some());
    }

    #[test]
    fn ignores_oversized_and_non_markdown_files() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "huge.md", &"x".repeat(MAX_BODY_BYTES + 1));
        write_template(dir.path(), "notes.txt", "not a template");
        fs::create_dir_all(dir.path().join(".jcode").join("prompts").join("nested.md")).unwrap();
        let registry = empty_registry(dir.path());
        assert!(registry.is_empty());
    }

    #[test]
    fn caps_the_number_of_templates() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..(MAX_TEMPLATES + 5) {
            write_template(dir.path(), &format!("t{index}.md"), "body");
        }
        let registry = empty_registry(dir.path());
        assert_eq!(registry.len(), MAX_TEMPLATES);
    }

    #[test]
    fn parses_quoted_arguments_without_backslash_escapes() {
        assert_eq!(parse_command_args("a b"), args(&["a", "b"]));
        assert_eq!(
            parse_command_args("\"API compatibility\" auth"),
            args(&["API compatibility", "auth"])
        );
        assert_eq!(parse_command_args("'a b'c"), args(&["a bc"]));
        assert_eq!(parse_command_args("a\\ b"), args(&["a\\", "b"]));
        assert_eq!(parse_command_args("\"\""), args(&[""]));
        assert_eq!(parse_command_args("   "), Vec::<String>::new());
    }

    #[test]
    fn expands_positional_and_all_arguments() {
        let body = "$1|$2|$@|$ARGUMENTS";
        assert_eq!(substitute(body, &args(&["a", "b"])), "a|b|a b|a b");
        assert_eq!(substitute(body, &args(&[])), "|||");
        assert_eq!(substitute("$9", &args(&["a"])), "");
        assert_eq!(substitute("$0", &args(&["a"])), "");
    }

    #[test]
    fn expands_defaults_and_slices() {
        let list = args(&["a", "b", "c"]);
        assert_eq!(substitute("${1:-fallback}", &list), "a");
        assert_eq!(substitute("${4:-fallback}", &list), "fallback");
        assert_eq!(substitute("${4:-fallback}", &args(&[""])), "fallback");
        assert_eq!(substitute("${@:-none}", &args(&[])), "none");
        assert_eq!(substitute("${ARGUMENTS:-none}", &args(&[])), "none");
        assert_eq!(substitute("${@:2}", &list), "b c");
        assert_eq!(substitute("${@:2:1}", &list), "b");
        assert_eq!(substitute("${@:9}", &list), "");
    }

    #[test]
    fn leaves_unknown_dollars_literal_and_never_recurses() {
        assert_eq!(
            substitute("$foo ${bar} cost$", &args(&[])),
            "$foo ${bar} cost$"
        );
        assert_eq!(substitute("$1", &args(&["$2"])), "$2");
        assert_eq!(substitute("${1:-$2}", &args(&[])), "$2");
    }

    #[test]
    fn resolves_invocations_only_for_known_names() {
        let dir = tempfile::tempdir().unwrap();
        write_template(dir.path(), "review.md", "Focus: ${1:-everything}");
        let registry = empty_registry(dir.path());
        assert_eq!(
            registry.expand_invocation("/review concurrency"),
            Some(("review".to_string(), "Focus: concurrency".to_string()))
        );
        assert_eq!(
            registry.expand_invocation("  /review"),
            Some(("review".to_string(), "Focus: everything".to_string()))
        );
        assert_eq!(registry.expand_invocation("/unknown x"), None);
        assert_eq!(registry.expand_invocation("/tmp/file.md"), None);
        assert_eq!(registry.expand_invocation("review"), None);
    }

    #[test]
    fn descriptions_strip_control_characters() {
        let dir = tempfile::tempdir().unwrap();
        write_template(
            dir.path(),
            "escape.md",
            "---\ndescription: \"clean\\u001b]0;pwned\"\n---\nbody",
        );
        let registry = empty_registry(dir.path());
        assert_eq!(
            registry.get("escape").unwrap().description(),
            "clean]0;pwned"
        );
    }
}
