// `/new` starts a fresh session in place, optionally named (pi parity).

use tokio::io::{AsyncBufReadExt, BufReader};

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
