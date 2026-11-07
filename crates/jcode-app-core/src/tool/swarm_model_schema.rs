use super::*;

pub(super) fn apply_swarm_model_policy(schema: &mut Value, agents: &crate::config::AgentsConfig) {
    let Some(properties) = schema["properties"].as_object_mut() else {
        crate::logging::error(
            "Cannot apply swarm model policy: schema properties are not an object",
        );
        return;
    };
    if !agents.swarm_model_selection || agents.swarm_model_allowlist.is_empty() {
        properties.remove("model");
    } else {
        properties.insert(
            "model".to_string(),
            json!({
                "type": "string",
                "enum": agents.swarm_model_allowlist,
                "description": "Exact user-allowlisted route; omit for configured default."
            }),
        );
    }
}

/// Effort is a separate opt-in from model selection: a user may allow exact
/// model routes while still owning the worker effort budget, or the reverse.
/// Keep the default schema free of the field so the coordinating agent cannot
/// discover a knob that would be rejected at spawn time.
pub(super) fn apply_swarm_effort_policy(schema: &mut Value, agents: &crate::config::AgentsConfig) {
    let Some(properties) = schema["properties"].as_object_mut() else {
        crate::logging::error(
            "Cannot apply swarm effort policy: schema properties are not an object",
        );
        return;
    };
    if !agents.swarm_effort_selection {
        properties.remove("effort");
        return;
    }
    properties.insert(
        "effort".to_string(),
        json!({
            "type": "string",
            "enum": crate::config::SWARM_EFFORT_LEVELS,
            "description": "Reasoning effort for a newly spawned worker; omit for the configured default."
        }),
    );
}
