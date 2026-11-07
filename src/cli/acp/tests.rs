use super::*;
use std::path::Path;

#[cfg(unix)]
#[tokio::test]
async fn acp_new_and_resumed_sessions_advertise_server_failover() {
    for resume in [false, true] {
        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        let (reader, writer) = client.into_split();
        let daemon = tokio::spawn(async move {
            let (reader, mut writer) = server.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let request: Request = serde_json::from_str(&line).unwrap();
            let Request::Subscribe {
                id,
                supports_server_failover,
                target_session_id,
                ..
            } = request
            else {
                panic!("expected ACP subscription");
            };
            assert!(
                supports_server_failover,
                "ACP must delegate account failover to the daemon"
            );
            assert_eq!(target_session_id.as_deref(), resume.then_some("existing"));
            // Stop after the subscription, without needing a live provider or daemon.
            let response = json!({"type": "error", "id": id, "message": "subscription captured"});
            writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        });
        let runtime = AcpRuntime::new(AcpProfile::Standard, ProviderChoice::Claude, None, None);
        let cwd = PathBuf::from("/acp-test");
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            if resume {
                runtime
                    .attach_existing_session_connected(
                        "existing".into(),
                        cwd,
                        false,
                        reader,
                        writer,
                    )
                    .await
            } else {
                runtime
                    .create_new_session_connected(cwd, reader, writer)
                    .await
            }
        })
        .await
        .expect("ACP subscription timed out");
        daemon.await.unwrap();
        assert_eq!(
            result
                .err()
                .expect("mock daemon rejects subscription")
                .to_string(),
            "subscription captured"
        );
    }
}

/// Mock daemon for session setup: the bootstrap History carries a cold
/// (empty) model list, and the follow-up `GetModelCatalog` either returns the
/// full catalog or fails.
#[cfg(unix)]
async fn mock_daemon_session_setup(
    server: tokio::net::UnixStream,
    resume: bool,
    catalog: Option<Vec<String>>,
) {
    let (reader, mut writer) = server.into_split();
    let mut reader = BufReader::new(reader);
    let history = |id: u64, models: Vec<String>| {
        json!({
            "type": "history", "id": id, "session_id": "s1", "messages": [],
            "provider_name": "claude", "provider_model": "claude-opus-5-5",
            "available_models": models,
        })
    };
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.unwrap() == 0 {
            return;
        }
        let responses = match serde_json::from_str::<Request>(&line).unwrap() {
            Request::Subscribe { id, .. } if resume => {
                vec![history(id, Vec::new()), json!({"type": "done", "id": id})]
            }
            Request::Subscribe { id, .. } => vec![json!({"type": "done", "id": id})],
            Request::GetHistory { id } => vec![history(id, Vec::new())],
            Request::GetModelCatalog { id, .. } => match &catalog {
                Some(models) => vec![history(id, models.clone())],
                None => vec![json!({"type": "error", "id": id, "message": "catalog failed"})],
            },
            other => panic!("unexpected request {other:?}"),
        };
        for response in responses {
            writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn acp_sessions_fetch_full_model_catalog_when_history_is_cold() {
    let catalog: Vec<String> = ["claude-opus-5-5", "deepseek-v4.1-flash", "gpt-5.2"]
        .map(String::from)
        .to_vec();
    for resume in [false, true] {
        for catalog in [Some(catalog.clone()), None] {
            let (client, server) = tokio::net::UnixStream::pair().unwrap();
            let (reader, writer) = client.into_split();
            let daemon = tokio::spawn(mock_daemon_session_setup(server, resume, catalog.clone()));
            let runtime = AcpRuntime::new(AcpProfile::Standard, ProviderChoice::Claude, None, None);
            let cwd = PathBuf::from("/acp-test");
            let session = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                if resume {
                    runtime
                        .attach_existing_session_connected("s1".into(), cwd, false, reader, writer)
                        .await
                } else {
                    runtime
                        .create_new_session_connected(cwd, reader, writer)
                        .await
                }
            })
            .await
            .expect("ACP session setup timed out")
            .expect("ACP session setup succeeds even if the catalog request fails");
            let state = session.ui_state.lock().await.clone();
            let expected = catalog.clone().unwrap_or_default();
            assert_eq!(state.available_models, expected, "resume={resume}");
            assert_eq!(state.model.as_deref(), Some("claude-opus-5-5"));

            let options = session_config_options(&state);
            let model_values = options
                .iter()
                .find(|option| option["id"] == "model")
                .expect("model config option")["options"]
                .as_array()
                .unwrap()
                .len();
            assert_eq!(model_values, expected.len().max(1), "resume={resume}");
            drop(session);
            daemon.await.unwrap();
        }
    }
}

