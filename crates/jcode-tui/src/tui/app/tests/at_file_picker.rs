// CUSTOM (fork ledger C1): `@file` popup behavior through the real key path.

fn at_file_app(files: &[&str]) -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for f in files {
        let p = dir.path().join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "").unwrap();
    }
    let mut app = create_test_app();
    app.session.working_dir = Some(dir.path().to_string_lossy().into_owned());
    (app, dir)
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(KeyCode::Char(c), KeyModifiers::NONE).unwrap();
    }
    // Searches may finish on the idle tick.
    for _ in 0..200 {
        if app.at_file_pending.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        app.poll_at_file();
    }
}

fn first_row(app: &App) -> Option<String> {
    app.at_file_view().map(|v| v.rows[0].1.clone())
}

#[test]
fn at_file_t_at_opens_popup_and_best_match_is_first() {
    let (mut app, _d) = at_file_app(&["README.md", "src/main.rs"]);
    type_str(&mut app, "@readm");
    assert_eq!(first_row(&app).as_deref(), Some("README.md"));
}

#[test]
fn at_file_t_email_does_not_open_popup() {
    let (mut app, _d) = at_file_app(&["README.md"]);
    type_str(&mut app, "mail me at x@rea");
    assert!(app.at_file_view().is_none());
}

#[test]
fn at_file_t_enter_accepts_and_does_not_submit() {
    let (mut app, _d) = at_file_app(&["README.md"]);
    type_str(&mut app, "see @readm");
    app.handle_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert_eq!(app.input, "see @README.md ");
    assert_eq!(app.cursor_pos, app.input.len());
    assert!(app.at_file_view().is_none());
    assert!(app.display_messages.iter().all(|m| !m.content.contains("README")));
}

#[test]
fn at_file_t_tab_on_dir_keeps_drilling() {
    let (mut app, _d) = at_file_app(&["src/main.rs", "src/lib.rs"]);
    type_str(&mut app, "@sr");
    app.handle_key(KeyCode::Tab, KeyModifiers::NONE).unwrap();
    assert_eq!(app.input, "@src/");
    let rows: Vec<_> = app.at_file_view().expect("popup stays open").rows;
    assert!(rows.iter().any(|r| r.1 == "src/main.rs"));
}

#[test]
fn at_file_t_esc_closes_popup_before_clearing_input() {
    let (mut app, _d) = at_file_app(&["README.md"]);
    type_str(&mut app, "@rea");
    app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
    assert!(app.at_file_view().is_none());
    assert_eq!(app.input, "@rea");
}

#[test]
fn at_file_t_up_down_move_selection_and_wrap() {
    let (mut app, _d) = at_file_app(&["a1.rs", "a2.rs"]);
    type_str(&mut app, "@a");
    app.handle_key(KeyCode::Down, KeyModifiers::NONE).unwrap();
    assert_eq!(app.at_file_view().unwrap().selected, 1);
    app.handle_key(KeyCode::Down, KeyModifiers::NONE).unwrap();
    assert_eq!(app.at_file_view().unwrap().selected, 0);
    app.handle_key(KeyCode::Up, KeyModifiers::NONE).unwrap();
    assert_eq!(app.at_file_view().unwrap().selected, 1);
}

#[test]
fn at_file_t_backspace_updates_and_closes() {
    let (mut app, _d) = at_file_app(&["README.md"]);
    type_str(&mut app, "@readm");
    app.handle_key(KeyCode::Backspace, KeyModifiers::NONE).unwrap();
    assert!(app.at_file_view().is_some());
    for _ in 0..4 {
        app.handle_key(KeyCode::Backspace, KeyModifiers::NONE).unwrap();
    }
    assert_eq!(app.input, "@");
    app.handle_key(KeyCode::Backspace, KeyModifiers::NONE).unwrap();
    assert!(app.at_file_view().is_none());
}

#[test]
fn at_file_t_paste_never_opens_popup() {
    let (mut app, _d) = at_file_app(&["README.md"]);
    app.handle_paste("@readm".to_string());
    assert!(app.at_file_view().is_none());
}

#[test]
fn at_file_t_tab_forces_path_completion_on_plain_token() {
    let (mut app, _d) = at_file_app(&["only_one.txt"]);
    type_str(&mut app, "cat only_o");
    app.handle_key(KeyCode::Tab, KeyModifiers::NONE).unwrap();
    assert_eq!(app.input, "cat only_one.txt ");
}

#[test]
fn at_file_t_slash_commands_unchanged() {
    let (mut app, _d) = at_file_app(&["model.rs"]);
    type_str(&mut app, "/mod");
    assert!(app.at_file_view().is_none());
    assert!(!app.command_suggestions().is_empty());
}

#[test]
fn at_file_t_undo_restores_pre_completion_input() {
    let (mut app, _d) = at_file_app(&["README.md"]);
    type_str(&mut app, "@readm");
    app.handle_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    app.undo_input_change();
    assert_eq!(app.input, "@readm");
}
