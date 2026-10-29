//! Project-defined tools from `.jcode/tools.json`.
//!
//! jcode ships two ways to add tools: MCP servers and SDK client-executed
//! tools. Neither lets a repository declare a small local command as a
//! first-class tool with its own name. This module does: every session loads
//! `~/.jcode/tools.json` and then `<working_dir>/.jcode/tools.json`, and each
//! entry becomes an ordinary tool.
//!
//! ```json
//! {
//!   "tools": [
//!     {
//!       "name": "feature_list",
//!       "description": "List alpha features.",
//!       "command": ["scripts/feature-workflow/feature-list"],
//!       "parameters": {"type": "object", "properties": {}},
//!       "timeout_secs": 120,
//!       "cwd": "project"
//!     }
//!   ]
//! }
//! ```
//!
//! The tool input JSON is written to the command's stdin and its output is the
//! tool result. `command` is an argv array (no shell); a relative first element
//! resolves against the project root (the parent of `.jcode`), so a symlinked
//! `.jcode/tools.json` runs commands from the project that owns the file.
//! `cwd` is `project` (default) or `session`. Invalid entries are skipped with
//! a warning: a broken tools file must never prevent a session from starting.

use super::{Tool, ToolContext, ToolOutput};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 3600;
const MAX_RESULT_BYTES: usize = 128 * 1024;
const MAX_NAME_LEN: usize = 64;
/// Generic flag jcode adds to every tool schema; the command should not see it.
const ACCEPT_LARGE_OUTPUT: &str = "accept_large_output";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum CwdMode {
    #[default]
    Project,
    Session,
}

#[derive(Debug, Deserialize)]
struct ToolsFile {
    #[serde(default)]
    tools: Vec<ToolEntry>,
}

#[derive(Debug, Deserialize)]
struct ToolEntry {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    command: Vec<String>,
    #[serde(default)]
    parameters: Option<Value>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    cwd: CwdMode,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectToolSpec {
    name: String,
    description: String,
    command: Vec<String>,
    parameters: Value,
    timeout: Duration,
    cwd: CwdMode,
    env: BTreeMap<String, String>,
    /// Project root that owns the config file; relative commands and the
    /// default working directory resolve against it.
    base_dir: PathBuf,
}

impl ProjectToolSpec {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    fn validate(entry: ToolEntry, base_dir: &Path) -> Result<Self> {
        let name = entry.name.trim().to_string();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            bail!("tool name must be 1..={MAX_NAME_LEN} characters");
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            bail!("tool name '{name}' may only contain letters, digits, '_' and '-'");
        }
        let description = entry.description.trim().to_string();
        if description.is_empty() {
            bail!("tool '{name}' has an empty description");
        }
        if entry.command.is_empty() {
            bail!("tool '{name}' has no command");
        }
        let parameters = match entry.parameters {
            Some(Value::Object(map)) => {
                if map.get("type").and_then(Value::as_str) != Some("object") {
                    bail!("tool '{name}' parameters must have \"type\": \"object\"");
                }
                Value::Object(map)
            }
            Some(_) => bail!("tool '{name}' parameters must be a JSON object schema"),
            None => json!({"type": "object", "properties": {}}),
        };
        let timeout_secs = entry
            .timeout_secs
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(1, MAX_TIMEOUT_SECS);
        Ok(Self {
            name,
            description,
            command: entry.command,
            parameters,
            timeout: Duration::from_secs(timeout_secs),
            cwd: entry.cwd,
            env: entry.env,
            base_dir: base_dir.to_path_buf(),
        })
    }

    fn program(&self) -> PathBuf {
        let first = Path::new(&self.command[0]);
        if first.is_absolute() {
            return first.to_path_buf();
        }
        let local = self.base_dir.join(first);
        if first.components().count() > 1 || local.exists() {
            local
        } else {
            first.to_path_buf()
        }
    }

    fn working_dir(&self, ctx: &ToolContext) -> PathBuf {
        match self.cwd {
            CwdMode::Session => ctx
                .working_dir
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| self.base_dir.clone()),
            CwdMode::Project => self.base_dir.clone(),
        }
    }
}