#[test]
fn acp_initial_model_spec_uses_the_explicit_provider() {
    let runtime = |provider_choice, model: Option<&str>| {
        AcpRuntime::new(
            AcpProfile::Standard,
            provider_choice,
            model.map(str::to_string),
            None,
        )
    };

    assert_eq!(
        runtime(ProviderChoice::OpencodeGo, Some("claude-opus-5-5")).initial_model_spec(),
        Some("opencode-go:claude-opus-5-5".to_string())
    );
    assert_eq!(
        runtime(ProviderChoice::Auto, Some("claude-opus-5-5")).initial_model_spec(),
        Some("claude-opus-5-5".to_string()),
        "auto must not invent a provider prefix"
    );
    assert_eq!(
        runtime(
            ProviderChoice::OpencodeGo,
            Some("claude:claude-opus-5-5")
        )
        .initial_model_spec(),
        Some("claude:claude-opus-5-5".to_string()),
        "an explicitly prefixed model must not be prefixed twice"
    );
    assert_eq!(
        runtime(ProviderChoice::Claude, None).initial_model_spec(),
        None
    );
}

#[cfg(unix)]
async fn mock_daemon_initial_model(
    server: tokio::net::UnixStream,
    requested_model: tokio::sync::mpsc::UnboundedSender<String>,
) {
    let (reader, mut writer) = server.into_split();
    let mut reader = BufReader::new(reader);
    let history = |id: u64| {
        json!({
            "type": "history", "id": id, "session_id": "s1", "messages": [],
            "provider_name": "claude", "provider_model": "claude-opus-5-5",
            "available_models": ["claude-opus-5-5", "deepseek-v4.1-flash"],
        })
    };
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.unwrap() == 0 {
            return;
        }
        let responses = match serde_json::from_str::<Request>(&line).unwrap() {
            Request::Subscribe { id, .. } => vec![json!({"type": "done", "id": id})],
            Request::GetHistory { id } => vec![history(id)],
            Request::GetModelCatalog { id, .. } => vec![history(id)],
            Request::SetModel { id, model } => {
                requested_model.send(model.clone()).unwrap();
                vec![json!({
                    "type": "model_changed", "id": id,
                    "model": model.rsplit(':').next().unwrap_or(&model),
                    "provider_name": "Claude",
                })]
            }
            other => panic!("unexpected request {other:?}"),
        };
        for response in responses {
            writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn acp_initial_model_flag_is_applied_to_the_new_session() {
    let (client, server) = tokio::net::UnixStream::pair().unwrap();
    let (reader, writer) = client.into_split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let daemon = tokio::spawn(mock_daemon_initial_model(server, tx));
    let runtime = AcpRuntime::new(
        AcpProfile::Standard,
        ProviderChoice::OpencodeGo,
        Some("claude-opus-5-5".to_string()),
        None,
    );

    let session = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        runtime.create_new_session_connected(PathBuf::from("/acp-test"), reader, writer),
    )
    .await
    .expect("ACP session setup timed out")
    .expect("ACP session setup succeeds");

    assert_eq!(
        rx.try_recv().expect("new session must send SetModel"),
        "opencode-go:claude-opus-5-5"
    );
    let state = session.ui_state.lock().await.clone();
    assert_eq!(state.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(state.provider_name.as_deref(), Some("Claude"));
    drop(session);
    daemon.await.unwrap();
}

#[test]
fn acp_tool_kind_maps_core_tools() {
    assert_eq!(tool_kind("read"), "read");
    assert_eq!(tool_kind("apply_patch"), "edit");
    assert_eq!(tool_kind("bash"), "execute");
    assert_eq!(tool_kind("agentgrep"), "search");
    assert_eq!(tool_kind("webfetch"), "fetch");
    assert_eq!(tool_kind("swarm"), "other");
}

#[test]
fn json_rpc_parse_errors_use_standard_codes() {
    let (code, _) = JsonRpcMessage::parse("not json").unwrap_err();
    assert_eq!(code, JSONRPC_PARSE_ERROR);

    let (code, message) = JsonRpcMessage::parse(r#"{"method":"initialize"}"#).unwrap_err();
    assert_eq!(code, JSONRPC_INVALID_REQUEST);
    assert!(message.contains("jsonrpc"));
}

#[test]
fn prompt_from_params_accepts_text_images_and_resources() {
    let params = json!({
        "sessionId": "s1",
        "prompt": [
            {"type": "text", "text": "hello"},
            {"type": "image", "mimeType": "image/png", "data": "abc"},
            {"type": "resource", "resource": {"uri": "file:///tmp/a.rs", "text": "fn main(){}"}},
            {"type": "resource_link", "uri": "file:///tmp/b.rs", "name": "b.rs"}
        ]
    });
    let (text, images) = prompt_from_params(&params).unwrap();
    assert!(text.contains("hello"));
    assert!(text.contains("Embedded resource: file:///tmp/a.rs"));
    assert!(text.contains("Resource link: b.rs"));
    assert_eq!(images, vec![("image/png".to_string(), "abc".to_string())]);
}

#[test]
fn prompt_usage_deduplicates_live_snapshots_and_resets_per_call() {
    let mut usage = TurnUsage::default();
    usage.observe_snapshot(10, 2, Some(4), None);
    usage.observe_snapshot(10, 3, Some(4), Some(5));
    usage.observe_snapshot(10, 3, Some(4), Some(5));
    // Stale or partial snapshots must not lower the per-call watermark.
    usage.observe_snapshot(8, 1, Some(2), None);
    usage.observe_snapshot(10, 3, Some(4), Some(5));
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 3);
    assert_eq!(usage.cached_read_tokens, Some(4));
    assert_eq!(usage.cached_write_tokens, Some(5));
    usage.begin_call();
    usage.observe_snapshot(20, 3, Some(6), Some(7));
    assert_eq!(usage.input_tokens, 30);
    assert_eq!(usage.output_tokens, 6);
    assert_eq!(usage.cached_read_tokens, Some(10));
    assert_eq!(usage.cached_write_tokens, Some(12));
}

#[test]
fn prompt_response_reports_usage_accumulated_across_the_turn() {
    let mut usage = TurnUsage::default();
    usage.add(10, 2, Some(4), Some(5));
    usage.add(20, 3, Some(6), Some(7));

    assert_eq!(
        prompt_response("end_turn", &usage),
        json!({
            "stopReason": "end_turn",
            "usage": {
                "totalTokens": 57,
                "inputTokens": 30,
                "outputTokens": 5,
                "cachedReadTokens": 10,
                "cachedWriteTokens": 12,
            }
        })
    );
}

#[test]
fn prompt_response_omits_unreported_usage_and_cache_fields() {
    assert_eq!(
        prompt_response("end_turn", &TurnUsage::default()),
        json!({ "stopReason": "end_turn" })
    );

    let mut usage = TurnUsage::default();
    usage.add(10, 2, None, None);
    assert_eq!(
        prompt_response("cancelled", &usage),
        json!({
            "stopReason": "cancelled",
            "usage": {
                "totalTokens": 12,
                "inputTokens": 10,
                "outputTokens": 2,
            }
        })
    );
}

#[test]
fn initialize_standard_omits_jcode_meta() {
    let result = initialize_result(&json!({"protocolVersion": 1}), AcpProfile::Standard);
    assert_eq!(result["protocolVersion"], 1);
    assert!(result["agentCapabilities"].get("_meta").is_none());
    assert_eq!(result["agentCapabilities"]["loadSession"], true);
}

#[test]
fn initialize_full_advertises_jcode_extension_meta() {
    let result = initialize_result(&json!({"protocolVersion": 1}), AcpProfile::Full);
    assert_eq!(
        result["agentCapabilities"]["_meta"]["jcode"]["profile"],
        "full"
    );
}

#[test]
fn event_mapper_maps_tool_lifecycle() {
    let mut mapper = EventMapper::new("session1".to_string(), AcpProfile::Standard);
    let start = mapper.map_event(ServerEvent::ToolStart {
        id: "tool1".to_string(),
        name: "bash".to_string(),
    });
    assert_eq!(start[0]["sessionUpdate"], "tool_call");
    assert_eq!(start[0]["kind"], "execute");

    let input = mapper.map_event(ServerEvent::ToolInput {
        id: None,
        delta: "{\"command\":\"true\"}".to_string(),
    });
    assert_eq!(input[0]["rawInput"]["command"], "true");

    let done = mapper.map_event(ServerEvent::ToolDone {
        id: "tool1".to_string(),
        name: "bash".to_string(),
        output: "ok".to_string(),
        error: None,
    });
    assert_eq!(done[0]["status"], "completed");
    assert_eq!(done[0]["content"][0]["content"]["text"], "ok");
}

#[test]
fn event_mapper_keeps_overlapping_tool_inputs_on_their_call() {
    let mut mapper = EventMapper::new("session1".to_string(), AcpProfile::Standard);
    for id in ["a", "b"] {
        mapper.map_event(ServerEvent::ToolStart {
            id: id.to_string(),
            name: "bash".to_string(),
        });
        assert!(
            mapper
                .map_event(ServerEvent::ToolInput {
                    id: Some(id.to_string()),
                    delta: "{\"command\":".to_string(),
                })
                .is_empty()
        );
    }
    for (id, command) in [("a", "true"), ("b", "false")] {
        let updates = mapper.map_event(ServerEvent::ToolInput {
            id: Some(id.to_string()),
            delta: format!("\"{command}\"}}"),
        });
        assert_eq!(
            updates,
            vec![json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": id,
                "rawInput": {"command": command},
            })]
        );
    }
    for (id, command) in [("a", "true"), ("b", "false")] {
        let updates = mapper.map_event(ServerEvent::ToolExec {
            id: id.to_string(),
            name: "bash".to_string(),
        });
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0]["toolCallId"], id);
        assert_eq!(updates[0]["status"], "in_progress");
        assert_eq!(updates[0]["rawInput"], json!({"command": command}));
    }
}

