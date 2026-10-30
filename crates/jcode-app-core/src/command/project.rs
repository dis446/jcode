//! Project-defined slash commands from `.jcode/commands.json`.
//!
//! A repository can declare deterministic, user-invoked commands next to the
//! project tools it already declares. Unlike a skill, a command runs one shell
//! line with no model turn, no provider tokens, and no dependence on the
//! session's tool list.
//!
//! ```json
//! {
//!   "commands": [
//!     {
//!       "name": "feature-list",
//!       "description": "List alpha features.",
//!       "command": "scripts/feature-workflow/feature-list",
//!       "cwd": "project",
//!       "timeout_secs": 120,
//!       "confirm": false
//!     }
//!   ]
//! }
//! ```
//!
//! `command` is a shell prefix. The text typed after the command name is
//! appended verbatim, so quoting, globs, and paths reach the script exactly as
//! typed. jcode does not add a shell here; a declared command may itself be
//! one. `cwd` is `project` (the root that owns the config file, following
//! symlinks) or `session` (the session working directory). Invalid entries are
//! skipped with a warning: a broken commands file must never prevent a session
//! from starting, and nothing is executed at load time.

use crate::project_config::{ConfigOwner, config_base_dir, read_config_file};
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 3600;
const MAX_NAME_LEN: usize = 64;
const MAX_COMMANDS: usize = 128;
const MAX_DESCRIPTION_BYTES: usize = 4 * 1024;
const MAX_COMMAND_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommandCwd {
    /// Run from the root that owns the config file.
    #[default]
    Project,
    /// Run from the session working directory.
    Session,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandsFile {
    #[serde(default)]
    commands: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandEntry {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    cwd: Option<CommandCwd>,
    #[serde(default)]
    confirm: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectCommand {
    name: String,
    description: String,
    command: String,
    timeout_secs: u64,
    cwd: CommandCwd,
    confirm: bool,
    /// Root of the config file that owns this entry: the project root for a
    /// repo file, `$JCODE_HOME` for the global file.
    base_dir: PathBuf,
}

impl ProjectCommand {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    pub fn requires_confirmation(&self) -> bool {
        self.confirm
    }

    /// The shell prefix declared by the config file.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Full shell line for a typed argument tail. The tail is appended
    /// verbatim, so the shell sees the same words the user typed.
    pub fn shell_line(&self, args: &str) -> String {
        let args = args.trim();
        if args.is_empty() {
            self.command.clone()
        } else {
            format!("{} {}", self.command, args)
        }
    }

    /// Directory the shell should run in, or `None` when the config root could
    /// not be resolved.
    pub fn working_dir(&self, session_working_dir: Option<&Path>) -> Option<PathBuf> {
        match self.cwd {
            CommandCwd::Project => Some(self.base_dir.clone()),
            CommandCwd::Session => Some(
                session_working_dir
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.base_dir.clone()),
            ),
        }
    }

    fn validate(entry: CommandEntry, base_dir: &Path, default_cwd: CommandCwd) -> Result<Self> {
        let name = entry.name.trim().to_string();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            bail!("command name must be 1..={MAX_NAME_LEN} characters");
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            bail!("command name '{name}' may only contain letters, digits, '_' and '-'");
        }
        let description = entry.description.trim().to_string();
        if description.is_empty() {
            bail!("command '{name}' has an empty description");
        }
        if description.len() > MAX_DESCRIPTION_BYTES {
            bail!("command '{name}' description exceeds {MAX_DESCRIPTION_BYTES} bytes");
        }
        // The description is repo-controlled text drawn directly in `/help` and
        // the completion list, where an escape sequence would reach the
        // terminal. Names are already restricted to ASCII, so this is the only
        // free-form string that gets rendered.
        if description.chars().any(char::is_control) {
            bail!("command '{name}' description contains control characters");
        }
        let command = entry.command.trim().to_string();
        if command.is_empty() {
            bail!("command '{name}' has no command");
        }
        if command.len() > MAX_COMMAND_BYTES {
            bail!("command '{name}' exceeds {MAX_COMMAND_BYTES} bytes");
        }
        if command.contains('\0') {
            bail!("command '{name}' contains a NUL byte");
        }
        let timeout_secs = entry
            .timeout_secs
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(1, MAX_TIMEOUT_SECS);
        Ok(Self {
            name,
            description,
            command,
            timeout_secs,
            cwd: entry.cwd.unwrap_or(default_cwd),
            confirm: entry.confirm,
            base_dir: base_dir.to_path_buf(),
        })
    }
}

/// `$JCODE_HOME/commands.json` (normally `~/.jcode/commands.json`).
pub fn global_commands_path() -> Option<PathBuf> {
    crate::storage::jcode_dir()
        .ok()
        .map(|dir| dir.join("commands.json"))
}

/// Load global then project command definitions for `working_dir`.
///
/// Project entries win on a name clash. A missing or malformed file never
/// fails the session.
pub fn load_commands(working_dir: Option<&Path>) -> Vec<ProjectCommand> {
    load_commands_with_global(working_dir, global_commands_path().as_deref())
}

/// Load with an explicit global file so tests never read the real home.
pub(crate) fn load_commands_with_global(
    working_dir: Option<&Path>,
    global: Option<&Path>,
) -> Vec<ProjectCommand> {
    let mut sources: Vec<(PathBuf, CommandCwd, ConfigOwner)> = Vec::new();
    if let Some(path) = global {
        // A global command usually acts on the project the session is in.
        sources.push((path.to_path_buf(), CommandCwd::Session, ConfigOwner::Global));
    }
    if let Some(root) = working_dir {
        sources.push((
            root.join(".jcode").join("commands.json"),
            CommandCwd::Project,
            ConfigOwner::Project,
        ));
    }

    let mut merged: Vec<ProjectCommand> = Vec::new();
    for (path, default_cwd, owner) in sources {
        let Some(raw) = read_config_file(&path, "project commands") else {
            continue;
        };
        let base_dir = config_base_dir(&path, owner);
        let file: CommandsFile = match serde_json::from_str(&raw) {
            Ok(file) => file,
            Err(err) => {
                crate::logging::warn(&format!(
                    "project commands: ignoring {}: {err}",
                    path.display()
                ));
                continue;
            }
        };
        for (index, value) in file.commands.into_iter().enumerate() {
            let entry: CommandEntry = match serde_json::from_value(value) {
                Ok(entry) => entry,
                Err(err) => {
                    crate::logging::warn(&format!(
                        "project commands: skipping entry #{} in {}: {err}",
                        index + 1,
                        path.display()
                    ));
                    continue;
                }
            };
            match ProjectCommand::validate(entry, &base_dir, default_cwd) {
                Ok(spec) => {
                    if !merged.iter().any(|existing| existing.name == spec.name)
                        && merged.len() >= MAX_COMMANDS
                    {
                        crate::logging::warn(&format!(
                            "project commands: ignoring '{}': limit is {MAX_COMMANDS} commands",
                            spec.name
                        ));
                        continue;
                    }
                    merge_command(&mut merged, spec);
                }
                Err(err) => crate::logging::warn(&format!(
                    "project commands: skipping an entry in {}: {err}",
                    path.display()
                )),
            }
        }
    }
    merged
}

/// Project files are read after the global one, so a later spec replaces an
/// earlier one with the same name.
fn merge_command(commands: &mut Vec<ProjectCommand>, spec: ProjectCommand) {
    if let Some(existing) = commands.iter_mut().find(|c| c.name == spec.name) {
        *existing = spec;
    } else {
        commands.push(spec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_config::MAX_CONFIG_BYTES;
    use serde_json::json;
    use std::fs;

    fn write_commands_file(path: &Path, json: Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, json.to_string()).unwrap();
    }

    fn write_commands(dir: &Path, json: Value) {
        write_commands_file(&dir.join(".jcode").join("commands.json"), json);
    }

    fn names(commands: &[ProjectCommand]) -> Vec<&str> {
        commands.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn loads_commands_and_skips_invalid_entries() {
        let dir = tempfile::tempdir().unwrap();
        write_commands(
            dir.path(),
            json!({"commands": [
                {"name": "feature-list", "description": "list", "command": "echo list"},
                {"name": "bad name", "description": "x", "command": "true"},
                {"name": "no_command", "description": "x"},
                {"name": "no_description", "command": "true"},
                {"name": "bad_cwd", "description": "x", "command": "true", "cwd": "bogus"},
                {"name": "empty_command", "description": "x", "command": "   "}
            ]}),
        );
        let commands = load_commands_with_global(Some(dir.path()), None);
        assert_eq!(names(&commands), vec!["feature-list"]);
        assert_eq!(commands[0].base_dir, fs::canonicalize(dir.path()).unwrap());
        assert_eq!(commands[0].timeout_secs(), DEFAULT_TIMEOUT_SECS);
        assert_eq!(commands[0].cwd, CommandCwd::Project);
        assert!(!commands[0].requires_confirmation());
    }

    #[test]
    fn rejects_unknown_entry_and_file_fields() {
        let dir = tempfile::tempdir().unwrap();
        write_commands(
            dir.path(),
            json!({"commands": [
                {"name": "typo", "description": "x", "command": "true", "timeout_second": 5}
            ]}),
        );
        assert!(load_commands_with_global(Some(dir.path()), None).is_empty());

        write_commands(
            dir.path(),
            json!({"command": [
                {"name": "typo", "description": "x", "command": "true"}
            ]}),
        );
        assert!(load_commands_with_global(Some(dir.path()), None).is_empty());
    }

    #[test]
    fn rejects_oversized_description_and_command() {
        let dir = tempfile::tempdir().unwrap();
        write_commands(
            dir.path(),
            json!({"commands": [
                {
                    "name": "long_description",
                    "description": "x".repeat(MAX_DESCRIPTION_BYTES + 1),
                    "command": "true"
                },
                {
                    "name": "long_command",
                    "description": "x",
                    "command": "x".repeat(MAX_COMMAND_BYTES + 1)
                }
            ]}),
        );
        assert!(load_commands_with_global(Some(dir.path()), None).is_empty());
    }

    /// Descriptions are drawn straight into `/help`, so a repo must not be able
    /// to inject terminal escapes through one.
    #[test]
    fn rejects_descriptions_with_control_characters() {
        let dir = tempfile::tempdir().unwrap();
        write_commands(
            dir.path(),
            json!({"commands": [
                {
                    "name": "escaped",
                    "description": "list \u{1b}]0;pwned\u{7}",
                    "command": "true"
                },
                {
                    "name": "clean",
                    "description": "list features",
                    "command": "true"
                }
            ]}),
        );
        let commands = load_commands_with_global(Some(dir.path()), None);
        assert_eq!(names(&commands), vec!["clean"]);
    }

    #[test]
    fn ignores_oversized_config_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".jcode")).unwrap();
        fs::write(
            dir.path().join(".jcode").join("commands.json"),
            vec![b' '; (MAX_CONFIG_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(load_commands_with_global(Some(dir.path()), None).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn ignores_non_regular_config_file() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".jcode")).unwrap();
        symlink("/dev/null", dir.path().join(".jcode").join("commands.json")).unwrap();
        assert!(load_commands_with_global(Some(dir.path()), None).is_empty());
    }

    #[test]
    fn caps_the_number_of_commands() {
        let dir = tempfile::tempdir().unwrap();
        let commands: Vec<Value> = (0..MAX_COMMANDS + 2)
            .map(|index| {
                json!({
                    "name": format!("command_{index}"),
                    "description": "x",
                    "command": "true"
                })
            })
            .collect();
        write_commands(dir.path(), json!({"commands": commands}));
        assert_eq!(
            load_commands_with_global(Some(dir.path()), None).len(),
            MAX_COMMANDS
        );
    }

    #[test]
    fn project_entry_overrides_global_name() {
        let global = tempfile::tempdir().unwrap();
        let global_path = global.path().join("commands.json");
        write_commands_file(
            &global_path,
            json!({"commands": [
                {"name": "front", "description": "global", "command": "global-cmd"}
            ]}),
        );
        let project = tempfile::tempdir().unwrap();
        write_commands(
            project.path(),
            json!({"commands": [
                {"name": "front", "description": "project", "command": "project-cmd"}
            ]}),
        );

        let commands = load_commands_with_global(Some(project.path()), Some(&global_path));
        assert_eq!(names(&commands), vec!["front"]);
        assert_eq!(commands[0].command(), "project-cmd");
    }

    #[test]
    fn global_entries_default_to_session_cwd() {
        let global = tempfile::tempdir().unwrap();
        let global_path = global.path().join("commands.json");
        write_commands_file(
            &global_path,
            json!({"commands": [
                {"name": "home", "description": "x", "command": "true"}
            ]}),
        );
        let project = tempfile::tempdir().unwrap();
        let commands = load_commands_with_global(Some(project.path()), Some(&global_path));
        assert_eq!(commands[0].cwd, CommandCwd::Session);
        assert_eq!(
            commands[0].working_dir(Some(project.path())).unwrap(),
            project.path()
        );
    }

    #[test]
    fn session_cwd_falls_back_to_the_config_root() {
        let dir = tempfile::tempdir().unwrap();
        write_commands(
            dir.path(),
            json!({"commands": [
                {"name": "here", "description": "x", "command": "true", "cwd": "session"}
            ]}),
        );
        let commands = load_commands_with_global(Some(dir.path()), None);
        assert_eq!(
            commands[0].working_dir(None).unwrap(),
            fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_config_runs_from_its_real_owner() {
        use std::os::unix::fs::symlink;

        let owner = tempfile::tempdir().unwrap();
        let owner_config = owner.path().join(".jcode").join("commands.json");
        write_commands_file(
            &owner_config,
            json!({"commands": [
                {"name": "owned", "description": "x", "command": "true"}
            ]}),
        );
        let feature = tempfile::tempdir().unwrap();
        fs::create_dir_all(feature.path().join(".jcode")).unwrap();
        symlink(
            &owner_config,
            feature.path().join(".jcode").join("commands.json"),
        )
        .unwrap();

        let commands = load_commands_with_global(Some(feature.path()), None);
        assert_eq!(
            commands[0].working_dir(None).unwrap(),
            fs::canonicalize(owner.path()).unwrap()
        );
    }

    #[test]
    fn clamps_timeout_and_reads_confirm() {
        let dir = tempfile::tempdir().unwrap();
        write_commands(
            dir.path(),
            json!({"commands": [
                {"name": "slow", "description": "x", "command": "true", "timeout_secs": 99999},
                {"name": "stop", "description": "x", "command": "true", "confirm": true},
                {"name": "zero", "description": "x", "command": "true", "timeout_secs": 0}
            ]}),
        );
        let commands = load_commands_with_global(Some(dir.path()), None);
        assert_eq!(commands[0].timeout_secs(), MAX_TIMEOUT_SECS);
        assert!(commands[1].requires_confirmation());
        assert_eq!(commands[2].timeout_secs(), 1);
    }

    #[test]
    fn appends_arguments_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        write_commands(
            dir.path(),
            json!({"commands": [
                {"name": "link", "description": "x", "command": "scripts/link --lead"}
            ]}),
        );
        let commands = load_commands_with_global(Some(dir.path()), None);
        assert_eq!(commands[0].shell_line(""), "scripts/link --lead");
        assert_eq!(commands[0].shell_line("   "), "scripts/link --lead");
        assert_eq!(
            commands[0].shell_line(r#" "a b.md" 'c d'"#),
            r#"scripts/link --lead "a b.md" 'c d'"#
        );
    }

    #[test]
    fn malformed_file_does_not_hide_a_valid_project_file() {
        let global = tempfile::tempdir().unwrap();
        let global_path = global.path().join("commands.json");
        fs::write(&global_path, "{not json").unwrap();
        let project = tempfile::tempdir().unwrap();
        write_commands(
            project.path(),
            json!({"commands": [
                {"name": "ok", "description": "x", "command": "true"}
            ]}),
        );
        let commands = load_commands_with_global(Some(project.path()), Some(&global_path));
        assert_eq!(names(&commands), vec!["ok"]);
    }
}
