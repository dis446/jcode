//! App-side glue for the branching questionnaire dialog.

use super::App;
use crate::tui::question_overlay::{QuestionOverlay, QuestionRequestEnvelope};

impl App {
    /// Show a questionnaire, or queue it behind one already in flight.
    pub(super) fn open_question_request(&mut self, request: QuestionRequestEnvelope) {
        if self.question_overlay.is_some() {
            self.pending_questions.push_back(request);
            return;
        }
        self.question_overlay = Some(std::cell::RefCell::new(QuestionOverlay::new(request)));
        self.set_status_notice("Waiting for your answer");
    }

    /// Close the current dialog and open the next queued one, if any.
    pub(super) fn finish_question_overlay(&mut self) {
        self.question_overlay = None;
        if let Some(request) = self.pending_questions.pop_front() {
            self.question_overlay = Some(std::cell::RefCell::new(QuestionOverlay::new(request)));
        }
    }

    /// True when the current dialog belongs to this tool call.
    pub(super) fn question_overlay_matches_tool(&self, tool_call_id: &str) -> bool {
        self.question_overlay
            .as_ref()
            .is_some_and(|overlay| overlay.borrow().tool_call_id() == tool_call_id)
    }

    /// Dismiss a dialog whose tool call finished, was interrupted, or errored.
    pub(super) fn dismiss_question_for_tool(&mut self, tool_call_id: &str) {
        if self.question_overlay_matches_tool(tool_call_id) {
            self.finish_question_overlay();
        }
    }

    /// Dismiss every dialog. Used on disconnect, done, interrupt, and error.
    pub(super) fn clear_question_overlays(&mut self) {
        self.question_overlay = None;
        self.pending_questions.clear();
    }

    /// Route a paste into the custom answer editor when it is active.
    pub(super) fn handle_question_paste(&mut self, text: &str) -> bool {
        let Some(overlay) = self.question_overlay.as_ref() else {
            return false;
        };
        let sanitized = super::input::strip_terminal_control_sequences(text).into_owned();
        overlay.borrow_mut().insert_pasted_text(&sanitized)
    }
}
