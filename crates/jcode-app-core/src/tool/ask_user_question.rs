//! Interactive branching questionnaire tool.
//!
//! The tool sends a question tree to an attached client and waits for the
//! user's answers. It deliberately imposes no count limits on nodes or options;
//! the client virtualizes rendering instead.

use super::{
    InteractiveInputKind, InteractiveInputRequest, InteractiveInputResponse, QuestionRequest, Tool,
    ToolContext, ToolOutput,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use jcode_tool_types::{QuestionNode, UserQuestionResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::future::pending;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(super) struct AskUserQuestionTool;

impl AskUserQuestionTool {
    pub(super) fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    root: Option<String>,
    questions: Vec<QuestionNode>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(rename = "accept_large_output")]
    _accept_large_output: Option<bool>,
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        "ask_user_question"
    }

    fn description(&self) -> &str {
        "Ask the user a branching questionnaire instead of guessing. Use it when a \
         decision materially changes the work and cannot be discovered from the \
         repository. Each question carries typed options with a one-line \
         description; the user can also type a custom answer unless allow_custom \
         is false. `next` on an option or question selects the following node, so \
         different answers can lead to different questions, and several options may \
         point at the same node. There is no limit on the number of questions or \
         options. The call blocks until the user answers, cancels, or a \
         timeout_secs you pass elapses. Do not use it for confirmation or for facts \
         you can check yourself."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "intent": super::intent_schema_property(),
                "root": {
                    "type": "string",
                    "description": "Id of the first question. Defaults to the first entry in questions."
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Optional upper bound on the wait. Omit to wait until the user answers or cancels."
                },
                "questions": {
                    "type": "array",
                    "description": "Question nodes. No maximum count.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["id", "question"],
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Stable id referenced by root and next."
                            },
                            "header": {
                                "type": "string",
                                "description": "Optional short label for breadcrumbs and the review screen."
                            },
                            "question": {
                                "type": "string",
                                "description": "The question text."
                            },
                            "options": {
                                "type": "array",
                                "description": "Authored choices. No maximum count.",
                                "items": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "required": ["value", "label"],
                                    "properties": {
                                        "value": {
                                            "type": "string",
                                            "description": "Stable value returned to the model."
                                        },
                                        "label": {
                                            "type": "string",
                                            "description": "Label shown in the dialog."
                                        },
                                        "description": {
                                            "type": "string",
                                            "description": "Optional one-line explanation of this choice."
                                        },
                                        "next": {
                                            "type": "string",
                                            "description": "Question id to ask after this choice."
                                        }
                                    }
                                }
                            },
                            "multi_select": {
                                "type": "boolean",
                                "description": "Allow more than one selected option. Multi-select questions branch through their node-level next."
                            },
                            "allow_custom": {
                                "type": "boolean",
                                "description": "Offer a typed answer row. Defaults to true."
                            },
                            "next": {
                                "type": "string",
                                "description": "Question id to ask when the chosen option has no next, or when the answer is custom."
                            }
                        }
                    }
                }
            },
            "required": ["intent", "questions"]
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let input: Input = serde_json::from_value(input)?;
        let root = validate_tree(&input)?;

        let Some(request_tx) = ctx.stdin_request_tx.as_ref() else {
            bail!("No interactive client is attached. Ask the user in a normal message instead.");
        };

        let Some(intent) = input.intent else {
            bail!("intent is required");
        };

        let request_id = format!(
            "ask-{}-{}",
            ctx.tool_call_id,
            REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let request = InteractiveInputRequest {
            request_id: request_id.clone(),
            tool_call_id: ctx.tool_call_id.clone(),
            prompt: String::new(),
            is_password: false,
            kind: InteractiveInputKind::Questions(QuestionRequest {
                intent,
                root,
                questions: input.questions,
            }),
            response_tx,
        };

        if request_tx.send(request).is_err() {
            bail!("No interactive client is attached. Ask the user in a normal message instead.");
        }

        let interrupted = async {
            match &ctx.graceful_shutdown_signal {
                Some(signal) => signal.notified().await,
                None => pending::<()>().await,
            }
        };

        let result = match input.timeout_secs {
            Some(secs) => {
                tokio::select! {
                    response = response_rx => response,
                    _ = tokio::time::sleep(Duration::from_secs(secs)) => {
                        return Ok(cancelled_output("timed out waiting for the user"));
                    }
                    _ = interrupted => {
                        return Ok(cancelled_output("turn interrupted"));
                    }
                }
            }
            None => {
                tokio::select! {
                    response = response_rx => response,
                    _ = interrupted => {
                        return Ok(cancelled_output("turn interrupted"));
                    }
                }
            }
        };

        match result {
            Ok(InteractiveInputResponse::Questions(UserQuestionResult::Answered { answers })) => {
                let path: Vec<&str> = answers
                    .iter()
                    .map(|answer| answer.question_id.as_str())
                    .collect();
                let body = json!({
                    "status": "answered",
                    "answers": answers,
                    "path": path,
                });
                Ok(ToolOutput::new(serde_json::to_string(&body)?).with_title("Question answered"))
            }
            Ok(InteractiveInputResponse::Questions(UserQuestionResult::Cancelled { reason })) => {
                Ok(cancelled_output(
                    reason
                        .as_deref()
                        .unwrap_or("user cancelled the questionnaire"),
                ))
            }
            Ok(InteractiveInputResponse::Text(_)) => {
                bail!("Interactive client returned stdin input instead of answers")
            }
            Err(_) => Ok(cancelled_output("interactive client disconnected")),
        }
    }
}

