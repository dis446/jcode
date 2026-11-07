//! Fork C19: a bare native model id must leave a catalog profile slot.
//!
//! Repro: the active slot runs OpenCode Go and an ACP host calls
//! `session/set_model claude-opus-5-5`. Upstream's "custom endpoint owns
//! opaque ids" branch kept the id on the slot, then rebound the slot to real
//! OpenRouter with a bare Claude id, so the switch succeeded and the first
//! turn failed with OpenRouter's "not a valid model ID".

use super::*;

fn opencode_go_multi_provider(
    anthropic: Option<Arc<dyn Provider>>,
) -> MultiProvider {
    let profile = crate::provider_catalog::OPENCODE_GO_PROFILE;
    crate::env::set_var(profile.api_key_env, "test-opencode-go-key");
    crate::provider_catalog::force_apply_openai_compatible_profile_env(Some(profile));
    let runtime = test_openrouter_runtime().expect("OpenCode Go runtime should initialize");
    runtime
        .set_model("deepseek-v4-flash")
        .expect("initial OpenCode Go model should be selectable");
    let provider = test_multi_provider_with_openrouter(runtime);
    *provider
        .anthropic
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = anthropic;
    provider
}

fn active_compat_api_method(provider: &MultiProvider) -> Option<String> {
    provider
        .active_openrouter_execution_provider()
        .and_then(|runtime| runtime.direct_openai_compatible_route_parts())
        .map(|(_, api_method, _)| api_method)
}

#[test]
fn bare_claude_id_switches_from_catalog_profile_to_claude() {
    with_clean_provider_test_env(|| {
        let provider = opencode_go_multi_provider(Some(test_anthropic_runtime()));

        provider
            .set_model("claude-opus-4-8")
            .expect("a Claude id must route to the Claude runtime");

        assert_eq!(provider.active_provider(), ActiveProvider::Claude);
        assert_eq!(provider.model(), "claude-opus-4-8");
    });
}

#[test]
fn switching_back_to_catalog_model_still_reaches_opencode_go() {
    with_clean_provider_test_env(|| {
        let provider = opencode_go_multi_provider(Some(test_anthropic_runtime()));
        provider.set_model("claude-opus-4-8").expect("to Claude");

        provider
            .set_model("deepseek-v4-flash")
            .expect("an OpenCode Go id must route back to OpenCode Go");

        assert_eq!(provider.active_provider(), ActiveProvider::OpenRouter);
        assert_eq!(provider.model(), "deepseek-v4-flash");
        assert_eq!(
            active_compat_api_method(&provider),
            Some("openai-compatible:opencode-go".to_string())
        );
    });
}

#[test]
fn bare_claude_id_without_claude_login_fails_at_switch_time() {
    with_clean_provider_test_env(|| {
        let provider = opencode_go_multi_provider(None);

        let error = provider
            .set_model("claude-opus-4-8")
            .expect_err("no Claude runtime: the switch itself must fail");

        let message = format!("{error:#}");
        assert!(message.contains("Claude"), "unexpected error: {message}");
        assert!(!message.contains("openrouter.ai"), "{message}");
        assert_eq!(provider.model(), "deepseek-v4-flash");
        assert_eq!(
            active_compat_api_method(&provider),
            Some("openai-compatible:opencode-go".to_string())
        );
    });
}

#[test]
fn real_openrouter_rejects_bare_native_id_at_switch_time() {
    with_clean_provider_test_env(|| {
        with_env_var("OPENROUTER_API_KEY", "test-openrouter-key", || {
            let provider = opencode_go_multi_provider(None);

            let error = provider
                .set_model("openrouter:claude-opus-4-8")
                .expect_err("OpenRouter needs anthropic/<id>");

            let message = format!("{error:#}");
            assert!(
                message.contains("anthropic/claude-opus-4-8"),
                "unexpected error: {message}"
            );
            assert_eq!(provider.model(), "deepseek-v4-flash");
        })
    });
}

#[test]
fn generic_custom_endpoint_keeps_opaque_claude_like_ids() {
    // Upstream behaviour preserved: only *catalog* profiles hand off ids they
    // do not serve. A generic endpoint owns its ids.
    with_clean_provider_test_env(|| {
        with_env_var("OPENROUTER_API_KEY", "test-openrouter-key", || {
            with_env_var("JCODE_OPENROUTER_PROVIDER_FEATURES", "0", || {
                with_env_var(
                    "JCODE_OPENROUTER_API_BASE",
                    "https://compat.example.test/v1",
                    || {
                        let runtime = test_openrouter_runtime()
                            .expect("custom compatible provider should initialize");
                        let provider = test_multi_provider_with_openrouter(runtime);
                        *provider
                            .anthropic
                            .write()
                            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                            Some(test_anthropic_runtime());

                        provider
                            .set_model("claude-opus-4-8")
                            .expect("generic endpoint keeps the id");

                        assert_eq!(provider.active_provider(), ActiveProvider::OpenRouter);
                        assert_eq!(provider.model(), "claude-opus-4-8");
                    },
                )
            })
        })
    });
}
