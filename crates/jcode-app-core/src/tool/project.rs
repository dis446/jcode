//! Project-defined tools from `.jcode/tools.json`.
//!
//! jcode ships two ways to add tools: MCP servers and SDK client-executed
//! tools. Neither lets a repository declare a small local command as a
//! first-class tool with its own name. This module does: every session loads
//! `$JCODE_HOME/tools.json` (normally `~/.jcode/tools.json`) and then
//! `<working_dir>/.jcode/tools.json`, and each entry becomes an ordinary tool.
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
//! tool result. `command` is an argv array; jcode does not add a shell, though a
//! declared command may itself be one. A relative first element resolves against
//! the config file's root, so a symlinked `.jcode/tools.json` runs commands from
//! the project that owns the file. `cwd` is `project` or `session`; the default
//! is `session` for the global file and `project` for a repo file. Invalid
//! entries are skipped with a warning: a broken tools file must never prevent a
//! session from starting.
//!
//! Project tools are replaced on every session attach, so editing or deleting
//! the files takes effect on the next attach.

use super::{Tool, ToolContext, ToolOutput};
use crate::project_config::{ConfigOwner, config_base_dir, read_config_file};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 3600;
const MAX_RESULT_BYTES: usize = 128 * 1024;
const READ_CHUNK_BYTES: usize = 8 * 1024;
const MAX_NAME_LEN: usize = 64;
const MAX_TOOLS: usize = 128;
const MAX_DESCRIPTION_BYTES: usize = 4 * 1024;
const MAX_PARAMETERS_BYTES: usize = 32 * 1024;
/// Generic flags jcode adds to every tool schema; the command should not see
/// them. `intent` is stripped only when the declared schema does not own it.
const ACCEPT_LARGE_OUTPUT: &str = "accept_large_output";
const INTENT: &str = "intent";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum CwdMode {
    #[default]
    Project,
    Session,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsFile {
    #[serde(default)]
    tools: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
    cwd: Option<CwdMode>,
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
    /// Root of the config file that owns this entry (the project root for a
    /// repo file, the jcode home for the global file).
    base_dir: PathBuf,
}

impl ProjectToolSpec {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    fn validate(entry: ToolEntry, base_dir: &Path, default_cwd: CwdMode) -> Result<Self> {
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
        if description.len() > MAX_DESCRIPTION_BYTES {
            bail!(
                "tool '{name}' description exceeds {MAX_DESCRIPTION_BYTES} bytes"
            );
        }
        if entry.command.is_empty() {
            bail!("tool '{name}' has no command");
        }
        let parameters = match entry.parameters {
            Some(Value::Object(map)) => {
                if map.get("type").and_then(Value::as_str) != Some("object") {
                    bail!("tool '{name}' parameters must have \"type\": \"object\"");
                }
                if let Some(properties) = map.get("properties")
                    && !properties.is_object()
                {
                    bail!("tool '{name}' parameters.properties must be a JSON object");
                }
                if let Some(required) = map.get("required") {
                    let Some(required) = required.as_array() else {
                        bail!("tool '{name}' parameters.required must be an array");
                    };
                    if !required.iter().all(Value::is_string) {
                        bail!(
                            "tool '{name}' parameters.required entries must be strings"
                        );
                    }
                }
                Value::Object(map)
            }
            Some(_) => bail!("tool '{name}' parameters must be a JSON object schema"),
            None => json!({"type": "object", "properties": {}}),
        };
        if parameters.to_string().len() > MAX_PARAMETERS_BYTES {
            bail!(
                "tool '{name}' parameters exceed {MAX_PARAMETERS_BYTES} bytes"
            );
        }
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
            cwd: entry.cwd.unwrap_or(default_cwd),
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

    fn declares_intent(&self) -> bool {
        self.parameters
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(|properties| properties.contains_key(INTENT))
    }
}

/// The global project-tools file. Uses jcode's home resolver so
/// `JCODE_HOME` isolates sandboxed runs instead of leaking the real user file.
pub(crate) fn global_tools_path() -> Option<PathBuf> {
    crate::storage::jcode_dir()
        .ok()
        .map(|dir| dir.join("tools.json"))
}

/// Read one tools file without following special files or unbounded growth.
/// A missing file is silent; every other skip is logged and returns `None`.
fn read_tools_config(path: &Path) -> Option<String> {
    read_config_file(path, "project tools")
}

/// Load global then project tool definitions. Project entries win on a name
/// clash. A missing or malformed file never fails the session. `global` is the
/// already-resolved global file, so tests never read the developer's real home.
pub(crate) fn load_specs_with_global(
    working_dir: Option<&Path>,
    global: Option<&Path>,
) -> Vec<ProjectToolSpec> {
    let mut sources: Vec<(PathBuf, CwdMode, ConfigOwner)> = Vec::new();
    if let Some(path) = global {
        // A global tool belongs to the user's home, but it usually acts on the
        // project the session is in, so session cwd is the useful default.
        sources.push((path.to_path_buf(), CwdMode::Session, ConfigOwner::Global));
    }
    if let Some(root) = working_dir {
        sources.push((
            root.join(".jcode").join("tools.json"),
            CwdMode::Project,
            ConfigOwner::Project,
        ));
    }

    let mut merged: Vec<ProjectToolSpec> = Vec::new();
    for (path, default_cwd, kind) in sources {
        let Some(raw) = read_tools_config(&path) else {
            continue;
        };
        // Resolve the real path so a symlinked config runs its commands from
        // the project that owns the file, not the link's location.
        let base_dir = config_base_dir(&path, kind);
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
        for (index, value) in file.tools.into_iter().enumerate() {
            let entry: ToolEntry = match serde_json::from_value(value) {
                Ok(entry) => entry,
                Err(err) => {
                    crate::logging::warn(&format!(
                        "project tools: skipping entry #{} in {}: {err}",
                        index + 1,
                        path.display()
                    ));
                    continue;
                }
            };
            match ProjectToolSpec::validate(entry, &base_dir, default_cwd) {
                Ok(spec) => {
                    if !merged.iter().any(|existing| existing.name == spec.name)
                        && merged.len() >= MAX_TOOLS
                    {
                        crate::logging::warn(&format!(
                            "project tools: ignoring '{}': limit is {MAX_TOOLS} tools",
                            spec.name
                        ));
                        continue;
                    }
                    merge_spec(&mut merged, spec);
                }
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

/// Read at most `cap` bytes, then keep draining the pipe so the child never
/// blocks on a full output buffer. Returns the kept bytes and how many were
/// discarded.
async fn read_capped<R>(reader: Option<R>, cap: usize) -> (Vec<u8>, usize)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(mut reader) = reader else {
        return (Vec::new(), 0);
    };
    let mut kept = Vec::with_capacity(cap.min(READ_CHUNK_BYTES));
    let mut omitted = 0usize;
    let mut buf = [0u8; READ_CHUNK_BYTES];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let room = cap.saturating_sub(kept.len());
                let take = room.min(n);
                kept.extend_from_slice(&buf[..take]);
                omitted += n - take;
            }
            Err(_) => break,
        }
    }
    (kept, omitted)
}

fn lossy_string(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Cap `output` at the result budget. `extra_omitted` counts bytes already
/// discarded by the pipe readers so the marker reports the real loss.
fn cap_output(mut output: String, extra_omitted: usize) -> String {
    if output.len() <= MAX_RESULT_BYTES && extra_omitted == 0 {
        return output;
    }
    let mut end = output.len().min(MAX_RESULT_BYTES);
    while end > 0 && !output.is_char_boundary(end) {
        end -= 1;
    }
    let omitted = extra_omitted + (output.len() - end);
    output.truncate(end);
    output.push_str(&format!("\n[truncated {omitted} bytes]"));
    output
}

/// Join the primary stream with the secondary stream. If the primary is empty
/// the secondary is either labeled (success output still promises `[stderr]`)
/// or returned bare (a stderr-only failure should read naturally).
fn compose_output(
    primary: String,
    secondary: String,
    secondary_label: &str,
    label_when_primary_empty: bool,
) -> String {
    let mut combined = primary.trim_end().to_string();
    let secondary = secondary.trim_end();
    if secondary.is_empty() {
        return combined;
    }
    if !combined.is_empty() {
        combined.push('\n');
    }
    if !combined.is_empty() || label_when_primary_empty {
        combined.push_str(secondary_label);
        combined.push('\n');
    }
    combined.push_str(secondary);
    combined
}

#[async_trait]
impl Tool for ProjectTool {
    fn name(&self) -> &str {
        &self.spec.name
    }

    fn is_project_tool(&self) -> bool {
        true
    }

    fn description(&self) -> &str {
        &self.spec.description
    }

    fn parameters_schema(&self) -> Value {
        self.spec.parameters.clone()
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let declares_intent = self.spec.declares_intent();
        let mut payload = match input {
            Value::Object(map) => Value::Object(map),
            _ => json!({}),
        };
        if let Value::Object(map) = &mut payload {
            map.remove(ACCEPT_LARGE_OUTPUT);
            if !declares_intent {
                map.remove(INTENT);
            }
        }
        let mut stdin_json = payload.to_string();
        stdin_json.push('\n');
        let stdin_json = stdin_json.into_bytes();

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

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let shutdown = ctx.graceful_shutdown_signal.clone();
        // The timeout covers writing stdin, reading both pipes, and waiting.
        // A child that never reads a large input therefore cannot hang the
        // call beyond `timeout_secs`.
        let exchange = async move {
            let write = async move {
                if let Some(mut stdin) = stdin {
                    // A command that never reads stdin must not fail the call;
                    // a pipe error is reported by the process's own exit status.
                    let _ = stdin.write_all(&stdin_json).await;
                }
            };
            let read_stdout = read_capped(stdout, MAX_RESULT_BYTES);
            let read_stderr = read_capped(stderr, MAX_RESULT_BYTES);
            let wait = async move { child.wait().await };
            let (_, (stdout, stdout_omitted), (stderr, stderr_omitted), status) =
                tokio::join!(write, read_stdout, read_stderr, wait);
            let status = status.context("failed to wait for the project tool")?;
            Ok::<_, anyhow::Error>((stdout, stdout_omitted, stderr, stderr_omitted, status))
        };

        let exchange_result = match shutdown {
            Some(signal) => {
                tokio::select! {
                    biased;
                    _ = signal.notified() => bail!(
                        "tool '{}' cancelled",
                        self.spec.name
                    ),
                    result = tokio::time::timeout(self.spec.timeout, exchange) => result,
                }
            }
            None => tokio::time::timeout(self.spec.timeout, exchange).await,
        };
        let (stdout, stdout_omitted, stderr, stderr_omitted, status) = match exchange_result {
            Ok(result) => result?,
            Err(_) => bail!(
                "tool '{}' timed out after {}s",
                self.spec.name,
                self.spec.timeout.as_secs()
            ),
        };

        let omitted = stdout_omitted + stderr_omitted;
        let stdout = lossy_string(stdout);
        let stderr = lossy_string(stderr);

        if !status.success() {
            let status = status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string());
            // Put stderr first: it usually carries the failure reason, and
            // stdout can fill the result cap on its own.
            let combined = compose_output(stderr, stdout, "[stdout]", false);
            bail!(
                "tool '{}' exited with status {status}\n{}",
                self.spec.name,
                cap_output(combined, omitted)
            );
        }

        let combined = compose_output(stdout, stderr, "[stderr]", true);
        Ok(ToolOutput::new(cap_output(combined, omitted))
            .with_title(self.spec.name.clone())
            .with_metadata(json!({"command": self.spec.command, "cwd": cwd})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_config::MAX_CONFIG_BYTES;
    use crate::tool::Registry;
    use async_trait::async_trait;
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

    fn write_tools_file(path: &Path, json: Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, json.to_string()).unwrap();
    }

    fn write_tools(dir: &Path, json: Value) {
        write_tools_file(&dir.join(".jcode").join("tools.json"), json);
    }

    fn names(specs: &[ProjectToolSpec]) -> Vec<&str> {
        specs.iter().map(|spec| spec.name.as_str()).collect()
    }

    #[test]
    fn loads_project_tools_and_skips_invalid_entries() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [
                {"name": "echo_json", "description": "echo", "command": ["echo"]},
                {"name": "bad name", "description": "x", "command": ["/bin/true"]},
                {"name": "no_command", "description": "x"},
                {"name": "no_description", "command": ["/bin/true"]},
                {"name": "bad_cwd", "description": "x", "command": ["/bin/true"], "cwd": "bogus"}
            ]}),
        );
        let specs = load_specs_with_global(Some(dir.path()), None);
        assert_eq!(names(&specs), vec!["echo_json"]);
        assert_eq!(specs[0].base_dir, fs::canonicalize(dir.path()).unwrap());
    }

    #[test]
    fn rejects_unknown_entry_and_file_fields() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [
                {"name": "typo", "description": "x", "command": ["/bin/true"], "timeout_sec": 5}
            ]}),
        );
        assert!(load_specs_with_global(Some(dir.path()), None).is_empty());

        write_tools(
            dir.path(),
            json!({"tool": [
                {"name": "typo", "description": "x", "command": ["/bin/true"]}
            ]}),
        );
        assert!(load_specs_with_global(Some(dir.path()), None).is_empty());
    }

    #[test]
    fn rejects_malformed_properties_and_required() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [
                {
                    "name": "bad_properties",
                    "description": "x",
                    "command": ["/bin/true"],
                    "parameters": {"type": "object", "properties": "nope"}
                },
                {
                    "name": "bad_required",
                    "description": "x",
                    "command": ["/bin/true"],
                    "parameters": {"type": "object", "properties": {}, "required": "nope"}
                },
                {
                    "name": "bad_required_entry",
                    "description": "x",
                    "command": ["/bin/true"],
                    "parameters": {"type": "object", "properties": {}, "required": [1]}
                }
            ]}),
        );
        assert!(load_specs_with_global(Some(dir.path()), None).is_empty());
    }

    #[test]
    fn rejects_oversized_description() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{
                "name": "long_description",
                "description": "x".repeat(MAX_DESCRIPTION_BYTES + 1),
                "command": ["/bin/true"]
            }]}),
        );
        assert!(load_specs_with_global(Some(dir.path()), None).is_empty());
    }

    #[test]
    fn ignores_oversized_config_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".jcode")).unwrap();
        fs::write(
            dir.path().join(".jcode").join("tools.json"),
            vec![b' '; (MAX_CONFIG_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(load_specs_with_global(Some(dir.path()), None).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn ignores_non_regular_config_file() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".jcode")).unwrap();
        symlink("/dev/null", dir.path().join(".jcode").join("tools.json")).unwrap();
        assert!(load_specs_with_global(Some(dir.path()), None).is_empty());
    }

    #[test]
    fn caps_the_number_of_tools() {
        let dir = tempfile::tempdir().unwrap();
        let tools: Vec<Value> = (0..MAX_TOOLS + 2)
            .map(|index| {
                json!({
                    "name": format!("tool_{index}"),
                    "description": "x",
                    "command": ["/bin/true"]
                })
            })
            .collect();
        write_tools(dir.path(), json!({"tools": tools}));
        assert_eq!(
            load_specs_with_global(Some(dir.path()), None).len(),
            MAX_TOOLS
        );
    }

    #[test]
    fn project_entry_overrides_global_name() {
        let global = tempfile::tempdir().unwrap();
        let global_path = global.path().join(".jcode").join("tools.json");
        write_tools_file(
            &global_path,
            json!({"tools": [{"name": "x", "description": "global", "command": ["/bin/true"]}]}),
        );
        let project = tempfile::tempdir().unwrap();
        write_tools(
            project.path(),
            json!({"tools": [{"name": "x", "description": "project", "command": ["/bin/true"]}]}),
        );
        let specs = load_specs_with_global(Some(project.path()), Some(&global_path));
        assert_eq!(names(&specs), vec!["x"]);
        assert_eq!(specs[0].description, "project");
        assert_eq!(specs[0].base_dir, fs::canonicalize(project.path()).unwrap());
    }

    #[test]
    fn without_working_dir_only_global_tools_load() {
        let global = tempfile::tempdir().unwrap();
        let global_path = global.path().join(".jcode").join("tools.json");
        write_tools_file(
            &global_path,
            json!({"tools": [{"name": "global_only", "description": "x", "command": ["/bin/true"]}]}),
        );
        let specs = load_specs_with_global(None, Some(&global_path));
        assert_eq!(names(&specs), vec!["global_only"]);
    }

    #[test]
    fn global_entry_defaults_to_session_cwd() {
        let global = tempfile::tempdir().unwrap();
        let global_path = global.path().join(".jcode").join("tools.json");
        write_tools_file(
            &global_path,
            json!({"tools": [{"name": "pwd", "description": "pwd", "command": ["/bin/pwd"]}]}),
        );
        let project = tempfile::tempdir().unwrap();
        let session = tempfile::tempdir().unwrap();
        let specs = load_specs_with_global(Some(project.path()), Some(&global_path));
        assert_eq!(
            specs[0].working_dir(&ctx(Some(session.path().to_path_buf()))),
            fs::canonicalize(session.path()).unwrap()
        );
    }

    #[test]
    fn repo_entry_defaults_to_project_cwd() {
        let project = tempfile::tempdir().unwrap();
        write_tools(
            project.path(),
            json!({"tools": [{"name": "pwd", "description": "pwd", "command": ["/bin/pwd"]}]}),
        );
        let session = tempfile::tempdir().unwrap();
        let specs = load_specs_with_global(Some(project.path()), None);
        assert_eq!(
            specs[0].working_dir(&ctx(Some(session.path().to_path_buf()))),
            fs::canonicalize(project.path()).unwrap()
        );
    }

    #[test]
    fn relative_command_resolves_against_the_real_config_dir() {
        let owner = tempfile::tempdir().unwrap();
        write_tools(
            owner.path(),
            json!({"tools": [{"name": "run", "description": "run", "command": ["scripts/run.sh"]}]}),
        );
        let specs = load_specs_with_global(Some(owner.path()), None);
        let expected = fs::canonicalize(owner.path())
            .unwrap()
            .join("scripts/run.sh");
        assert_eq!(specs[0].program(), expected);
    }

    #[test]
    fn global_relative_command_resolves_against_the_jcode_home() {
        let home = tempfile::tempdir().unwrap();
        let global_path = home.path().join("tools.json");
        write_tools_file(
            &global_path,
            json!({"tools": [{"name": "run", "description": "run", "command": ["scripts/run.sh"]}]}),
        );
        let specs = load_specs_with_global(None, Some(&global_path));
        let expected = fs::canonicalize(home.path())
            .unwrap()
            .join("scripts/run.sh");
        assert_eq!(specs[0].program(), expected);
    }

    #[test]
    fn global_tools_path_uses_jcode_home() {
        let _lock = crate::storage::lock_test_env();
        let home = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", home.path());
        let path = global_tools_path();
        match previous {
            Some(value) => crate::env::set_var("JCODE_HOME", value),
            None => crate::env::remove_var("JCODE_HOME"),
        }
        assert_eq!(path, Some(home.path().join("tools.json")));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_config_uses_the_owning_project_root() {
        use std::os::unix::fs::symlink;

        let owner = tempfile::tempdir().unwrap();
        write_tools(
            owner.path(),
            json!({"tools": [{"name": "run", "description": "run", "command": ["scripts/run.sh"]}]}),
        );
        let feature = tempfile::tempdir().unwrap();
        fs::create_dir_all(feature.path().join(".jcode")).unwrap();
        symlink(
            owner.path().join(".jcode").join("tools.json"),
            feature.path().join(".jcode").join("tools.json"),
        )
        .unwrap();

        let specs = load_specs_with_global(Some(feature.path()), None);
        let expected = fs::canonicalize(owner.path())
            .unwrap()
            .join("scripts/run.sh");
        assert_eq!(specs[0].base_dir, fs::canonicalize(owner.path()).unwrap());
        assert_eq!(specs[0].program(), expected);
    }

    #[tokio::test]
    async fn reload_replaces_project_tools_and_drops_removed_ones() {
        let registry = Registry::empty();
        let first = tempfile::tempdir().unwrap();
        write_tools(
            first.path(),
            json!({"tools": [{"name": "alpha", "description": "x", "command": ["/bin/true"]}]}),
        );
        registry
            .register_project_tools_with_global(Some(first.path().to_path_buf()), None)
            .await;
        assert_eq!(registry.tool_names().await, vec!["alpha"]);

        write_tools(
            first.path(),
            json!({"tools": [{"name": "beta", "description": "x", "command": ["/bin/true"]}]}),
        );
        registry
            .register_project_tools_with_global(Some(first.path().to_path_buf()), None)
            .await;
        assert_eq!(registry.tool_names().await, vec!["beta"]);

        let second = tempfile::tempdir().unwrap();
        write_tools(
            second.path(),
            json!({"tools": [{"name": "gamma", "description": "x", "command": ["/bin/true"]}]}),
        );
        registry
            .register_project_tools_with_global(Some(second.path().to_path_buf()), None)
            .await;
        assert_eq!(registry.tool_names().await, vec!["gamma"]);

        fs::remove_file(first.path().join(".jcode").join("tools.json")).unwrap();
        registry
            .register_project_tools_with_global(Some(first.path().to_path_buf()), None)
            .await;
        assert!(registry.tool_names().await.is_empty());
    }

    struct DummyTool;

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "mcp"
        }

        fn description(&self) -> &str {
            "dummy"
        }

        fn parameters_schema(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }

        async fn execute(&self, _input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
            Ok(ToolOutput::new("dummy"))
        }
    }

    #[tokio::test]
    async fn existing_tool_name_wins_over_project_entry() {
        let registry = Registry::empty();
        registry
            .register("mcp".to_string(), std::sync::Arc::new(DummyTool))
            .await;
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [
                {"name": "mcp", "description": "shadow attempt", "command": ["/bin/false"]},
                {"name": "other", "description": "x", "command": ["/bin/true"]}
            ]}),
        );
        registry
            .register_project_tools_with_global(Some(dir.path().to_path_buf()), None)
            .await;

        let mut names = registry.tool_names().await;
        names.sort();
        assert_eq!(names, vec!["mcp", "other"]);
        let output = registry.execute("mcp", json!({}), ctx(None)).await.unwrap();
        assert_eq!(output.output, "dummy");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn passes_input_on_stdin_and_returns_stdout() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{"name": "echo_json", "description": "echo", "command": ["/bin/sh", "-c", "cat"], "timeout_secs": 5}]}),
        );
        let spec = load_specs_with_global(Some(dir.path()), None).remove(0);
        let tool = ProjectTool::new(spec);
        let out = tool
            .execute(
                json!({"hello": "world"}),
                ctx(Some(dir.path().to_path_buf())),
            )
            .await
            .unwrap();
        assert_eq!(out.output.trim(), r#"{"hello":"world"}"#);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn strips_intent_and_accept_large_output_from_stdin() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{"name": "echo_json", "description": "echo", "command": ["/bin/sh", "-c", "cat"], "timeout_secs": 5}]}),
        );
        let spec = load_specs_with_global(Some(dir.path()), None).remove(0);
        let out = ProjectTool::new(spec)
            .execute(
                json!({"hello": "world", "intent": "why", "accept_large_output": true}),
                ctx(Some(dir.path().to_path_buf())),
            )
            .await
            .unwrap();
        assert_eq!(out.output.trim(), r#"{"hello":"world"}"#);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn keeps_intent_when_the_schema_declares_it() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{
                "name": "echo_json",
                "description": "echo",
                "command": ["/bin/sh", "-c", "cat"],
                "parameters": {"type": "object", "properties": {"intent": {"type": "string"}}},
                "timeout_secs": 5
            }]}),
        );
        let spec = load_specs_with_global(Some(dir.path()), None).remove(0);
        let out = ProjectTool::new(spec)
            .execute(
                json!({"hello": "world", "intent": "keep"}),
                ctx(Some(dir.path().to_path_buf())),
            )
            .await
            .unwrap();
        assert_eq!(out.output.trim(), r#"{"hello":"world","intent":"keep"}"#);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn nonzero_exit_is_an_error_with_output() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{"name": "fail", "description": "fail", "command": ["/bin/sh", "-c", "echo nope >&2; exit 3"], "timeout_secs": 5}]}),
        );
        let spec = load_specs_with_global(Some(dir.path()), None).remove(0);
        let err = ProjectTool::new(spec)
            .execute(json!({}), ctx(Some(dir.path().to_path_buf())))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("status 3"), "{err}");
        assert!(err.contains("nope"), "{err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failure_keeps_stderr_when_stdout_fills_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{
                "name": "fail_loud",
                "description": "fail",
                "command": ["/bin/sh", "-c", "yes x | head -c 200000; echo boom >&2; exit 3"],
                "timeout_secs": 10
            }]}),
        );
        let spec = load_specs_with_global(Some(dir.path()), None).remove(0);
        let err = ProjectTool::new(spec)
            .execute(json!({}), ctx(Some(dir.path().to_path_buf())))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("boom"), "{err}");
        assert!(err.contains("[stdout]"), "{err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{"name": "slow", "description": "slow", "command": ["/bin/sh", "-c", "sleep 5"], "timeout_secs": 1}]}),
        );
        let spec = load_specs_with_global(Some(dir.path()), None).remove(0);
        let start = std::time::Instant::now();
        let err = ProjectTool::new(spec)
            .execute(json!({}), ctx(Some(dir.path().to_path_buf())))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("timed out"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(4));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn graceful_shutdown_cancels_a_running_tool() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{
                "name": "slow_cancel",
                "description": "slow",
                "command": ["/bin/sh", "-c", "sleep 5"],
                "timeout_secs": 30
            }]}),
        );
        let spec = load_specs_with_global(Some(dir.path()), None).remove(0);
        let signal = jcode_agent_runtime::InterruptSignal::new();
        let mut context = ctx(Some(dir.path().to_path_buf()));
        context.graceful_shutdown_signal = Some(signal.clone());

        let start = std::time::Instant::now();
        let tool = ProjectTool::new(spec);
        let execute = tool.execute(json!({}), context);
        let fire = async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            signal.fire();
        };
        let (result, ()) = tokio::join!(execute, fire);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cancelled"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stdin_write_is_inside_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{"name": "slow", "description": "slow", "command": ["/bin/sh", "-c", "sleep 5"], "timeout_secs": 1}]}),
        );
        let spec = load_specs_with_global(Some(dir.path()), None).remove(0);
        let start = std::time::Instant::now();
        // Larger than any plausible pipe buffer. The old implementation wrote
        // stdin before starting the timeout and could hang here forever.
        let input = json!({"blob": "x".repeat(1024 * 1024)});
        let err = ProjectTool::new(spec)
            .execute(input, ctx(Some(dir.path().to_path_buf())))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("timed out"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(4));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cwd_mode_session_runs_in_the_session_dir() {
        let project = tempfile::tempdir().unwrap();
        let session = tempfile::tempdir().unwrap();
        write_tools(
            project.path(),
            json!({"tools": [{"name": "pwd", "description": "pwd", "command": ["/bin/pwd"], "cwd": "session", "timeout_secs": 5}]}),
        );
        let spec = load_specs_with_global(Some(project.path()), None).remove(0);
        let out = ProjectTool::new(spec)
            .execute(json!({}), ctx(Some(session.path().to_path_buf())))
            .await
            .unwrap();
        let expected = fs::canonicalize(session.path()).unwrap();
        assert_eq!(out.output.trim(), expected.to_string_lossy());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn large_output_is_capped_while_being_read() {
        let dir = tempfile::tempdir().unwrap();
        write_tools(
            dir.path(),
            json!({"tools": [{"name": "big", "description": "big", "command": ["/bin/sh", "-c", "dd if=/dev/zero bs=1024 count=300 2>/dev/null | tr '\\000' x"], "timeout_secs": 5}]}),
        );
        let spec = load_specs_with_global(Some(dir.path()), None).remove(0);
        let out = ProjectTool::new(spec)
            .execute(json!({}), ctx(Some(dir.path().to_path_buf())))
            .await
            .unwrap();
        assert!(out.output.contains("[truncated"), "{}", out.output);
        assert!(
            out.output.len() <= MAX_RESULT_BYTES + 128,
            "capped output was {} bytes",
            out.output.len()
        );
    }
}
