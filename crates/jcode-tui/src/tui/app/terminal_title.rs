//! Cheap, session-scoped metadata for the terminal's native tabs.
//!
//! Diff counts are cumulative edit-output lines, matching the transcript's diff
//! badges, not a repository-wide `git diff` (other sessions may share the repo).
//! Time is the current/last observed working episode (including continuations),
//! never the age of an open tab or a fabricated lifetime total.
use super::{App, DisplayMessage};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct TerminalTitleState {
    pub base: String,
    pub session_id: String,
    pub last_sent: String,
    pub last_refresh: Option<Instant>,
    pub last_work: Option<Duration>,
}

pub(super) fn edit_line_counts(message: &DisplayMessage) -> (usize, usize) {
    let Some(tool) = message.tool_data.as_ref().filter(|tool| {
        message.role == "tool"
            && crate::tui::ui::tools_ui::is_edit_tool_name(&tool.name)
            && jcode_tui_tool_display::concise_tool_error_summary(&message.content).is_none()
    }) else {
        return (0, 0);
    };
    crate::tui::ui_diff::diff_change_counts_for_tool(tool, &message.content)
}

fn work_label(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 3600 {
        format!("{}h{:02}m", secs / 3600, secs / 60 % 60)
    } else if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

fn title_with_metrics(
    base: &str,
    counts: (usize, usize),
    work: Option<Duration>,
    active: bool,
) -> String {
    let mut title = base.to_owned();
    if counts != (0, 0) {
        title.push_str(&format!(" · +{} -{}", counts.0, counts.1));
    }
    if let Some(work) = work {
        title.push_str(&format!(
            " · {} ~{}",
            if active { "work" } else { "last" },
            work_label(work)
        ));
    } else if active {
        title.push_str(" · working");
    }
    title
}

impl App {
    /// herdr's `working` state must cover delegated work as well as this
    /// client's own turn: swarm workers and background tasks keep running
    /// while the parent waits, and a turn that ends to await them is not
    /// `is_processing()`. Counts come from in-memory UI state only (no
    /// filesystem or process probes) because this runs on every tick and
    /// every full draw.
    pub(super) fn herdr_agent_status(&self) -> (crate::tui::herdr::AgentState, Option<String>) {
        let workers = self.active_swarm_member_count();
        let tasks = self.running_background_task_count();
        let state = if self.is_processing() || workers > 0 || tasks > 0 {
            crate::tui::herdr::AgentState::Working
        } else {
            crate::tui::herdr::AgentState::Idle
        };
        (state, herdr_work_message(workers, tasks))
    }

    fn running_background_task_count(&self) -> usize {
        // Rows are kept in memory from task lifecycle events in both local and
        // remote clients. `running_snapshot()` would read a status file per
        // task, which is too much for a per-tick path.
        self.background_task_rows
            .iter()
            .filter(|row| row.status == crate::tui::BackgroundTaskRowStatus::Running)
            .count()
    }

    /// Tell herdr (when running in a herdr pane) whether this client is
    /// working or idle and which session to resume. Tick-safe: unchanged
    /// state is a cheap comparison and reports are sent off-thread.
    pub(super) fn sync_herdr_agent_state(&self) {
        if self.suppress_terminal_title_updates
            || !self.is_remote_client()
            || !crate::tui::herdr::enabled()
        {
            return;
        }
        let (state, message) = self.herdr_agent_status();
        crate::tui::herdr::sync(state, message.as_deref(), self.active_client_session_id());
    }

    pub(super) fn set_terminal_title_base(&self, session_id: &str, base: String) {
        let mut state = self.terminal_title.borrow_mut();
        if state.session_id != session_id {
            *state = TerminalTitleState::default();
            state.session_id = session_id.to_owned();
        }
        state.base = base;
        state.last_refresh = None;
        drop(state);
        self.refresh_terminal_title_metrics();
    }

    /// Tick-safe: no filesystem access, transcript scans, or subprocesses.
    /// At most one OSC title update per second, and only if the text changed.
    pub(super) fn refresh_terminal_title_metrics(&self) {
        if self.suppress_terminal_title_updates {
            return;
        }
        let mut state = self.terminal_title.borrow_mut();
        if state.base.is_empty()
            || state
                .last_refresh
                .is_some_and(|at| at.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        state.last_refresh = Some(Instant::now());
        let active = self.is_processing();
        let work = if active {
            self.display_turn_duration_secs()
                .filter(|secs| secs.is_finite() && *secs >= 0.0)
                .map(Duration::from_secs_f32)
        } else {
            state.last_work
        };
        if active && work.is_some() {
            state.last_work = work;
        }
        let title = title_with_metrics(&state.base, self.display_edit_line_counts, work, active);
        if state.last_sent != title
            && crossterm::execute!(std::io::stdout(), crossterm::terminal::SetTitle(&title)).is_ok()
        {
            state.last_sent = title;
        }
    }

    pub(super) fn remember_terminal_title_work(&self) {
        if let Some(secs) = self
            .display_turn_duration_secs()
            .filter(|secs| secs.is_finite() && *secs >= 0.0)
        {
            let mut state = self.terminal_title.borrow_mut();
            state.last_work = Some(Duration::from_secs_f32(secs));
            state.last_refresh = None;
        }
    }
}

/// Compact reason shown in herdr's sidebar, e.g. "3 workers running".
fn herdr_work_message(workers: usize, tasks: usize) -> Option<String> {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    match (workers, tasks) {
        (0, 0) => None,
        (0, tasks) => Some(format!("{tasks} background task{}", plural(tasks))),
        (workers, 0) => Some(format!("{workers} worker{} running", plural(workers))),
        (workers, tasks) => Some(format!(
            "{workers} worker{}, {tasks} background task{}",
            plural(workers),
            plural(tasks)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ToolCall;
    use crate::protocol::SwarmMemberStatus;
    use crate::tui::herdr::AgentState;
    use crate::tui::{BackgroundTaskRow, BackgroundTaskRowStatus};

    fn member(status: &str) -> SwarmMemberStatus {
        SwarmMemberStatus {
            session_id: "worker-1".into(),
            friendly_name: Some("worker-1".into()),
            status: status.into(),
            detail: None,
            task_label: None,
            role: None,
            is_headless: Some(true),
            live_attachments: None,
            status_age_secs: Some(1),
            output_tail: None,
            report_back_to_session_id: None,
            todo_progress: None,
            todo_items: Vec::new(),
            runtime: crate::protocol::SwarmMemberRuntime::default(),
        }
    }

    fn app_with_member(status: &str) -> App {
        let mut app = super::super::tests::create_test_app();
        app.debug_force_inline_gallery = true;
        app.remote_swarm_members = vec![member(status)];
        app
    }

    #[test]
    fn herdr_reports_working_while_swarm_member_runs() {
        for status in ["running", "streaming", "thinking"] {
            let app = app_with_member(status);
            assert_eq!(app.herdr_agent_status().0, AgentState::Working, "{status}");
        }
        assert_eq!(
            app_with_member("running").herdr_agent_status().1.as_deref(),
            Some("1 worker running")
        );
    }

    #[test]
    fn herdr_reports_idle_after_swarm_member_completes() {
        for status in ["completed", "failed", "stopped", "ready"] {
            let app = app_with_member(status);
            assert_eq!(
                app.herdr_agent_status(),
                (AgentState::Idle, None),
                "{status}"
            );
        }
    }

    #[test]
    fn herdr_reports_working_for_running_background_task() {
        let mut app = super::super::tests::create_test_app();
        app.background_task_rows.push(BackgroundTaskRow {
            task_id: "task-1".into(),
            label: "cargo test".into(),
            percent: None,
            status: BackgroundTaskRowStatus::Running,
            completed_at: None,
        });
        assert_eq!(
            app.herdr_agent_status(),
            (AgentState::Working, Some("1 background task".into()))
        );
        app.background_task_rows[0].status = BackgroundTaskRowStatus::Completed;
        assert_eq!(app.herdr_agent_status(), (AgentState::Idle, None));
    }

    #[test]
    fn herdr_work_message_counts_workers_and_tasks() {
        assert_eq!(herdr_work_message(0, 0), None);
        assert_eq!(
            herdr_work_message(1, 0).as_deref(),
            Some("1 worker running")
        );
        assert_eq!(
            herdr_work_message(3, 0).as_deref(),
            Some("3 workers running")
        );
        assert_eq!(
            herdr_work_message(0, 2).as_deref(),
            Some("2 background tasks")
        );
        assert_eq!(
            herdr_work_message(2, 1).as_deref(),
            Some("2 workers, 1 background task")
        );
    }

    #[test]
    fn terminal_title_counts_follow_append_remove_and_history_replace() {
        let mut app = super::super::tests::create_test_app();
        app.suppress_terminal_title_updates = true;
        let message = DisplayMessage::tool(
            "@@ -1 +1 @@\n-old\n+new\n+extra",
            ToolCall {
                id: "edit-1".into(),
                name: "edit".into(),
                input: serde_json::json!({"file_path": "example.rs"}),
                intent: None,
                thought_signature: None,
            },
        );
        app.push_display_message(message.clone());
        assert_eq!(app.display_edit_line_counts, (2, 1));
        app.push_display_message(DisplayMessage::user("continue"));
        assert_eq!(app.display_edit_line_counts, (2, 1));
        app.adjust_display_message_stats(&message, false);
        assert_eq!(app.display_edit_line_counts, (0, 0));
        app.replace_display_messages(vec![message.clone(), message]);
        assert_eq!(app.display_edit_line_counts, (4, 2));
        app.replace_display_messages(vec![]);
        assert_eq!(app.display_edit_line_counts, (0, 0));
    }

    #[test]
    fn terminal_title_work_freezes_and_resets_on_session_switch() {
        let mut app = super::super::tests::create_test_app();
        app.suppress_terminal_title_updates = true;
        app.set_terminal_title_base("first", "First task".into());
        app.visible_turn_started = Some(Instant::now() - Duration::from_secs(125));
        app.clear_visible_turn_started();
        let completed = app.terminal_title.borrow().last_work.unwrap();
        assert_eq!(completed.as_secs(), 125);
        app.remember_terminal_title_work();
        assert_eq!(app.terminal_title.borrow().last_work, Some(completed));
        app.set_terminal_title_base("first", "Renamed task".into());
        assert_eq!(app.terminal_title.borrow().last_work, Some(completed));
        app.set_terminal_title_base("second", "Second task".into());
        assert_eq!(app.terminal_title.borrow().last_work, None);
        assert!(app.terminal_title.borrow().last_sent.is_empty());
    }

    #[test]
    fn title_metrics_show_edits_and_current_or_last_turn() {
        let base = "🦊 Improve session tabs";
        assert_eq!(title_with_metrics(base, (0, 0), None, false), base);
        assert_eq!(
            title_with_metrics(base, (12, 3), Some(Duration::from_secs(125)), true),
            "🦊 Improve session tabs · +12 -3 · work ~2m05s"
        );
        assert_eq!(
            title_with_metrics(base, (0, 4), Some(Duration::from_secs(125)), false),
            "🦊 Improve session tabs · +0 -4 · last ~2m05s"
        );
        assert_eq!(
            title_with_metrics(base, (0, 0), None, true),
            "🦊 Improve session tabs · working"
        );
    }

    #[test]
    fn work_duration_boundaries_are_compact() {
        for (secs, expected) in [
            (0, "0s"),
            (59, "59s"),
            (60, "1m00s"),
            (3599, "59m59s"),
            (3600, "1h00m"),
            (90061, "25h01m"),
        ] {
            assert_eq!(work_label(Duration::from_secs(secs)), expected);
        }
    }

    #[test]
    fn edit_counts_ignore_failures_and_non_edit_output() {
        let mut message = DisplayMessage::tool(
            "@@ -1 +1 @@\n-old\n+new\n+extra",
            ToolCall {
                id: "edit-1".into(),
                name: "edit".into(),
                input: serde_json::json!({"file_path": "example.rs"}),
                intent: None,
                thought_signature: None,
            },
        );
        assert_eq!(edit_line_counts(&message), (2, 1));
        message.tool_data.as_mut().unwrap().name = "bash".into();
        assert_eq!(edit_line_counts(&message), (0, 0));
        message.tool_data.as_mut().unwrap().name = "edit".into();
        message.content = "Error: could not apply patch\n-old\n+new".into();
        assert_eq!(edit_line_counts(&message), (0, 0));
    }
}
