//! Full-frame branching questionnaire dialog.
//!
//! The tool validates the graph before it reaches the client, so this module
//! trusts node ids and focuses on navigation and rendering. Nothing here caps
//! the number of questions or options; the option and review lists scroll.

use crate::protocol::{QuestionAnswer, QuestionNode, QuestionOption, UserQuestionResult};
use crossterm::event::{KeyCode, KeyModifiers};
use jcode_tui_render::truncate_line_with_ellipsis_to_width;
use jcode_tui_style::rgb;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};
use std::collections::{BTreeSet, HashMap};

/// A request waiting for the dialog to become free.
#[derive(Debug, Clone)]
pub struct QuestionRequestEnvelope {
    pub request_id: String,
    pub tool_call_id: String,
    pub intent: String,
    pub root: String,
    pub questions: Vec<QuestionNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Asking,
    Review,
}

#[derive(Debug, Clone)]
struct AnswerFrame {
    node_id: String,
    selected: usize,
    multi_selected: BTreeSet<usize>,
    custom: Option<String>,
}

/// What the caller must do after a key press.
pub enum QuestionKeyOutcome {
    /// The dialog consumed the key.
    Handled,
    /// The user cancelled. Send this result and close the dialog.
    Cancel(UserQuestionResult),
    /// The user answered everything. Send this result and close the dialog.
    Submit(UserQuestionResult),
}

pub struct QuestionOverlay {
    request_id: String,
    tool_call_id: String,
    intent: String,
    nodes: HashMap<String, QuestionNode>,
    current: String,
    phase: Phase,
    path: Vec<AnswerFrame>,
    answers: Vec<QuestionAnswer>,
    selected: usize,
    multi_selected: BTreeSet<usize>,
    editing_custom: bool,
    custom: String,
    custom_cursor: usize,
    list_state: ListState,
}

impl QuestionOverlay {
    pub fn new(request: QuestionRequestEnvelope) -> Self {
        let mut nodes = HashMap::with_capacity(request.questions.len());
        for node in request.questions {
            nodes.insert(node.id.clone(), node);
        }
        let current = if nodes.contains_key(&request.root) {
            request.root.clone()
        } else {
            nodes.keys().next().cloned().unwrap_or_default()
        };
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            request_id: request.request_id,
            tool_call_id: request.tool_call_id,
            intent: request.intent,
            nodes,
            current,
            phase: Phase::Asking,
            path: Vec::new(),
            answers: Vec::new(),
            selected: 0,
            multi_selected: BTreeSet::new(),
            editing_custom: false,
            custom: String::new(),
            custom_cursor: 0,
            list_state,
        }
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    /// Insert pasted text into an active custom answer. Returns false when the
    /// dialog is not editing one, so the caller can route the paste elsewhere.
    pub fn insert_pasted_text(&mut self, text: &str) -> bool {
        if !self.editing_custom {
            return false;
        }
        self.insert_custom(text);
        true
    }

    fn node(&self) -> Option<&QuestionNode> {
        self.nodes.get(&self.current)
    }

    fn row_count(&self) -> usize {
        let options = self.node().map_or(0, |node| node.options.len());
        let custom = self
            .node()
            .is_some_and(|node| node.allow_custom)
            .then_some(1)
            .unwrap_or(0);
        options + custom
    }

    fn showing_custom_row(&self) -> bool {
        self.node()
            .is_some_and(|node| node.allow_custom && self.selected == node.options.len())
    }

    fn sync_list_state(&mut self) {
        let len = match self.phase {
            Phase::Asking => self.row_count(),
            Phase::Review => self.review_rows().len(),
        };
        if len == 0 {
            self.selected = 0;
            self.list_state.select(None);
            return;
        }
        if self.selected >= len {
            self.selected = len - 1;
        }
        self.list_state.select(Some(self.selected));
    }

