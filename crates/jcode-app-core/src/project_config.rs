//! Shared reading rules for repo-declared configuration files.
//!
//! `.jcode/tools.json` and `.jcode/commands.json` are read from
//! `$JCODE_HOME` and `<working dir>/.jcode`. Both are repository-controlled
//! input, so the reader must never follow special files, never grow unbounded,
//! and never fail a session because one entry is malformed.

use std::io::Read;
use std::path::{Path, PathBuf};

pub(crate) const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Which config file owns an entry. This decides the default cwd and the root
/// a relative command resolves against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigOwner {
    /// `$JCODE_HOME/<file>` belongs to the user's home.
    Global,
    /// `<root>/.jcode/<file>` belongs to the repository at `<root>`.
    Project,
}

/// Read one config file without following special files or unbounded growth.
/// A missing file is silent; every other skip is logged and returns `None`.
pub(crate) fn read_config_file(path: &Path, label: &str) -> Option<String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            crate::logging::warn(&format!(
                "{label}: cannot inspect {}: {err}",
                path.display()
            ));
            return None;
        }
    };
    if !metadata.is_file() {
        crate::logging::warn(&format!(
            "{label}: ignoring {}: not a regular file",
            path.display()
        ));
        return None;
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        crate::logging::warn(&format!(
            "{label}: ignoring {}: larger than {MAX_CONFIG_BYTES} bytes",
            path.display()
        ));
        return None;
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) => {
            crate::logging::warn(&format!("{label}: cannot read {}: {err}", path.display()));
            return None;
        }
    };
    let mut raw = String::new();
    if let Err(err) = file.take(MAX_CONFIG_BYTES + 1).read_to_string(&mut raw) {
        crate::logging::warn(&format!("{label}: cannot read {}: {err}", path.display()));
        return None;
    }
    if raw.len() as u64 > MAX_CONFIG_BYTES {
        crate::logging::warn(&format!(
            "{label}: ignoring {}: larger than {MAX_CONFIG_BYTES} bytes",
            path.display()
        ));
        return None;
    }
    Some(raw)
}

/// Root of the project that owns `path`, resolving symlinks so a linked
/// config runs commands from its real owner rather than the link location.
pub(crate) fn config_base_dir(path: &Path, owner: ConfigOwner) -> PathBuf {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match owner {
        ConfigOwner::Global => real.parent(),
        ConfigOwner::Project => real.parent().and_then(Path::parent),
    }
    .map(Path::to_path_buf)
    .unwrap_or_else(|| PathBuf::from("."))
}
