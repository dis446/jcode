// Prompt templates: discovery, expansion into a normal turn, precedence over
// skills, and the `/help`/completion surfaces. Included from `app/tests.rs`.

fn write_template(working_dir: &std::path::Path, name: &str, content: &str) {
    let prompts = working_dir.join(".jcode").join("prompts");
    std::fs::create_dir_all(&prompts).unwrap();
    std::fs::write(prompts.join(format!("{name}.md")), content).unwrap();
}

fn write_project_command(working_dir: &std::path::Path, name: &str) {
    let config = working_dir.join(".jcode");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("commands.json"),
        serde_json::json!({"commands": [
            {"name": name, "description": "project command", "command": "true"}
        ]})
        .to_string(),
    )
    .unwrap();
}

fn write_skill(working_dir: &std::path::Path, name: &str) {
    let skill = working_dir.join(".jcode").join("skills").join(name);
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: A skill that must lose\n---\nSkill body.\n"),
    )
    .unwrap();
}

fn app_with_templates(dir: &std::path::Path) -> App {
    let mut app = create_test_app();
    app.session.working_dir = Some(dir.to_string_lossy().to_string());
    app
}

fn user_messages(app: &App) -> Vec<String> {
    app.display_messages()
        .iter()
        .filter(|message| message.role == "user")
        .map(|message| message.content.clone())
        .collect()
}

#[test]
fn template_expands_into_one_normal_turn() {
    let dir = tempfile::tempdir().unwrap();
    write_template(
        dir.path(),
        "tpl-review",
        "---\ndescription: Review staged changes\n---\nReview the staged changes.",
    );
    let mut app = app_with_templates(dir.path());

    app.set_input_for_test("/tpl-review");
    app.submit_input();

    assert!(app.pending_turn, "a template starts an ordinary turn");
    assert_eq!(user_messages(&app), vec!["Review the staged changes."]);
}

#[test]
fn template_substitutes_quoted_arguments_and_defaults() {
    let dir = tempfile::tempdir().unwrap();
    write_template(dir.path(), "focus", "Focus on $1. All: ${@:-everything}.");
    let mut app = app_with_templates(dir.path());

    app.set_input_for_test("/focus \"API compatibility\"");
    app.submit_input();
    assert_eq!(
        user_messages(&app),
        vec!["Focus on API compatibility. All: API compatibility."]
    );

    let mut app = app_with_templates(dir.path());
    app.set_input_for_test("/focus");
    app.submit_input();
    assert_eq!(user_messages(&app), vec!["Focus on . All: everything."]);
}

#[test]
fn template_shadows_a_same_named_skill() {
    let dir = tempfile::tempdir().unwrap();
    write_skill(dir.path(), "tpl-review");
    write_template(dir.path(), "tpl-review", "Template body.");
    let mut app = app_with_templates(dir.path());

    app.set_input_for_test("/tpl-review");
    app.submit_input();

    assert_eq!(app.active_skill, None, "the skill must not activate");
    assert_eq!(user_messages(&app), vec!["Template body."]);
}

#[test]
fn built_in_and_project_commands_shadow_templates() {
    let dir = tempfile::tempdir().unwrap();
    write_template(dir.path(), "help", "Shadowed by /help.");
    write_template(dir.path(), "say", "Shadowed by the project command.");
    write_project_command(dir.path(), "say");
    let app = app_with_templates(dir.path());

    // Real home templates may also be loaded here, so assert on the two names
    // that must be gone rather than on an empty list.
    let entries = app.prompt_template_entries();
    assert!(
        !entries
            .iter()
            .any(|(name, _)| name == "/help" || name == "/say"),
        "{entries:?}"
    );
    assert_eq!(
        app.known_command_names()
            .iter()
            .filter(|name| *name == "/help")
            .count(),
        1
    );
}

#[test]
fn template_appears_in_help_and_completion() {
    let dir = tempfile::tempdir().unwrap();
    write_template(
        dir.path(),
        "tpl-review",
        "---\ndescription: Review staged changes\nargument-hint: \"[focus]\"\n---\nReview.",
    );
    let app = app_with_templates(dir.path());

    assert!(
        app.prompt_template_entries()
            .iter()
            .any(|(name, description)| name == "/tpl-review"
                && description == "Review staged changes")
    );
    let suggestions = app.get_suggestions_for("/tpl-review");
    assert!(
        suggestions
            .iter()
            .any(|(name, help)| name == "/tpl-review" && *help == "Prompt template"),
        "{suggestions:?}"
    );
}

#[test]
fn editing_a_template_is_picked_up_without_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    write_template(dir.path(), "tpl-review", "First body.");
    let app = app_with_templates(dir.path());
    assert_eq!(
        app.prompt_templates()
            .expand_invocation("/tpl-review")
            .map(|(_name, body)| body),
        Some("First body.".to_string())
    );

    write_template(dir.path(), "tpl-review", "Second body.");
    assert_eq!(
        super::commands_prompt::expand(&app, "/tpl-review"),
        Some("Second body.".to_string()),
        "a slash miss must reload the template directories"
    );
}

#[test]
fn unknown_slash_text_is_not_a_template() {
    let dir = tempfile::tempdir().unwrap();
    write_template(dir.path(), "tpl-review", "Review.");
    let app = app_with_templates(dir.path());

    assert_eq!(super::commands_prompt::expand(&app, "/tmp/notes.md"), None);
    assert_eq!(super::commands_prompt::expand(&app, "/nope"), None);
    assert_eq!(super::commands_prompt::expand(&app, "tpl-review"), None);
}