/// Load global then project tool definitions. Project entries win on a name
/// clash. A missing or malformed file never fails the session.
pub(crate) fn load_specs(working_dir: Option<&Path>) -> Vec<ProjectToolSpec> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".jcode").join("tools.json"));
    }
    let project_root = match working_dir {
        Some(dir) => Some(dir.to_path_buf()),
        None => std::env::current_dir().ok(),
    };
    if let Some(root) = project_root {
        paths.push(root.join(".jcode").join("tools.json"));
    }

    let mut merged: Vec<ProjectToolSpec> = Vec::new();
    for path in paths {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Resolve the real path so a symlinked config runs its commands from
        // the project that owns the file, not the link's location. The base is
        // the project root: `<root>/.jcode/tools.json` -> `<root>`.
        let real = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let base_dir = real
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let file: ToolsFile = match serde_json::from_str(&raw) {
            Ok(file) => file,
            Err(err) => {
                crate::logging::warn(&format!(
                    "project tools: ignoring {}: {err}",
                    path.display()
                ));
                continue;
            }
        };
        for entry in file.tools {
            match ProjectToolSpec::validate(entry, &base_dir) {
                Ok(spec) => merge_spec(&mut merged, spec),
                Err(err) => crate::logging::warn(&format!(
                    "project tools: skipping an entry in {}: {err}",
                    path.display()
                )),
            }
        }
    }
    merged
}

/// Project files are read after the global one, so a later spec replaces an
/// earlier one with the same name.
fn merge_spec(specs: &mut Vec<ProjectToolSpec>, spec: ProjectToolSpec) {
    if let Some(existing) = specs.iter_mut().find(|s| s.name == spec.name) {
        *existing = spec;
    } else {
        specs.push(spec);
    }
}

pub(crate) struct ProjectTool {
    spec: ProjectToolSpec,
}

impl ProjectTool {
    pub(crate) fn new(spec: ProjectToolSpec) -> Self {
        Self { spec }
    }
}

fn cap_output(mut output: String) -> String {
    if output.len() <= MAX_RESULT_BYTES {
        return output;
    }
    let mut end = MAX_RESULT_BYTES;
    while end > 0 && !output.is_char_boundary(end) {
        end -= 1;
    }
    let omitted = output.len() - end;
    output.truncate(end);
    output.push_str(&format!("\n[truncated {omitted} bytes]"));
    output
}

#[async_trait]
impl Tool for ProjectTool {
    fn name(&self) -> &str {
        &self.spec.name
    }

    fn description(&self) -> &str {
        &self.spec.description
    }

