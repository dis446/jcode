use super::*;

pub(super) fn build_input_shell_command(command: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("cmd.exe");
        cmd.arg("/C").arg(command);
        cmd
    }

    #[cfg(not(windows))]
    {
        let mut cmd = std::process::Command::new("bash");
        cmd.arg("-c").arg(command);
        cmd
    }
}

pub(super) fn combine_shell_output(stdout: &[u8], stderr: &[u8]) -> (String, bool) {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let mut output = String::new();

    if !stdout.is_empty() {
        output.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str("[stderr]\n");
        output.push_str(&stderr);
    }

    let truncated = if output.len() > INPUT_SHELL_MAX_OUTPUT_LEN {
        output = truncate_str(&output, INPUT_SHELL_MAX_OUTPUT_LEN).to_string();
        if !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str("… output truncated");
        true
    } else {
        false
    };

    (output, truncated)
}

pub(super) fn spawn_input_shell_command(session_id: String, command: String, cwd: Option<String>) {
    spawn_input_shell_command_with(session_id, command, cwd, None);
}

/// Like [`spawn_input_shell_command`], with an optional deadline. A timed-out
/// child is killed instead of being left to finish invisibly.
pub(super) fn spawn_input_shell_command_with(
    session_id: String,
    command: String,
    cwd: Option<String>,
    timeout_secs: Option<u64>,
) {
    std::thread::spawn(move || {
        use std::io::Read;

        let started = std::time::Instant::now();
        let mut cmd = build_input_shell_command(&command);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(dir) = cwd.as_ref() {
            cmd.current_dir(dir);
        }

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(error) => {
                let event = InputShellCompleted {
                    session_id,
                    result: crate::message::InputShellResult {
                        command,
                        cwd,
                        output: format!("Failed to run command: {}", error),
                        exit_code: None,
                        duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                        truncated: false,
                        failed_to_start: true,
                    },
                };
                Bus::global().publish(BusEvent::InputShellCompleted(event));
                return;
            }
        };

        let read_pipe = |pipe: Option<std::process::ChildStdout>| {
            pipe.map(|mut pipe| {
                std::thread::spawn(move || {
                    let mut buffer = Vec::new();
                    let _ = pipe.read_to_end(&mut buffer);
                    buffer
                })
            })
        };
        let stdout = read_pipe(child.stdout.take());
        let stderr = child.stderr.take().map(|mut pipe| {
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                let _ = pipe.read_to_end(&mut buffer);
                buffer
            })
        });

        let deadline = timeout_secs
            .map(|secs| std::time::Instant::now() + std::time::Duration::from_secs(secs));
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {}
                Err(error) => {
                    let event = InputShellCompleted {
                        session_id,
                        result: crate::message::InputShellResult {
                            command,
                            cwd,
                            output: format!("Failed to wait for command: {}", error),
                            exit_code: None,
                            duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                            truncated: false,
                            failed_to_start: true,
                        },
                    };
                    Bus::global().publish(BusEvent::InputShellCompleted(event));
                    return;
                }
            }
            if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };

        let stdout = stdout
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        let stderr = stderr
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        let (mut combined_output, truncated) = combine_shell_output(&stdout, &stderr);
        if timed_out {
            if !combined_output.is_empty() && !combined_output.ends_with('\n') {
                combined_output.push('\n');
            }
            combined_output.push_str(&match timeout_secs {
                Some(secs) => format!("Command timed out after {secs}s"),
                None => "Command timed out".to_string(),
            });
        }
        let event = InputShellCompleted {
            session_id,
            result: crate::message::InputShellResult {
                command,
                cwd,
                output: combined_output,
                exit_code: status.and_then(|status| status.code()),
                duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                truncated,
                failed_to_start: false,
            },
        };

        Bus::global().publish(BusEvent::InputShellCompleted(event));
    });
}
