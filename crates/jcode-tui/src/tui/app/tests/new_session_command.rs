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

fn read_request<R: tokio::io::AsyncRead + Unpin>(
    rt: &tokio::runtime::Runtime,
    reader: &mut BufReader<R>,
) -> crate::protocol::Request {
    let line = rt.block_on(async {
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            reader.read_line(&mut line),
        )
        .await
        .expect("request should arrive")
        .expect("request should be readable");
        line
    });
    serde_json::from_str(&line).expect("request should deserialize")
}

fn no_request_within<R: tokio::io::AsyncRead + Unpin>(
    rt: &tokio::runtime::Runtime,
    reader: &mut BufReader<R>,
    millis: u64,
) -> bool {
    rt.block_on(async {
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_millis(millis),
            reader.read_line(&mut line),
        )
        .await
    })
    .is_err()
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
fn local_clear_preserves_working_dir() {
    let mut app = create_test_app();
    let old_id = app.session.id.clone();
    app.session.working_dir = Some("/tmp/clear-session-work".to_string());

    assert!(super::commands::handle_session_command(&mut app, "/clear"));

    assert_ne!(app.session.id, old_id);
    assert_eq!(
        app.session.working_dir.as_deref(),
        Some("/tmp/clear-session-work")
    );
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

    app.handle_server_event(
        crate::protocol::ServerEvent::SessionId {
            session_id: "session_after_new".to_string(),
        },
        &mut remote,
    );
    app.handle_server_event(
        crate::protocol::ServerEvent::Done {
            id: app
                .pending_remote_new_session
                .as_ref()
                .expect("pending /new should exist")
                .clear_request_id,
        },
        &mut remote,
    );
    rt.block_on(super::remote::process_pending_remote_new_session(
        &mut app,
        &mut remote,
    ));

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
fn remote_new_waits_for_clear_confirmation_before_resetting() {
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
    let old_id = app.session.id.clone();
    app.push_display_message(DisplayMessage::system(
        "old transcript".to_string(),
    ));
    app.active_skill = Some("old-skill".to_string());
    app.improve_mode = Some(crate::tui::app::ImproveMode::ImproveRun);
    app.session.improve_mode = Some(crate::session::SessionImproveMode::ImproveRun);
    app.provider_session_id = Some("old-provider-session".to_string());
    app.session.provider_session_id = Some("old-provider-session".to_string());
    app.input = "/new My Session".to_string();
    app.cursor_pos = app.input.len();

    rt.block_on(app.handle_remote_key(KeyCode::Enter, KeyModifiers::empty(), &mut remote))
        .expect("remote /new should succeed");

    let clear = read_request(&rt, &mut reader);
    let clear_id = match clear {
        crate::protocol::Request::Clear { id } => id,
        other => panic!("expected Clear, got {other:?}"),
    };
    assert!(
        no_request_within(&rt, &mut reader, 250),
        "rename must wait for the Clear acknowledgement"
    );
    assert_eq!(app.session.id, old_id);
    assert!(
        app.display_messages()
            .iter()
            .any(|message| message.content == "old transcript")
    );
    assert_eq!(
        app.pending_remote_new_session
            .as_ref()
            .map(|pending| pending.clear_request_id),
        Some(clear_id)
    );

    let confirmed_id = "session_after_new".to_string();
    app.handle_server_event(
        crate::protocol::ServerEvent::SessionId {
            session_id: confirmed_id.clone(),
        },
        &mut remote,
    );
    assert!(
        app.display_messages()
            .iter()
            .any(|message| message.content == "old transcript"),
        "SessionId alone must not finish /new before the Clear Done arrives"
    );
    app.handle_server_event(
        crate::protocol::ServerEvent::Done { id: clear_id },
        &mut remote,
    );
    rt.block_on(super::remote::process_pending_remote_new_session(
        &mut app,
        &mut remote,
    ));

    assert!(
        !app.display_messages()
            .iter()
            .any(|message| message.content == "old transcript"),
        "the confirmed clear must reset the old transcript"
    );
    assert_eq!(app.active_skill, None);
    assert_eq!(app.improve_mode, None);
    assert_eq!(app.session.improve_mode, None);
    assert_eq!(app.provider_session_id, None);
    assert_eq!(app.session.provider_session_id, None);
    assert_eq!(
        app.status_notice().as_deref(),
        Some("New session: My Session")
    );

    match read_request(&rt, &mut reader) {
        crate::protocol::Request::RenameSession { title, .. } => {
            assert_eq!(title.as_deref(), Some("My Session"));
        }
        other => panic!("expected RenameSession, got {other:?}"),
    }

    app.handle_server_event(
        crate::protocol::ServerEvent::SessionRenamed {
            session_id: confirmed_id,
            title: Some("My Session".to_string()),
            display_title: "My Session".to_string(),
        },
        &mut remote,
    );
    assert_eq!(app.session.display_title(), Some("My Session"));
    assert!(
        !app.display_messages()
            .iter()
            .any(|message| message.content.contains("Renamed session")),
        "the /new rename must not print a second rename notice"
    );
    assert_eq!(
        app.status_notice().as_deref(),
        Some("New session: My Session")
    );
    assert!(app.pending_new_session_rename.is_none());
}

#[test]
fn remote_new_clear_rejection_keeps_old_view_and_sends_no_rename() {
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
    let old_id = app.session.id.clone();
    app.push_display_message(DisplayMessage::system(
        "old transcript".to_string(),
    ));
    app.input = "/new Rejected".to_string();
    app.cursor_pos = app.input.len();

    rt.block_on(app.handle_remote_key(KeyCode::Enter, KeyModifiers::empty(), &mut remote))
        .expect("remote /new should send Clear");

    let clear_id = match read_request(&rt, &mut reader) {
        crate::protocol::Request::Clear { id } => id,
        other => panic!("expected Clear, got {other:?}"),
    };
    app.handle_server_event(
        crate::protocol::ServerEvent::Error {
            id: clear_id,
            message:
                "Cannot handle clear while the session is busy. Try again after the current turn finishes."
                    .to_string(),
            retry_after_secs: Some(1),
        },
        &mut remote,
    );

    assert_eq!(app.session.id, old_id);
    assert!(
        app.display_messages()
            .iter()
            .any(|message| message.content == "old transcript"),
        "a rejected clear must not blank the old transcript"
    );
    assert!(app.pending_remote_new_session.is_none());
    assert_eq!(app.status_notice().as_deref(), Some("New session failed"));
    assert!(
        app.display_messages()
            .iter()
            .any(|message| message.content.contains("Cannot start a new session"))
    );
    assert!(
        no_request_within(&rt, &mut reader, 250),
        "a rejected clear must not send RenameSession"
    );
}
