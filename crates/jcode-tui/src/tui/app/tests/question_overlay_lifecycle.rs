// CUSTOM (C10): the question dialog lifecycle at the TUI event boundary.
//
// The daemon now keeps `ask_user_question` in the foreground, so a question is
// only followed by `ToolDone` when it was answered, cancelled, timed out, or
// the connection dropped. These tests pin the TUI side of that contract: a
// request opens the dialog, an unrelated tool finishing leaves it open, the
// question's own completion dismisses it and reveals the next queued dialog.

fn question_node(id: &str, question: &str) -> crate::protocol::QuestionNode {
    crate::protocol::QuestionNode {
        id: id.to_string(),
        header: None,
        question: question.to_string(),
        options: vec![crate::protocol::QuestionOption {
            value: "yes".to_string(),
            label: "Yes".to_string(),
            description: None,
            next: None,
        }],
        multi_select: false,
        allow_custom: true,
        next: None,
    }
}

fn question_request(
    request_id: &str,
    tool_call_id: &str,
    question: &str,
) -> crate::protocol::ServerEvent {
    crate::protocol::ServerEvent::UserQuestionRequest {
        request_id: request_id.to_string(),
        tool_call_id: tool_call_id.to_string(),
        intent: "Choose".to_string(),
        root: "q".to_string(),
        questions: vec![question_node("q", question)],
    }
}

#[test]
fn question_request_opens_the_dialog_and_unrelated_tool_done_keeps_it() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    app.handle_server_event(
        question_request("ask-1", "call-question", "Which task?"),
        &mut remote,
    );
    let overlay = app
        .question_overlay
        .as_ref()
        .expect("question request must open the dialog");
    assert_eq!(overlay.borrow().request_id(), "ask-1");

    app.handle_server_event(
        crate::protocol::ServerEvent::ToolDone {
            id: "call-other".to_string(),
            name: "bash".to_string(),
            output: "ok".to_string(),
            error: None,
        },
        &mut remote,
    );
    assert!(
        app.question_overlay.is_some(),
        "another tool finishing must not dismiss the question"
    );

    app.handle_server_event(
        crate::protocol::ServerEvent::ToolDone {
            id: "call-question".to_string(),
            name: "ask_user_question".to_string(),
            output: "{\"status\":\"cancelled\",\"reason\":\"timed out\"}".to_string(),
            error: None,
        },
        &mut remote,
    );
    assert!(
        app.question_overlay.is_none(),
        "the question's own completion must dismiss it"
    );
}

#[test]
fn queued_question_opens_after_the_current_one_finishes() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    app.handle_server_event(question_request("ask-1", "call-a", "First?"), &mut remote);
    app.handle_server_event(question_request("ask-2", "call-b", "Second?"), &mut remote);
    assert_eq!(app.pending_questions.len(), 1);
    assert_eq!(
        app.question_overlay
            .as_ref()
            .map(|overlay| overlay.borrow().request_id().to_string()),
        Some("ask-1".to_string())
    );

    app.handle_server_event(
        crate::protocol::ServerEvent::ToolDone {
            id: "call-a".to_string(),
            name: "ask_user_question".to_string(),
            output: "{\"status\":\"answered\"}".to_string(),
            error: None,
        },
        &mut remote,
    );
    assert!(app.pending_questions.is_empty());
    assert_eq!(
        app.question_overlay
            .as_ref()
            .map(|overlay| overlay.borrow().request_id().to_string()),
        Some("ask-2".to_string())
    );

    app.handle_server_event(crate::protocol::ServerEvent::Interrupted, &mut remote);
    assert!(app.question_overlay.is_none());
}