#[test]
fn event_mapper_suppresses_incomplete_legacy_tool_input() {
    let mut mapper = EventMapper::new("session1".to_string(), AcpProfile::Standard);
    assert!(
        mapper
            .map_event(ServerEvent::ToolInput {
                id: None,
                delta: "ignored without a tool".to_string(),
            })
            .is_empty()
    );
    mapper.map_event(ServerEvent::ToolStart {
        id: "legacy".to_string(),
        name: "bash".to_string(),
    });
    for delta in ["", "{", "\"command\":"] {
        assert!(
            mapper
                .map_event(ServerEvent::ToolInput {
                    id: None,
                    delta: delta.to_string(),
                })
                .is_empty()
        );
    }
    let updates = mapper.map_event(ServerEvent::ToolInput {
        id: None,
        delta: "\"true\"}".to_string(),
    });
    assert_eq!(
        updates,
        vec![json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "legacy",
            "rawInput": {"command": "true"},
        })]
    );
}

#[test]
fn non_empty_mcp_servers_are_tolerated_until_session_scoped_mcp_is_supported() {
    let params = json!({"mcpServers": [{"name": "fs"}]});
    assert!(validate_acp_mcp_servers(&params).is_ok());

    let params = json!({"mcpServers": []});
    assert!(validate_acp_mcp_servers(&params).is_ok());
}

