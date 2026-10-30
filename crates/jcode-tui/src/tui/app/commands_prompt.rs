//! Prompt-template expansion for `/name` commands.
//!
//! A template is user text with arguments substituted, so expansion happens on
//! the client before the message is sent and needs no protocol support. The
//! registry itself lives in [`crate::prompt_template`]; this module only resolves
//! a typed invocation against the session's current working directory and keeps
//! the reload-on-miss behavior so a newly written file works without a restart.

use super::App;

/// Expanded prompt text when `input` invokes a prompt template.
///
/// Returns `None` for built-in commands (filtered out of the registry), paths
/// that merely start with `/`, and unknown names. The template directories are
/// re-read on every attempt so an edit or a newly added file takes effect
/// immediately; they are a handful of small files, and only slash input that
/// survived the built-in and project-command checks reaches this point.
pub(super) fn expand(app: &App, input: &str) -> Option<String> {
    if !input.trim_start().starts_with('/') {
        return None;
    }
    app.reload_prompt_templates();
    app.prompt_templates()
        .expand_invocation(input)
        .map(|(_name, expanded)| expanded)
}