fn cancelled_output(reason: &str) -> ToolOutput {
    let body = json!({"status": "cancelled", "reason": reason});
    ToolOutput::new(
        serde_json::to_string(&body).unwrap_or_else(|_| "{\"status\":\"cancelled\"}".into()),
    )
    .with_title("Question cancelled")
}

/// Validate the whole graph before any UI request is sent. The traversal is
/// iterative so a very deep chain cannot overflow the stack.
fn validate_tree(input: &Input) -> Result<String> {
    if input.questions.is_empty() {
        bail!("questions must not be empty");
    }

    let mut nodes: HashMap<&str, &QuestionNode> = HashMap::with_capacity(input.questions.len());
    for node in &input.questions {
        if node.id.trim().is_empty() {
            bail!("question id must not be empty");
        }
        if nodes.insert(node.id.as_str(), node).is_some() {
            bail!("duplicate question id {:?}", node.id);
        }
    }

    let root = input
        .root
        .clone()
        .unwrap_or_else(|| input.questions[0].id.clone());
    if !nodes.contains_key(root.as_str()) {
        bail!("root question {:?} does not exist", root);
    }

    let mut children: HashMap<&str, Vec<&str>> = HashMap::with_capacity(nodes.len());
    for node in &input.questions {
        let mut node_children = Vec::with_capacity(node.options.len() + 1);
        if let Some(next) = node.next.as_deref() {
            if !nodes.contains_key(next) {
                bail!(
                    "question {:?} next references unknown id {:?}",
                    node.id,
                    next
                );
            }
            node_children.push(next);
        }

        let mut values = HashMap::with_capacity(node.options.len());
        for option in &node.options {
            if option.value.trim().is_empty() {
                bail!("question {:?} has an option with an empty value", node.id);
            }
            if values.insert(option.value.as_str(), ()).is_some() {
                bail!(
                    "question {:?} has duplicate option value {:?}",
                    node.id,
                    option.value
                );
            }
            if let Some(next) = option.next.as_deref() {
                if node.multi_select {
                    bail!(
                        "question {:?} is multi_select, so options must not set next",
                        node.id
                    );
                }
                if !nodes.contains_key(next) {
                    bail!(
                        "question {:?} option {:?} next references unknown id {:?}",
                        node.id,
                        option.value,
                        next
                    );
                }
                node_children.push(next);
            }
        }

        if node.options.is_empty() && (!node.allow_custom || node.multi_select) {
            bail!(
                "question {:?} has no options and cannot accept a custom answer",
                node.id
            );
        }

        children.insert(node.id.as_str(), node_children);
    }

    // Iterative DFS with gray/black states. A gray child is a cycle. A black
    // child is a shared node, which is allowed: converging branches are part of
    // the data model.
    let mut state: HashMap<&str, u8> = HashMap::with_capacity(nodes.len());
    let mut stack: Vec<(&str, usize)> = vec![(root.as_str(), 0)];
    state.insert(root.as_str(), 1);
    while let Some((node_id, index)) = stack.last_mut() {
        let next_child = children
            .get(*node_id)
            .and_then(|children| children.get(*index))
            .copied();
        let Some(child) = next_child else {
            state.insert(*node_id, 2);
            stack.pop();
            continue;
        };
        *index += 1;
        match state.get(child).copied().unwrap_or(0) {
            0 => {
                state.insert(child, 1);
                stack.push((child, 0));
            }
            1 => {
                let mut path: Vec<&str> = stack.iter().map(|(id, _)| *id).collect();
                path.push(child);
                bail!("question graph has a cycle: {}", path.join(" -> "));
            }
            _ => {}
        }
    }

    let unreachable: Vec<&str> = input
        .questions
        .iter()
        .map(|node| node.id.as_str())
        .filter(|id| !state.contains_key(id))
        .collect();
    if !unreachable.is_empty() {
        bail!("unreachable question ids: {}", unreachable.join(", "));
    }

    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolExecutionMode;
    use serde_json::json;

    fn input(value: Value) -> Input {
        serde_json::from_value(value).expect("input should deserialize")
    }

    fn context() -> ToolContext {
        ToolContext {
            session_id: "ask-test".into(),
            message_id: "msg".into(),
            tool_call_id: "call".into(),
            working_dir: None,
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: ToolExecutionMode::Direct,
        }
    }

    #[test]
    fn schema_has_no_question_or_option_caps() {
        let schema = AskUserQuestionTool::new().parameters_schema();
        let text = schema.to_string();
        assert!(!text.contains("maxItems"), "{text}");
        assert!(!text.contains("maxLength"), "{text}");
        let questions = &schema["properties"]["questions"];
        assert!(questions.get("maxItems").is_none());
        assert!(
            questions["items"]["properties"]["options"]
                .get("maxItems")
                .is_none()
        );
    }

    #[test]
    fn validates_a_converging_branching_tree() {
        let input = input(json!({
            "intent": "Choose",
            "questions": [
                {"id": "task", "question": "Task?", "options": [
                    {"value": "bug", "label": "Bug", "next": "area"},
                    {"value": "feature", "label": "Feature", "next": "area"}
                ]},
                {"id": "area", "question": "Area?", "options": [
                    {"value": "tui", "label": "TUI"}
                ]}
            ]
        }));
        assert_eq!(validate_tree(&input).unwrap(), "task");
    }

    #[test]
    fn rejects_cycles() {
        let input = input(json!({
            "intent": "Choose",
            "questions": [
                {"id": "a", "question": "A?", "options": [{"value": "x", "label": "X", "next": "b"}]},
                {"id": "b", "question": "B?", "options": [{"value": "y", "label": "Y", "next": "a"}]}
            ]
        }));
        let error = validate_tree(&input).unwrap_err().to_string();
        assert!(error.contains("cycle"), "{error}");
        assert!(error.contains("a -> b -> a"), "{error}");
    }

    #[test]
    fn rejects_unknown_next_and_unreachable_nodes() {
        let unknown = input(json!({
            "intent": "Choose",
            "questions": [
                {"id": "a", "question": "A?", "options": [{"value": "x", "label": "X", "next": "missing"}]}
            ]
        }));
        assert!(
            validate_tree(&unknown)
                .unwrap_err()
                .to_string()
                .contains("unknown")
        );

        let unreachable = input(json!({
            "intent": "Choose",
            "questions": [
                {"id": "a", "question": "A?", "options": [{"value": "x", "label": "X"}]},
                {"id": "b", "question": "B?", "options": [{"value": "y", "label": "Y"}]}
            ]
        }));
        assert!(
            validate_tree(&unreachable)
                .unwrap_err()
                .to_string()
                .contains("unreachable")
        );
    }

    #[test]
    fn rejects_duplicate_ids_and_values() {
        let duplicate_id = input(json!({
            "intent": "Choose",
            "questions": [
                {"id": "a", "question": "A?", "options": [{"value": "x", "label": "X"}]},
                {"id": "a", "question": "B?", "options": [{"value": "y", "label": "Y"}]}
            ]
        }));
        assert!(
            validate_tree(&duplicate_id)
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );

        let duplicate_value = input(json!({
            "intent": "Choose",
            "questions": [
                {"id": "a", "question": "A?", "options": [
                    {"value": "x", "label": "X"},
                    {"value": "x", "label": "Other X"}
                ]}
            ]
        }));
        assert!(
            validate_tree(&duplicate_value)
                .unwrap_err()
                .to_string()
                .contains("duplicate option")
        );
    }

    #[test]
    fn rejects_multi_select_with_option_next() {
        let input = input(json!({
            "intent": "Choose",
            "questions": [
                {"id": "a", "question": "A?", "multi_select": true, "options": [
                    {"value": "x", "label": "X", "next": "b"}
                ]},
                {"id": "b", "question": "B?", "options": [{"value": "y", "label": "Y"}]}
            ]
        }));
        assert!(
            validate_tree(&input)
                .unwrap_err()
                .to_string()
                .contains("multi_select")
        );
    }

    #[test]
    fn accepts_many_questions_and_options() {
        let questions: Vec<Value> = (0..200)
            .map(|index| {
                let options: Vec<Value> = (0..500)
                    .map(|option| json!({"value": format!("v{option}"), "label": format!("L{option}")}))
                    .collect();
                let next = (index + 1 < 200).then(|| format!("q{}", index + 1));
                json!({
                    "id": format!("q{index}"),
                    "question": "Pick",
                    "options": options,
                    "next": next,
                })
            })
            .collect();
        let input = input(json!({"intent": "Choose", "questions": questions}));
        assert_eq!(validate_tree(&input).unwrap(), "q0");
    }

    #[test]
    fn deep_chain_does_not_recurse() {
        let mut nodes = Vec::new();
        for index in 0..20_000 {
            nodes.push(json!({
                "id": format!("q{index}"),
                "question": "Pick",
                "options": [{"value": "x", "label": "X", "next": format!("q{}", index + 1)}]
            }));
        }
        nodes.push(json!({
            "id": "q20000",
            "question": "End",
            "options": [{"value": "done", "label": "Done"}]
        }));
        let input = input(json!({"intent": "Choose", "questions": nodes}));
        assert_eq!(validate_tree(&input).unwrap(), "q0");
    }

    #[tokio::test]
    async fn no_channel_returns_instructional_error() {
        let result = AskUserQuestionTool::new()
            .execute(
                json!({"intent": "Choose", "questions": [{"id": "a", "question": "A?", "options": [{"value": "x", "label": "X"}]}]}),
                context(),
            )
            .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No interactive client")
        );
    }

    #[tokio::test]
    async fn answered_result_contains_answers_and_path() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ctx = context();
        ctx.stdin_request_tx = Some(tx);
        let tool = AskUserQuestionTool::new();
        let handle = tokio::spawn(async move {
            tool.execute(
                json!({"intent": "Choose", "questions": [
                    {"id": "a", "question": "A?", "options": [{"value": "x", "label": "X", "next": "b"}]},
                    {"id": "b", "question": "B?", "options": [{"value": "y", "label": "Y"}]}
                ]}),
                ctx,
            )
            .await
        });
        let request = rx.recv().await.expect("request");
        assert!(matches!(request.kind, InteractiveInputKind::Questions(_)));
        let result = jcode_tool_types::UserQuestionResult::Answered {
            answers: vec![
                jcode_tool_types::QuestionAnswer {
                    question_id: "a".into(),
                    selected: vec!["x".into()],
                    custom: None,
                },
                jcode_tool_types::QuestionAnswer {
                    question_id: "b".into(),
                    selected: vec!["y".into()],
                    custom: None,
                },
            ],
        };
        request
            .response_tx
            .send(InteractiveInputResponse::Questions(result))
            .unwrap();
        let output = handle.await.unwrap().unwrap();
        assert!(output.output.contains("\"path\":[\"a\",\"b\"]"));
        assert_eq!(output.title.as_deref(), Some("Question answered"));
    }

    #[tokio::test]
    async fn cancelled_result_is_not_an_error() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ctx = context();
        ctx.stdin_request_tx = Some(tx);
        let tool = AskUserQuestionTool::new();
        let handle = tokio::spawn(async move {
            tool.execute(
                json!({"intent": "Choose", "questions": [{"id": "a", "question": "A?", "options": [{"value": "x", "label": "X"}]}]}),
                ctx,
            )
            .await
        });
        let request = rx.recv().await.expect("request");
        request
            .response_tx
            .send(InteractiveInputResponse::Questions(
                UserQuestionResult::Cancelled {
                    reason: Some("user pressed escape".into()),
                },
            ))
            .unwrap();
        let output = handle.await.unwrap().unwrap();
        assert!(output.output.contains("\"status\":\"cancelled\""));
    }
}
