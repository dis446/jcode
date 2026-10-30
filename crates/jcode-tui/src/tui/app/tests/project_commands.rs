// Project-defined slash commands: discovery, precedence, execution intent, and
// the `--initial-message` draft. Included from `app/tests.rs`.

use std::fs;

fn write_project_commands(dir: &std::path::Path, commands: serde_json::Value) {
    let config = dir.join(".jcode");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("commands.json"), commands.to_string()).unwrap();
}

fn app_with_project(dir: &std::path::Path) -> App {
    let mut app = create_test_app();
    app.session.working_dir = Some(dir.to_string_lossy().to_string());
    app
}

#[test]
fn project_commands_are_discoverable_and_paths_stay_prompt_text() {
    let dir = tempfile::tempdir().unwrap();
    write_project_commands(
        dir.path(),
        serde_json::json!({"commands": [
            {"name": "feature-list", "description": "List features", "command": "true"}
        ]}),
    );
    let app = app_with_project(dir.path());

    assert!(
        app.known_command_names()
            .iter()
            .any(|name| name == "/feature-list")
    );
    assert!(super::commands_dispatch::contains_registered_slash_command(
        &app,
        "/feature-list"
    ));
    assert!(!super::commands_dispatch::contains_registered_slash_command(
        &app,
        "/feature-list/extra"
    ));
    assert!(!super::commands_dispatch::contains_registered_slash_command(
        &app,
        "/tmp/feature-list"
    ));

    let suggestions = app.get_suggestions_for("/feature");
    assert!(
        suggestions
            .iter()
            .any(|(name, _)| name == "/feature-list"),
        "{suggestions:?}"
    );
    assert!(
        app.project_commands()
            .iter()
            .any(|command| command.name() == "feature-list"
                && command.description() == "List features")
    );
}

#[test]
fn built_in_command_wins_over_a_project_command() {
    let dir = tempfile::tempdir().unwrap();
    write_project_commands(
        dir.path(),
        serde_json::json!({"commands": [
            {"name": "help", "description": "project help", "command": "true"}
        ]}),
    );
    let app = app_with_project(dir.path());

    assert_eq!(
        app.known_command_names()
            .iter()
            .filter(|name| *name == "/help")
            .count(),
        1
    );
    assert!(
        !app.project_commands()
            .iter()
            .any(|command| command.name() == "help")
    );
}

#[test]
fn switching_project_rebuilds_completion_candidates() {
    let first = tempfile::tempdir().unwrap();
    write_project_commands(
        first.path(),
        serde_json::json!({"commands": [
            {"name": "first-cmd", "description": "First project", "command": "true"}
        ]}),
    );
    let second = tempfile::tempdir().unwrap();
    write_project_commands(
        second.path(),
        serde_json::json!({"commands": [
            {"name": "second-cmd", "description": "Second project", "command": "true"}
        ]}),
    );

    let mut app = app_with_project(first.path());
    assert!(
        app.get_suggestions_for("/first")
            .iter()
            .any(|(name, _)| name == "/first-cmd")
    );

    app.session.working_dir = Some(second.path().to_string_lossy().to_string());
    assert!(
        app.get_suggestions_for("/second")
            .iter()
            .any(|(name, _)| name == "/second-cmd")
    );
    assert!(
        !app.get_suggestions_for("/first")
            .iter()
            .any(|(name, _)| name == "/first-cmd")
    );
}

#[test]
fn resolver_builds_the_shell_line_cwd_and_timeout() {
    let dir = tempfile::tempdir().unwrap();
    write_project_commands(
        dir.path(),
        serde_json::json!({"commands": [
            {
                "name": "feature-start",
                "description": "Start a feature",
                "command": "scripts/feature-start --lead",
                "timeout_secs": 300
            }
        ]}),
    );
    let app = app_with_project(dir.path());

    let super::commands_project::ProjectAction::Run(resolved) =
        super::commands_project::resolve(&app, "/feature-start demo \"two words\"")
    else {
        panic!("expected a runnable project command");
    };
    assert_eq!(resolved.display, "/feature-start demo \"two words\"");
    assert_eq!(
        resolved.line,
        "scripts/feature-start --lead demo \"two words\""
    );
    assert_eq!(
        std::fs::canonicalize(resolved.cwd.as_deref().unwrap()).unwrap(),
        std::fs::canonicalize(dir.path()).unwrap()
    );
    assert_eq!(resolved.timeout_secs, 300);
}

#[test]
fn project_command_runs_without_a_model_turn() {
    let dir = tempfile::tempdir().unwrap();
    write_project_commands(
        dir.path(),
        serde_json::json!({"commands": [
            {"name": "say", "description": "Say something", "command": "printf project-ok"}
        ]}),
    );
    let mut app = app_with_project(dir.path());

    app.set_input_for_test("/say hello");
    app.submit_input();

    assert!(!app.pending_turn, "a project command must not start a turn");
    assert!(app.queued_messages.is_empty());
    assert!(
        app.display_messages()
            .iter()
            .any(|message| message.role == "user" && message.content == "/say hello")
    );
}

#[test]
fn confirm_requires_an_explicit_yes() {
    let dir = tempfile::tempdir().unwrap();
    write_project_commands(
        dir.path(),
        serde_json::json!({"commands": [
            {
                "name": "danger",
                "description": "Destructive",
                "command": "printf danger",
                "confirm": true
            }
        ]}),
    );
    let mut app = app_with_project(dir.path());

    app.set_input_for_test("/danger");
    app.submit_input();
    assert!(app.pending_project_confirm.is_some());
    assert!(
        !app.display_messages()
            .iter()
            .any(|message| message.role == "user" && message.content == "/danger"),
        "nothing may run before confirmation"
    );

    app.set_input_for_test("n");
    app.submit_input();
    assert!(app.pending_project_confirm.is_none());
    assert!(
        app.display_messages()
            .iter()
            .any(|message| message.content.contains("Cancelled /danger"))
    );

    app.set_input_for_test("/danger");
    app.submit_input();
    app.set_input_for_test("y");
    app.submit_input();
    assert!(app.pending_project_confirm.is_none());
    assert!(
        app.display_messages()
            .iter()
            .any(|message| message.role == "user" && message.content == "/danger")
    );
}

#[test]
fn initial_message_prefills_without_sending() {
    let mut app = create_test_app();
    app.prefill_input("Do not call any tools. Wait for instructions.".to_string());
    assert_eq!(app.input(), "Do not call any tools. Wait for instructions.");
    assert!(!app.is_processing());
    assert!(!app.pending_turn);

    app.prefill_input("must not overwrite".to_string());
    assert_eq!(app.input(), "Do not call any tools. Wait for instructions.");
}