    fn parameters_schema(&self) -> Value {
        self.spec.parameters.clone()
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let mut payload = match input {
            Value::Object(map) => Value::Object(map),
            _ => json!({}),
        };
        if let Value::Object(map) = &mut payload {
            map.remove(ACCEPT_LARGE_OUTPUT);
        }
        let mut stdin_json = payload.to_string();
        stdin_json.push('\n');

        let program = self.spec.program();
        let cwd = self.spec.working_dir(&ctx);
        let mut command = Command::new(&program);
        command
            .args(&self.spec.command[1..])
            .current_dir(&cwd)
            .envs(&self.spec.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn().with_context(|| {
            format!(
                "tool '{}' could not start {} (cwd {})",
                self.spec.name,
                program.display(),
                cwd.display()
            )
        })?;
        if let Some(mut stdin) = child.stdin.take() {
            // A command that never reads stdin must not fail the call; a pipe
            // error is reported by the process's own exit status.
            let _ = stdin.write_all(stdin_json.as_bytes()).await;
            drop(stdin);
        }

        let output = match tokio::time::timeout(self.spec.timeout, child.wait_with_output()).await {
            Ok(result) => result.context("failed to wait for the project tool")?,
            Err(_) => bail!(
                "tool '{}' timed out after {}s",
                self.spec.name,
                self.spec.timeout.as_secs()
            ),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut combined = stdout.trim_end().to_string();
        if !stderr.trim().is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str("[stderr]\n");
            combined.push_str(stderr.trim_end());
        }

        if !output.status.success() {
            let status = output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string());
            bail!(
                "tool '{}' exited with status {status}\n{}",
                self.spec.name,
                cap_output(combined)
            );
        }

        Ok(ToolOutput::new(cap_output(combined))
            .with_title(self.spec.name.clone())
            .with_metadata(json!({"command": self.spec.command, "cwd": cwd})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_tool_core::ToolExecutionMode;
    use std::fs;

    fn ctx(working_dir: Option<PathBuf>) -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            message_id: "test".into(),
            tool_call_id: "test".into(),
            working_dir,
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: ToolExecutionMode::AgentTurn,
        }
    }

    fn write_tools(dir: &Path, json: Value) {
        let path = dir.join(".jcode").join("tools.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, json.to_string()).unwrap();
    }

    #[test]
    fn loads_project_tools_and_skips_invalid_entries() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [
                {"name": "echo_json", "description": "echo", "command": ["/bin/sh", "-c", "cat"], "parameters": {"type": "object"}},
                {"name": "bad name", "description": "x", "command": ["/bin/true"]},
                {"name": "no_command", "description": "x"},
                {"name": "no_description", "command": ["/bin/true"]}
            ]}),
        );
        let specs = load_specs(Some(dir.path()));
        let names: Vec<_> = specs.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["echo_json"]);
        assert_eq!(specs[0].base_dir, fs::canonicalize(dir.path()).unwrap());
    }

    #[test]
    fn project_entry_overrides_global_name() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{"name": "x", "description": "project", "command": ["/bin/true"]}]}),
        );
        let mut specs = vec![ProjectToolSpec {
            name: "x".into(),
            description: "global".into(),
            command: vec!["/bin/true".into()],
            parameters: json!({"type": "object", "properties": {}}),
            timeout: Duration::from_secs(1),
            cwd: CwdMode::Project,
            env: BTreeMap::new(),
            base_dir: dir.path().to_path_buf(),
        }];
        for spec in load_specs(Some(dir.path())) {
            merge_spec(&mut specs, spec);
        }
        let x = specs.iter().filter(|s| s.name == "x").collect::<Vec<_>>();
        assert_eq!(x.len(), 1);
        assert_eq!(x[0].description, "project");
    }

    #[tokio::test]
    async fn passes_input_on_stdin_and_returns_stdout() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{"name": "echo_json", "description": "echo", "command": ["/bin/sh", "-c", "cat"], "timeout_secs": 5}]}),
        );
        let spec = load_specs(Some(dir.path())).remove(0);
        let tool = ProjectTool::new(spec);
        let out = tool
            .execute(json!({"hello": "world"}), ctx(Some(dir.path().to_path_buf())))
            .await
            .unwrap();
        assert_eq!(out.output.trim(), r#"{"hello":"world"}"#);
    }

    #[tokio::test]
    async fn nonzero_exit_is_an_error_with_output() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{"name": "fail", "description": "fail", "command": ["/bin/sh", "-c", "echo nope >&2; exit 3"], "timeout_secs": 5}]}),
        );
        let spec = load_specs(Some(dir.path())).remove(0);
        let err = ProjectTool::new(spec)
            .execute(json!({}), ctx(Some(dir.path().to_path_buf())))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("status 3"), "{err}");
        assert!(err.contains("nope"), "{err}");
    }

    #[tokio::test]
    async fn timeout_kills_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{"name": "slow", "description": "slow", "command": ["/bin/sh", "-c", "sleep 5"], "timeout_secs": 1}]}),
        );
        let spec = load_specs(Some(dir.path())).remove(0);
        let start = std::time::Instant::now();
        let err = ProjectTool::new(spec)
            .execute(json!({}), ctx(Some(dir.path().to_path_buf())))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("timed out"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(4));
    }

    #[tokio::test]
    async fn cwd_mode_session_runs_in_the_session_dir() {
        let project = tempfile::tempdir().unwrap();
        let session = tempfile::tempdir().unwrap();
        write_tools(
            project.path(),
            json!({"tools": [{"name": "pwd", "description": "pwd", "command": ["/bin/pwd"], "cwd": "session", "timeout_secs": 5}]}),
        );
        let spec = load_specs(Some(project.path())).remove(0);
        let out = ProjectTool::new(spec)
            .execute(json!({}), ctx(Some(session.path().to_path_buf())))
            .await
            .unwrap();
        let expected = fs::canonicalize(session.path()).unwrap();
        assert_eq!(out.output.trim(), expected.to_string_lossy());
    }

    #[test]
    fn relative_command_resolves_against_the_real_config_dir() {
        let owner = tempfile::tempdir().unwrap();
        write_tools(
            owner.path(),
            json!({"tools": [{"name": "run", "description": "run", "command": ["scripts/run.sh"]}]}),
        );
        let specs = load_specs(Some(owner.path()));
        let expected = fs::canonicalize(owner.path()).unwrap().join("scripts/run.sh");
        assert_eq!(specs[0].program(), expected);
    }
}