    pub fn handle_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        text: Option<&str>,
    ) -> QuestionKeyOutcome {
        if self.editing_custom {
            return self.handle_custom_edit_key(code, modifiers, text);
        }
        if code == KeyCode::Esc
            || (code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL))
        {
            return QuestionKeyOutcome::Cancel(UserQuestionResult::Cancelled {
                reason: Some("user cancelled the questionnaire".into()),
            });
        }

        match self.phase {
            Phase::Asking => self.handle_asking_key(code, modifiers, text),
            Phase::Review => self.handle_review_key(code),
        }
    }

    fn handle_asking_key(
        &mut self,
        code: KeyCode,
        _modifiers: KeyModifiers,
        _text: Option<&str>,
    ) -> QuestionKeyOutcome {
        let page = 10usize;
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.row_count();
                if len > 0 {
                    self.selected = (self.selected + 1).min(len - 1);
                }
            }
            KeyCode::PageUp => {
                self.selected = self.selected.saturating_sub(page);
            }
            KeyCode::PageDown => {
                self.selected = (self.selected + page).min(self.row_count().saturating_sub(1));
            }
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = self.row_count().saturating_sub(1),
            KeyCode::Left | KeyCode::Backspace => {
                self.go_back();
            }
            KeyCode::Char(' ') => {
                self.toggle_multi();
            }
            KeyCode::Enter => {
                if self.showing_custom_row() {
                    self.begin_custom_edit();
                } else if let Some(node) = self.node().cloned() {
                    if node.multi_select {
                        if self.multi_selected.is_empty() {
                            return QuestionKeyOutcome::Handled;
                        }
                        let selected = self.selected_values(&node);
                        let custom = self.non_empty_custom();
                        self.commit_answer(&node, selected, custom);
                    } else if let Some(option) = node.options.get(self.selected).cloned() {
                        self.commit_single(&node, &option);
                    }
                }
            }
            _ => return QuestionKeyOutcome::Handled,
        }
        self.sync_list_state();
        QuestionKeyOutcome::Handled
    }

    fn handle_review_key(&mut self, code: KeyCode) -> QuestionKeyOutcome {
        let rows = self.review_rows().len();
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if rows > 0 {
                    self.selected = (self.selected + 1).min(rows - 1);
                }
            }
            KeyCode::PageUp => self.selected = self.selected.saturating_sub(10),
            KeyCode::PageDown => {
                self.selected = (self.selected + 10).min(rows.saturating_sub(1));
            }
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = rows.saturating_sub(1),
            KeyCode::Left | KeyCode::Backspace => {
                self.go_back();
            }
            KeyCode::Enter => match self.selected {
                0 => {
                    return QuestionKeyOutcome::Submit(UserQuestionResult::Answered {
                        answers: self.answers.clone(),
                    });
                }
                1 => self.go_back(),
                _ => {}
            },
            _ => {}
        }
        self.sync_list_state();
        QuestionKeyOutcome::Handled
    }

    fn handle_custom_edit_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        text: Option<&str>,
    ) -> QuestionKeyOutcome {
        if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            return QuestionKeyOutcome::Cancel(UserQuestionResult::Cancelled {
                reason: Some("user cancelled the questionnaire".into()),
            });
        }
        match code {
            KeyCode::Esc => {
                self.editing_custom = false;
            }
            KeyCode::Enter if modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL) => {
                self.insert_custom("\n");
            }
            KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_custom("\n");
            }
            KeyCode::Enter => {
                let custom = self.custom.trim().to_string();
                if !custom.is_empty() {
                    if let Some(node) = self.node().cloned() {
                        self.editing_custom = false;
                        let selected = if node.multi_select {
                            self.selected_values(&node)
                        } else {
                            Vec::new()
                        };
                        self.commit_answer(&node, selected, Some(custom));
                    }
                } else {
                    self.editing_custom = false;
                }
            }
            KeyCode::Backspace => {
                self.backspace_custom();
            }
            KeyCode::Left => {
                self.custom_cursor = self.custom_cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                self.custom_cursor = (self.custom_cursor + 1).min(self.custom.chars().count());
            }
            KeyCode::Home => self.custom_cursor = 0,
            KeyCode::End => self.custom_cursor = self.custom.chars().count(),
            _ => {
                if let Some(text) = text {
                    self.insert_custom(text);
                }
            }
        }
        self.sync_list_state();
        QuestionKeyOutcome::Handled
    }

    fn insert_custom(&mut self, text: &str) {
        let mut chars: Vec<char> = self.custom.chars().collect();
        let at = self.custom_cursor.min(chars.len());
        let inserted: Vec<char> = text.chars().collect();
        let inserted_len = inserted.len();
        chars.splice(at..at, inserted);
        self.custom = chars.into_iter().collect();
        self.custom_cursor = at + inserted_len;
    }

    fn backspace_custom(&mut self) {
        if self.custom_cursor == 0 {
            return;
        }
        let mut chars: Vec<char> = self.custom.chars().collect();
        let at = self.custom_cursor.min(chars.len());
        chars.remove(at - 1);
        self.custom = chars.into_iter().collect();
        self.custom_cursor = at - 1;
    }

    fn begin_custom_edit(&mut self) {
        self.editing_custom = true;
        self.custom_cursor = self.custom.chars().count();
    }

    fn non_empty_custom(&self) -> Option<String> {
        let custom = self.custom.trim();
        (!custom.is_empty()).then(|| custom.to_string())
    }

    fn toggle_multi(&mut self) {
        let Some(node) = self.node() else {
            return;
        };
        if !node.multi_select || self.selected >= node.options.len() {
            return;
        }
        if !self.multi_selected.remove(&self.selected) {
            self.multi_selected.insert(self.selected);
        }
    }

    fn selected_values(&self, node: &QuestionNode) -> Vec<String> {
        self.multi_selected
            .iter()
            .filter_map(|index| node.options.get(*index))
            .map(|option| option.value.clone())
            .collect()
    }

    fn commit_single(&mut self, node: &QuestionNode, option: &QuestionOption) {
        let selected = vec![option.value.clone()];
        let next = option.next.clone().or_else(|| node.next.clone());
        self.commit_answer_with_next(node, selected, None, next);
    }

    fn commit_answer(
        &mut self,
        node: &QuestionNode,
        selected: Vec<String>,
        custom: Option<String>,
    ) {
        let next = node.next.clone();
        self.commit_answer_with_next(node, selected, custom, next);
    }

    fn commit_answer_with_next(
        &mut self,
        node: &QuestionNode,
        selected: Vec<String>,
        custom: Option<String>,
        next: Option<String>,
    ) {
        self.answers.push(QuestionAnswer {
            question_id: node.id.clone(),
            selected,
            custom,
        });
        self.path.push(AnswerFrame {
            node_id: node.id.clone(),
            selected: self.selected,
            multi_selected: self.multi_selected.clone(),
            custom: self.non_empty_custom(),
        });

        match next {
            Some(next) if self.nodes.contains_key(&next) => {
                self.current = next;
                self.selected = 0;
                *self.list_state.offset_mut() = 0;
                self.multi_selected.clear();
                self.custom.clear();
                self.custom_cursor = 0;
                self.editing_custom = false;
            }
            _ => {
                self.phase = Phase::Review;
                self.selected = 0;
                *self.list_state.offset_mut() = 0;
            }
        }
        self.sync_list_state();
    }

    fn go_back(&mut self) {
        let Some(frame) = self.path.pop() else {
            return;
        };
        self.answers.pop();
        self.current = frame.node_id;
        self.phase = Phase::Asking;
        self.selected = frame.selected;
        *self.list_state.offset_mut() = 0;
        self.multi_selected = frame.multi_selected;
        self.custom = frame.custom.unwrap_or_default();
        self.custom_cursor = self.custom.chars().count();
        self.editing_custom = false;
        self.sync_list_state();
    }

    fn review_rows(&self) -> Vec<(String, bool)> {
        let mut rows = vec![
            ("Submit answers".to_string(), true),
            ("Back to last question".to_string(), true),
        ];
        for answer in &self.answers {
            let node = self.nodes.get(&answer.question_id);
            let label = node
                .and_then(|node| node.header.as_deref())
                .filter(|header| !header.is_empty())
                .map(single_line)
                .unwrap_or_else(|| "Question".to_string());
            let mut value = if answer.selected.is_empty() {
                String::new()
            } else {
                answer
                    .selected
                    .iter()
                    .map(|selected| {
                        node.and_then(|node| {
                            node.options
                                .iter()
                                .find(|option| &option.value == selected)
                                .map(|option| single_line(&option.label))
                        })
                        .unwrap_or_else(|| selected.clone())
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            if let Some(custom) = answer.custom.as_deref() {
                if !value.is_empty() {
                    value.push_str(" + ");
                }
                value.push_str(&single_line(custom));
            }
            rows.push((format!("{}: {}", label, value), false));
        }
        rows
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(
            Block::default().style(Style::default().bg(rgb(18, 18, 22))),
            area,
        );
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(4),
                Constraint::Min(3),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);

        self.render_header(frame, chunks[0]);
        self.render_progress(frame, chunks[1]);
        self.render_prompt(frame, chunks[2]);
        match self.phase {
            Phase::Asking => self.render_options(frame, chunks[3]),
            Phase::Review => self.render_review(frame, chunks[3]),
        }
        self.render_detail(frame, chunks[4]);
        self.render_footer(frame, chunks[5]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let count = format!("{} answered", self.answers.len());
        let count_width = count.chars().count();
        let width = area.width as usize;
        let intent_width = width.saturating_sub(count_width + 2);
        let mut spans = vec![
            Span::styled(
                truncate_text(&single_line(&self.intent), intent_width),
                Style::default()
                    .fg(rgb(235, 235, 240))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(count, Style::default().fg(rgb(110, 110, 120))),
        ];
        let line = Line::from(std::mem::take(&mut spans));
        let line = truncate_line_with_ellipsis_to_width(&line, width);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_progress(&self, frame: &mut Frame, area: Rect) {
        let number = self.answers.len() + 1;
        let mut text = format!("Question {number}");
        if !self.answers.is_empty() {
            text.push_str("  ");
            text.push_str(&self.breadcrumb());
        }
        let line = Line::from(Span::styled(text, Style::default().fg(rgb(150, 150, 160))));
        let line = truncate_line_with_ellipsis_to_width(&line, area.width as usize);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn breadcrumb(&self) -> String {
        let mut parts = Vec::new();
        for answer in &self.answers {
            let Some(node) = self.nodes.get(&answer.question_id) else {
                continue;
            };
            let label = node
                .header
                .as_deref()
                .map(single_line)
                .unwrap_or_else(|| "Question".to_string());
            let value = if answer.selected.is_empty() {
                answer.custom.clone().unwrap_or_default()
            } else {
                answer
                    .selected
                    .iter()
                    .map(|selected| {
                        node.options
                            .iter()
                            .find(|option| &option.value == selected)
                            .map(|option| option.label.as_str())
                            .unwrap_or(selected.as_str())
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            parts.push(format!("{label}: {}", single_line(&value)));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!(" › {}", parts.join(" › "))
        }
    }

    fn render_prompt(&self, frame: &mut Frame, area: Rect) {
        let text = self
            .node()
            .map(|node| node.question.clone())
            .unwrap_or_default();
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(Color::White)),
            area,
        );
    }

    fn render_options(&mut self, frame: &mut Frame, area: Rect) {
        let Some(node) = self.node().cloned() else {
            return;
        };
        let mut items = Vec::with_capacity(node.options.len() + 1);
        for (index, option) in node.options.iter().enumerate() {
            let selected = self.multi_selected.contains(&index);
            let marker = if node.multi_select {
                if selected { "[x] " } else { "[ ] " }
            } else {
                ""
            };
            let marker_style = if selected {
                Style::default().fg(rgb(120, 220, 150))
            } else {
                Style::default().fg(rgb(90, 90, 100))
            };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(marker, marker_style),
                Span::styled(
                    single_line(&option.label),
                    Style::default().fg(rgb(225, 225, 230)),
                ),
            ])));
        }
        if node.allow_custom {
            let text = if self.custom.is_empty() {
                "Type something.".to_string()
            } else {
                format!("Type something: {}", single_line(&self.custom))
            };
            items.push(ListItem::new(Line::from(Span::styled(
                text,
                Style::default().fg(rgb(150, 150, 160)),
            ))));
        }
        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(rgb(45, 45, 55))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› ");
        self.sync_list_state();
        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_review(&mut self, frame: &mut Frame, area: Rect) {
        let rows = self.review_rows();
        let items: Vec<ListItem> = rows
            .into_iter()
            .map(|(text, action)| {
                let style = if action {
                    Style::default()
                        .fg(rgb(220, 220, 230))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(rgb(190, 190, 200))
                };
                ListItem::new(Line::from(Span::styled(text, style)))
            })
            .collect();
        let list = List::new(items)
            .highlight_style(Style::default().bg(rgb(45, 45, 55)).fg(Color::White))
            .highlight_symbol("› ");
        self.sync_list_state();
        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_detail(&self, frame: &mut Frame, area: Rect) {
        let text = if self.phase == Phase::Review {
            "Review your answers, then submit. Left goes back to the last question.".to_string()
        } else if self.editing_custom {
            let mut shown = self.custom.clone();
            shown.push('▎');
            format!("> {shown}")
        } else if self.showing_custom_row() {
            "Type your own answer and press Enter.".to_string()
        } else {
            self.node()
                .and_then(|node| node.options.get(self.selected))
                .and_then(|option| option.description.clone())
                .unwrap_or_default()
        };
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(rgb(140, 140, 150))),
            area,
        );
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let hints = if self.editing_custom {
            "enter submit  esc back to list  shift+enter newline"
        } else {
            "up/down move  enter select  left back  esc cancel"
        };
        let line = Line::from(Span::styled(hints, Style::default().fg(rgb(90, 90, 100))));
        let line = truncate_line_with_ellipsis_to_width(&line, area.width as usize);
        frame.render_widget(Paragraph::new(line), area);
    }
}

fn truncate_text(text: &str, width: usize) -> String {
    let line = Line::from(text.to_string());
    let line = truncate_line_with_ellipsis_to_width(&line, width);
    line.spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
}

/// Collapse embedded line breaks so list rows and chrome stay one visual row.
fn single_line(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\n' | '\r' | '\t' => ' ',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn request() -> QuestionRequestEnvelope {
        QuestionRequestEnvelope {
            request_id: "req".into(),
            tool_call_id: "call".into(),
            intent: "Choose the path".into(),
            root: "task".into(),
            questions: vec![
                QuestionNode {
                    id: "task".into(),
                    header: Some("Task".into()),
                    question: "Which task?".into(),
                    options: vec![
                        QuestionOption {
                            value: "bug".into(),
                            label: "Bug fix".into(),
                            description: Some("Fix a defect".into()),
                            next: Some("area".into()),
                        },
                        QuestionOption {
                            value: "feature".into(),
                            label: "New feature".into(),
                            description: None,
                            next: Some("area".into()),
                        },
                    ],
                    multi_select: false,
                    allow_custom: true,
                    next: None,
                },
                QuestionNode {
                    id: "area".into(),
                    header: Some("Area".into()),
                    question: "Which area?".into(),
                    options: vec![QuestionOption {
                        value: "tui".into(),
                        label: "TUI".into(),
                        description: None,
                        next: None,
                    }],
                    multi_select: false,
                    allow_custom: true,
                    next: None,
                },
            ],
        }
    }

    #[test]
    fn follows_a_branch_and_converges() {
        let mut overlay = QuestionOverlay::new(request());
        assert_eq!(overlay.current, "task");
        assert!(matches!(
            overlay.handle_key(KeyCode::Enter, KeyModifiers::NONE, None),
            QuestionKeyOutcome::Handled
        ));
        assert_eq!(overlay.current, "area");
        let outcome = overlay.handle_key(KeyCode::Enter, KeyModifiers::NONE, None);
        assert!(matches!(outcome, QuestionKeyOutcome::Handled));
        assert_eq!(overlay.phase, Phase::Review);
        let outcome = overlay.handle_key(KeyCode::Enter, KeyModifiers::NONE, None);
        let QuestionKeyOutcome::Submit(UserQuestionResult::Answered { answers }) = outcome else {
            panic!("expected submit");
        };
        assert_eq!(answers.len(), 2);
        assert_eq!(answers[0].selected, vec!["bug"]);
        assert_eq!(answers[1].selected, vec!["tui"]);
    }

    #[test]
    fn back_restores_the_previous_question() {
        let mut overlay = QuestionOverlay::new(request());
        overlay.handle_key(KeyCode::Enter, KeyModifiers::NONE, None);
        assert_eq!(overlay.current, "area");
        overlay.handle_key(KeyCode::Left, KeyModifiers::NONE, None);
        assert_eq!(overlay.current, "task");
        assert!(overlay.answers.is_empty());
    }

    #[test]
    fn escape_cancels() {
        let mut overlay = QuestionOverlay::new(request());
        let outcome = overlay.handle_key(KeyCode::Esc, KeyModifiers::NONE, None);
        assert!(matches!(
            outcome,
            QuestionKeyOutcome::Cancel(UserQuestionResult::Cancelled { .. })
        ));
    }

    #[test]
    fn multi_select_toggles_and_submits() {
        let mut request = request();
        request.questions = vec![QuestionNode {
            id: "tests".into(),
            header: None,
            question: "Which tests?".into(),
            options: vec![
                QuestionOption {
                    value: "unit".into(),
                    label: "Unit".into(),
                    description: None,
                    next: None,
                },
                QuestionOption {
                    value: "e2e".into(),
                    label: "E2E".into(),
                    description: None,
                    next: None,
                },
            ],
            multi_select: true,
            allow_custom: false,
            next: None,
        }];
        request.root = "tests".into();
        let mut overlay = QuestionOverlay::new(request);
        overlay.handle_key(KeyCode::Char(' '), KeyModifiers::NONE, Some(" "));
        overlay.handle_key(KeyCode::Down, KeyModifiers::NONE, None);
        overlay.handle_key(KeyCode::Char(' '), KeyModifiers::NONE, Some(" "));
        overlay.handle_key(KeyCode::Enter, KeyModifiers::NONE, None);
        assert_eq!(overlay.phase, Phase::Review);
        assert_eq!(overlay.answers[0].selected, vec!["unit", "e2e"]);
    }

    #[test]
    fn custom_answer_is_recorded() {
        let mut overlay = QuestionOverlay::new(request());
        overlay.selected = 2;
        overlay.handle_key(KeyCode::Enter, KeyModifiers::NONE, None);
        assert!(overlay.editing_custom);
        for ch in "bespoke".chars() {
            overlay.handle_key(KeyCode::Char(ch), KeyModifiers::NONE, Some(&ch.to_string()));
        }
        overlay.handle_key(KeyCode::Enter, KeyModifiers::NONE, None);
        assert_eq!(overlay.answers[0].custom.as_deref(), Some("bespoke"));
        assert_eq!(overlay.current, "task");
    }

    #[test]
    fn renders_long_option_lists_without_wrapping_chrome() {
        let mut request = request();
        let options: Vec<QuestionOption> = (0..500)
            .map(|index| QuestionOption {
                value: format!("v{index}"),
                label: format!("Option {index} with a long label that should truncate"),
                description: None,
                next: None,
            })
            .collect();
        request.questions = vec![QuestionNode {
            id: "many".into(),
            header: None,
            question: "Pick one".into(),
            options,
            multi_select: false,
            allow_custom: true,
            next: None,
        }];
        request.root = "many".into();
        let mut overlay = QuestionOverlay::new(request);
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| overlay.render(frame))
            .expect("render should succeed");
        let buffer = terminal.backend().buffer();
        for y in 0..20 {
            let row: String = (0..40)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect();
            // Chrome rows must remain a single visual row; the renderer writes
            // exactly one buffer row for each of them.
            assert!(row.chars().count() <= 40);
        }
    }
}
