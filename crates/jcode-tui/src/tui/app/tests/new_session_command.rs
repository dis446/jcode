// `/new` starts a fresh session in place, optionally named (pi parity).

use tokio::io::{AsyncBufReadExt, BufReader};

fn side_panel_snapshot_with_one_page() -> crate::side_panel::SidePanelSnapshot {
    crate::side_panel::SidePanelSnapshot {
        focus_revision: 0,
        focused_page_id: Some("page-from-old-session".to_string()),
        pages: vec![crate::side_panel::SidePanelPage {
            id: "page-from-old-session".to_string(),
            title: "Old session goals".to_string(),
            file_path: "/tmp/old-session-goals.md".to_string(),
            format: Default::default(),
            pdf_data: None,
            source: Default::default(),
            content: "leftover content".to_string(),
            updated_at_ms: 1,
        }],
    }
}

#[test]
fn local_new_starts_fresh_session_with_name() {
    let mut app = create_test_app();
    let old_id = app.session.id.clone();
    app.session.working_dir = Some("/tmp/new-session-work".to_string());

    assert!(super::commands::handle_session_command(
        &mut app,
        "/new My Session"
    ));

    assert_ne!(app.session.id, old_id);
    assert_eq!(app.session.display_title(), Some("My Session"));
    assert_eq!(
        app.session.working_dir.as_deref(),
        Some("/tmp/new-session-work")
    );
}

#[test]
fn local_new_without_name_leaves_session_unnamed() {
    let mut app = create_test_app();
    let old_id = app.session.id.clone();
    app.session.rename_title(Some("old name".to_string()));

    assert!(super::commands::handle_session_command(&mut app, "/new"));

    assert_ne!(app.session.id, old_id);
    assert_eq!(app.session.display_title(), None);
}

#[test]
fn local_new_requires_a_command_boundary() {
    let mut app = create_test_app();
    let old_id = app.session.id.clone();

    assert!(!super::commands::handle_session_command(&mut app, "/newer"));
    assert!(!super::commands::handle_session_command(&mut app, "/newest"));

    assert_eq!(app.session.id, old_id);
}

#[test]
fn local_new_trims_the_name() {
    let mut app = create_test_app();

    assert!(super::commands::handle_session_command(
        &mut app,
        "/new   spaced out name  "
    ));

    assert_eq!(app.session.display_title(), Some("spaced out name"));
}

#[test]
fn new_is_registered_for_discovery() {
    assert!(super::registered_command_names().any(|name| name == "/new"));
    assert!(super::registered_command_entries().any(|(name, _)| name == "/new"));
}

#[test]
fn local_new_clears_side_panel_pages() {
    let mut app = create_test_app();
    app.apply_side_panel_snapshot(side_panel_snapshot_with_one_page());
    assert!(app.side_panel.has_pages());

    assert!(super::commands::handle_session_command(&mut app, "/new"));

    assert!(!app.side_panel.has_pages());
    assert!(app.side_panel.focused_page_id.is_none());
}

#[test]
fn remote_new_clears_side_panel_pages() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    remote.mark_history_loaded();
    app.is_remote = true;
    app.apply_side_panel_snapshot(side_panel_snapshot_with_one_page());
    app.input = "/new".to_string();
    app.cursor_pos = app.input.len();

    rt.block_on(app.handle_remote_key(KeyCode::Enter, KeyModifiers::empty(), &mut remote))
        .expect("remote /new should succeed");

    assert!(!app.side_panel.has_pages());
    assert!(app.side_panel.focused_page_id.is_none());
}

#[test]
fn local_new_refuses_while_processing() {
    let mut app = create_test_app();
    let old_id = app.session.id.clone();
    app.is_processing = true;

    assert!(super::commands::handle_session_command(
        &mut app,
        "/new named"
    ));

    assert_eq!(app.session.id, old_id);
    assert!(app.session.display_title().is_none());
    assert!(
        app.display_messages()
            .last()
            .is_some_and(|message| message.content.contains("while a turn is running"))
    );
}

#[test]
fn remote_new_refuses_while_processing() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    let peer = remote
        .take_dummy_peer()
        .expect("dummy remote should retain peer stream");
    let (reader, _writer) = peer.into_split();
    let mut reader = BufReader::new(reader);
    remote.mark_history_loaded();
    app.is_remote = true;
    app.is_processing = true;
    app.input = "/new named".to_string();
    app.cursor_pos = app.input.len();

    rt.block_on(app.handle_remote_key(KeyCode::Enter, KeyModifiers::empty(), &mut remote))
        .expect("remote /new should be handled locally");

    assert!(
        app.display_messages()
            .last()
            .is_some_and(|message| message.content.contains("while a turn is running"))
    );
    let sent = rt.block_on(async {
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_millis(250),
            reader.read_line(&mut line),
        )
        .await
    });
    assert!(sent.is_err(), "busy /new must not send Clear or RenameSession");
}

#[test]
fn remote_new_sends_clear_then_rename() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    let peer = remote
        .take_dummy_peer()
        .expect("dummy remote should retain peer stream");
    let (reader, _writer) = peer.into_split();
    let mut reader = BufReader::new(reader);
    remote.mark_history_loaded();
    app.is_remote = true;
    app.input = "/new My Session".to_string();
    app.cursor_pos = app.input.len();

    rt.block_on(app.handle_remote_key(KeyCode::Enter, KeyModifiers::empty(), &mut remote))
        .expect("remote /new should succeed");

    let first = rt
        .block_on(async {
            let mut line = String::new();
            reader.read_line(&mut line).await?;
            Ok::<_, std::io::Error>(line)
        })
        .expect("clear request should be readable");
    let second = rt
        .block_on(async {
            let mut line = String::new();
            reader.read_line(&mut line).await?;
            Ok::<_, std::io::Error>(line)
        })
        .expect("rename request should be readable");

    assert!(matches!(
        serde_json::from_str::<crate::protocol::Request>(&first)
            .expect("clear request should deserialize"),
        crate::protocol::Request::Clear { .. }
    ));
    match serde_json::from_str::<crate::protocol::Request>(&second)
        .expect("rename request should deserialize")
    {
        crate::protocol::Request::RenameSession { title, .. } => {
            assert_eq!(title.as_deref(), Some("My Session"));
        }
        other => panic!("expected RenameSession, got {other:?}"),
    }
}