#[test]
fn advertised_commands_cover_all_acp_daemon_model_controls() {
    let commands = acp_available_commands();
    let names: Vec<&str> = commands
        .iter()
        .map(|command| command["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["model", "models", "effort"]);
    assert_eq!(commands[0]["input"]["hint"], "model id (optional)");
    assert!(commands[1].get("input").is_none());
    assert!(
        commands[2]["input"]["hint"]
            .as_str()
            .unwrap()
            .contains("high")
    );
}

#[test]
fn advertised_commands_parse_to_real_dispatch_variants() {
    assert_eq!(
        parse_acp_slash_command("/model claude-sonnet-4-5")
            .unwrap()
            .unwrap(),
        AcpSlashCommand::Model(Some("claude-sonnet-4-5".to_string()))
    );
    assert_eq!(
        parse_acp_slash_command("/model ").unwrap().unwrap(),
        AcpSlashCommand::Model(None)
    );
    assert_eq!(
        parse_acp_slash_command("/models").unwrap().unwrap(),
        AcpSlashCommand::Models
    );
    assert_eq!(
        parse_acp_slash_command("/effort xhigh").unwrap().unwrap(),
        AcpSlashCommand::Effort(Some("xhigh".to_string()))
    );
    assert!(parse_acp_slash_command("/models now").unwrap().is_err());
    assert!(parse_acp_slash_command("/not-advertised").is_none());
    assert!(parse_acp_slash_command(" /model literal").is_none());
    assert!(parse_acp_slash_command("ordinary prompt").is_none());
}

fn project_skills(names: &[&str]) -> (tempfile::TempDir, crate::skill::SkillRegistry) {
    let dir = tempfile::tempdir().unwrap();
    for name in names {
        let skill_dir = dir.path().join(".jcode/skills").join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test {name}\n---\nDo {name}.\n"),
        )
        .unwrap();
    }
    let registry = crate::skill::SkillRegistry::load_project_overlay(Some(dir.path())).unwrap();
    (dir, registry)
}

#[test]
fn slash_and_dollar_skill_invocations_resolve_with_trailing_prompt() {
    let (_dir, skills) = project_skills(&["interrogate"]);
    assert_eq!(
        resolve_acp_skill_invocation(&skills, "/interrogate"),
        Some(("interrogate".to_string(), None))
    );
    assert_eq!(
        resolve_acp_skill_invocation(&skills, "/interrogate the auth flow\n"),
        Some(("interrogate".to_string(), Some("the auth flow".to_string())))
    );
    assert_eq!(
        resolve_acp_skill_invocation(&skills, "$interrogate why"),
        Some(("interrogate".to_string(), Some("why".to_string())))
    );
}

#[test]
fn unknown_escaped_and_path_like_slash_text_is_untouched() {
    let (_dir, skills) = project_skills(&["interrogate"]);
    for text in [
        "/unknown do things",
        "/home/user/file.rs explain",
        " /interrogate escaped",
        "please /interrogate",
        "$HOME is set",
        "ordinary prompt",
    ] {
        assert_eq!(resolve_acp_skill_invocation(&skills, text), None, "{text}");
    }
}

#[test]
fn explicit_skill_metadata_is_read_from_prompt_meta() {
    assert_eq!(
        explicit_skill_from_params(&json!({"_meta": {"jcode": {"skill": "/interrogate"}}})),
        Some("interrogate".to_string())
    );
    assert_eq!(
        explicit_skill_from_params(&json!({"_meta": {"activeSkill": "$review"}})),
        Some("review".to_string())
    );
    assert_eq!(
        explicit_skill_from_params(&json!({"_meta": {"skill": " "}})),
        None
    );
    assert_eq!(explicit_skill_from_params(&json!({})), None);
}

#[test]
fn skill_commands_advertise_installed_skills_without_shadowing_builtins() {
    let (_dir, skills) = project_skills(&["interrogate", "model"]);
    let commands = acp_skill_commands(&skills, &std::collections::HashSet::new());
    let names: Vec<&str> = commands
        .iter()
        .map(|command| command["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["interrogate"]);
    assert_eq!(commands[0]["description"], "Test interrogate");

    // A prompt template of the same name shadows the skill in the picker, just
    // like it does at submit time.
    let shadowed = std::collections::HashSet::from(["interrogate"]);
    assert!(acp_skill_commands(&skills, &shadowed).is_empty());
}

#[test]
fn prompt_template_commands_advertise_templates_without_shadowing_builtins() {
    let dir = tempfile::tempdir().unwrap();
    let prompts = dir.path().join(".jcode").join("prompts");
    std::fs::create_dir_all(&prompts).unwrap();
    std::fs::write(
        prompts.join("review.md"),
        "---\ndescription: Review staged changes\nargument-hint: \"[focus]\"\n---\nReview.",
    )
    .unwrap();
    std::fs::write(prompts.join("model.md"), "Shadowed by the built-in.").unwrap();
    let templates = crate::prompt_template::PromptTemplateRegistry::load_from_roots(
        Some(dir.path()),
        None,
        None,
    );
    let commands = acp_prompt_template_commands(&templates);
    assert_eq!(commands.len(), 1, "{commands:?}");
    assert_eq!(commands[0]["name"], "review");
    assert_eq!(commands[0]["description"], "Review staged changes");
    assert_eq!(commands[0]["input"]["hint"], "[focus]");
}

#[test]
fn compatibility_methods_accept_host_field_names_and_aliases() {
    assert_eq!(
        compatibility_option_value(
            &json!({"modelId": "deepseek-v4-flash"}),
            &["modelId", "model"],
            "session/set_model"
        )
        .unwrap(),
        "deepseek-v4-flash"
    );
    assert_eq!(
        compatibility_option_value(
            &json!({"reasoningEffort": "high"}),
            &["effort", "reasoningEffort"],
            "session/set_reasoning_effort"
        )
        .unwrap(),
        "high"
    );
    assert!(
        compatibility_option_value(
            &json!({"effort": ""}),
            &["effort", "reasoningEffort"],
            "session/set_reasoning_effort"
        )
        .unwrap_err()
        .contains("non-empty")
    );
}

#[test]
fn cwd_must_be_absolute() {
    let params = json!({"cwd": "relative"});
    assert!(cwd_from_params(&params).is_err());
    let params = json!({"cwd": "/tmp"});
    assert_eq!(cwd_from_params(&params).unwrap(), Path::new("/tmp"));
}

#[test]
fn config_options_include_model_selector_and_effort_ladder() {
    let state = SessionUiState {
        provider_name: Some("openai".to_string()),
        model: Some("gpt-5.2".to_string()),
        available_models: vec!["gpt-5.2".to_string(), "gpt-5.2-codex".to_string()],
        reasoning_effort: Some("high".to_string()),
    };
    let options = session_config_options(&state);
    assert_eq!(options.len(), 2);

    let model = &options[0];
    assert_eq!(model["id"], CONFIG_ID_MODEL);
    assert_eq!(model["category"], "model");
    assert_eq!(model["type"], "select");
    assert_eq!(model["currentValue"], "gpt-5.2");
    assert_eq!(model["options"].as_array().unwrap().len(), 2);

    let effort = &options[1];
    assert_eq!(effort["id"], CONFIG_ID_EFFORT);
    assert_eq!(effort["category"], "thought_level");
    assert_eq!(effort["currentValue"], "high");
    let effort_values: Vec<&str> = effort["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|option| option["value"].as_str().unwrap())
        .collect();
    assert!(effort_values.contains(&"medium"));
    assert!(
        !effort_values.iter().any(|value| value.starts_with("swarm")),
        "swarm sentinels are TUI-only and must not leak over ACP: {effort_values:?}"
    );
}

#[test]
fn config_options_current_model_prepended_when_not_listed() {
    let state = SessionUiState {
        provider_name: Some("anthropic".to_string()),
        model: Some("claude-opus-4-6".to_string()),
        available_models: vec!["claude-sonnet-4-5".to_string()],
        reasoning_effort: None,
    };
    let options = session_config_options(&state);
    let model_values: Vec<&str> = options[0]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|option| option["value"].as_str().unwrap())
        .collect();
    assert_eq!(model_values[0], "claude-opus-4-6");
    assert!(model_values.contains(&"claude-sonnet-4-5"));
}

#[test]
fn legacy_models_catalog_is_emitted_alongside_config_options() {
    let state = SessionUiState {
        provider_name: Some("deepseek".to_string()),
        model: Some("deepseek-v4-flash".to_string()),
        available_models: vec!["deepseek-v4-pro".to_string()],
        reasoning_effort: Some("high".to_string()),
    };
    let mut result = json!({"sessionId": "s1"});
    insert_session_configuration(&mut result, &state);

    assert!(result["configOptions"].is_array());
    assert_eq!(result["models"]["currentModelId"], "deepseek-v4-flash");
    let ids: Vec<&str> = result["models"]["availableModels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["modelId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["deepseek-v4-flash", "deepseek-v4-pro"]);
}

#[test]
fn config_options_empty_without_model_state() {
    let options = session_config_options(&SessionUiState::default());
    assert!(options.is_empty());
}

#[test]
fn context_limit_falls_back_to_default_for_unknown_models() {
    let state = SessionUiState {
        provider_name: Some("mystery".to_string()),
        model: Some("mystery-model-9000".to_string()),
        available_models: Vec::new(),
        reasoning_effort: None,
    };
    assert_eq!(
        state.context_limit(),
        crate::provider::DEFAULT_CONTEXT_LIMIT as u64
    );
}
