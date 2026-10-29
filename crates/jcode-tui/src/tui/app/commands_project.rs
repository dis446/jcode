//! Dispatch for `.jcode/commands.json` entries.
//!
//! A project command runs one shell line in the daemon, so it needs no model
//! turn, no provider tokens, and no tool list. The client only resolves the
//! name, arguments, working directory, and deadline, then reuses the existing
//! `Request::InputShell` surface that renders `!cmd` results.
//!
//! Precedence is built-in command > project command > skill. Built-in names
//! are filtered when the registry loads, and this module resolves a project
//! command before the skill invocation path runs.

use super::App;
use super::DisplayMessage;
use crate::tui::backend::RemoteConnection;
use anyhow::Result;

/// One ready-to-run project command.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedProjectCommand {
    /// `/name args`, shown in the transcript and confirmation prompt.
    pub display: String,
    /// Shell line to run.
    pub line: String,
    /// Directory override; `None` uses the session working directory.
    pub cwd: Option<String>,
    pub timeout_secs: u64,
}

pub(super) enum ProjectAction {
    NotHandled,
    Run(ResolvedProjectCommand),
    Ask(ResolvedProjectCommand),
}

pub(super) enum ConfirmationOutcome {
    Confirmed(ResolvedProjectCommand),
    Cancelled,
}

/// Resolve a typed `/name ...` against the project commands. Built-in names
/// are never claimed here.
pub(super) fn resolve(app: &App, input: &str) -> ProjectAction {
    let input = input.trim_start();
    if !input.starts_with('/') {
        return ProjectAction::NotHandled;
    }
    let token_end = input.find(char::is_whitespace).unwrap_or(input.len());
    let name = &input[1..token_end];
    if name.is_empty() {
        return ProjectAction::NotHandled;
    }
    let args = input[token_end..].trim_start();

    let commands = app.project_commands();
    let Some(command) = commands.iter().find(|command| command.name() == name) else {
        return ProjectAction::NotHandled;
    };

    let display = if args.is_empty() {
        format!("/{name}")
    } else {
        format!("/{name} {args}")
    };
    let resolved = ResolvedProjectCommand {
        display,
        line: command.shell_line(args),
        cwd: command
            .working_dir(app.session.working_dir.as_deref().map(std::path::Path::new))
            .map(|dir| dir.to_string_lossy().into_owned()),
        timeout_secs: command.timeout_secs(),
    };

    if command.requires_confirmation() {
        ProjectAction::Ask(resolved)
    } else {
        ProjectAction::Run(resolved)
    }
}

/// Consume `y`/`yes` (or anything else) for a pending destructive command.
/// Returns `None` when nothing was pending, so the input continues normally.
pub(super) fn take_confirmation(app: &mut App, input: &str) -> Option<ConfirmationOutcome> {
    let pending = app.pending_project_confirm.take()?;
    let answer = input.trim().to_ascii_lowercase();
    if answer == "y" || answer == "yes" {
        Some(ConfirmationOutcome::Confirmed(pending))
    } else {
        app.push_display_message(DisplayMessage::system(format!(
            "Cancelled {}.",
            pending.display
        )));
        app.set_status_notice("Project command cancelled");
        Some(ConfirmationOutcome::Cancelled)
    }
}

fn remember(app: &mut App, resolved: ResolvedProjectCommand) {
    let directory = resolved
        .cwd
        .clone()
        .unwrap_or_else(|| "the session directory".to_string());
    app.push_display_message(DisplayMessage::system(format!(
        "Run {}?\nRuns a shell command in {}.\nPress y to run, anything else to cancel.",
        resolved.display, directory
    )));
    app.set_status_notice("Project command waiting for confirmation");
    app.pending_project_confirm = Some(resolved);
}

fn run_local(app: &mut App, resolved: ResolvedProjectCommand) {
    app.run_local_project_shell(&resolved);
}

async fn run_remote(
    app: &mut App,
    remote: &mut RemoteConnection,
    resolved: ResolvedProjectCommand,
) -> Result<()> {
    if crate::tui::is_ssh_remote() {
        super::commands_dispatch::ssh_local_action_blocked(app, &resolved.display);
        return Ok(());
    }
    super::remote::run_remote_shell(
        app,
        remote,
        resolved.display,
        resolved.line,
        resolved.cwd,
        Some(resolved.timeout_secs),
    )
    .await
}

/// Local (in-process) dispatch. Called last in `dispatch_single_local_command`,
/// so every built-in wins.
pub(super) fn handle_local(app: &mut App, input: &str) -> bool {
    // The normal TUI is a remote client; its project commands go through
    // `handle_remote` so the daemon runs them. This path only serves a truly
    // in-process app (tests, replay, local tools).
    if app.is_remote {
        return false;
    }
    if let Some(outcome) = take_confirmation(app, input) {
        if let ConfirmationOutcome::Confirmed(resolved) = outcome {
            run_local(app, resolved);
        }
        return true;
    }
    match resolve(app, input) {
        ProjectAction::NotHandled => false,
        ProjectAction::Ask(resolved) => {
            remember(app, resolved);
            true
        }
        ProjectAction::Run(resolved) => {
            run_local(app, resolved);
            true
        }
    }
}

/// Remote-client dispatch. Runs before skill resolution so a same-named skill
/// cannot steal the command.
pub(in crate::tui::app) async fn handle_remote(
    app: &mut App,
    remote: &mut RemoteConnection,
    input: &str,
) -> Result<bool> {
    if let Some(outcome) = take_confirmation(app, input) {
        if let ConfirmationOutcome::Confirmed(resolved) = outcome {
            run_remote(app, remote, resolved).await?;
        }
        return Ok(true);
    }
    match resolve(app, input) {
        ProjectAction::NotHandled => Ok(false),
        ProjectAction::Ask(resolved) => {
            remember(app, resolved);
            Ok(true)
        }
        ProjectAction::Run(resolved) => {
            run_remote(app, remote, resolved).await?;
            Ok(true)
        }
    }
}
