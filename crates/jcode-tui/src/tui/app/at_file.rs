//! CUSTOM (dis446 fork, ledger C1): `@file` composer completion state.
//!
//! The pure logic lives in the `jcode-at-file` crate. This module owns the
//! popup state, key handling and the off-thread search. Typing `@` opens a
//! popup of files and directories under the session working directory. Up/Down
//! move, Tab/Enter accept, Esc closes. `@path` stays literal text: nothing is
//! expanded on submit (the model reads the file itself).

use super::App;
use crossterm::event::{KeyCode, KeyModifiers};
use jcode_at_file::{AtPrefix, Candidate};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

/// Wait this long for a search before leaving it to the idle tick.
const SYNC_WAIT: Duration = Duration::from_millis(15);
/// Hard cap for a Tab-forced (synchronous) search.
const FORCE_BUDGET: Duration = Duration::from_millis(400);

pub(crate) struct AtFileState {
    prefix: AtPrefix,
    items: Vec<Candidate>,
    selected: usize,
}

pub(crate) struct AtFilePending {
    id: u64,
    query: String,
    cancel: Arc<AtomicBool>,
    rx: Receiver<Vec<Candidate>>,
}

impl Drop for AtFilePending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn next_request_id() -> u64 {
    use std::sync::atomic::AtomicU64;
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn spawn_search(cwd: PathBuf, query: String) -> AtFilePending {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = channel();
    let flag = cancel.clone();
    let q = query.clone();
    std::thread::spawn(move || {
        let _ = tx.send(jcode_at_file::search(&cwd, &q, &flag));
    });
    AtFilePending {
        id: next_request_id(),
        query,
        cancel,
        rx,
    }
}

impl App {
    fn at_file_cwd(&self) -> PathBuf {
        self.session
            .working_dir
            .as_deref()
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub(crate) fn at_file_close(&mut self) {
        self.at_file = None;
        self.at_file_pending = None;
    }

    pub(crate) fn at_file_view(&self) -> Option<crate::tui::AtFileView> {
        let state = self.at_file.as_ref()?;
        if state.items.is_empty() {
            return None;
        }
        Some(crate::tui::AtFileView {
            rows: state
                .items
                .iter()
                .map(|c| (c.name.clone(), c.path.clone(), c.is_dir))
                .collect(),
            selected: state.selected,
        })
    }

    /// Re-evaluate the popup after a typed character or backspace.
    pub(crate) fn at_file_after_edit(&mut self) {
        let Some(prefix) = jcode_at_file::extract_at_prefix(&self.input, self.cursor_pos) else {
            self.at_file_close();
            return;
        };
        // Same token and query as the visible list: nothing to do.
        if let Some(state) = &self.at_file
            && state.prefix == prefix
        {
            return;
        }
        let pending = spawn_search(self.at_file_cwd(), prefix.query.clone());
        // Brief wait keeps the popup in step with fast typing on small trees.
        match pending.rx.recv_timeout(SYNC_WAIT) {
            Ok(items) => {
                self.at_file_pending = None;
                self.at_file_set(prefix, items);
            }
            Err(RecvTimeoutError::Timeout) => {
                // Newer request replaces (and cancels) the older one via Drop.
                self.at_file_pending = Some(pending);
            }
            Err(RecvTimeoutError::Disconnected) => self.at_file_close(),
        }
    }

    fn at_file_set(&mut self, prefix: AtPrefix, items: Vec<Candidate>) {
        if items.is_empty() {
            self.at_file = None;
            return;
        }
        self.at_file = Some(AtFileState {
            prefix,
            items,
            selected: 0,
        });
    }

    /// Idle tick: adopt a finished search unless the text moved on. True = redraw.
    pub(crate) fn poll_at_file(&mut self) -> bool {
        let Some(pending) = &self.at_file_pending else {
            return false;
        };
        let items = match pending.rx.try_recv() {
            Ok(items) => items,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.at_file_pending = None;
                return false;
            }
        };
        let (id, query) = (pending.id, pending.query.clone());
        self.at_file_pending = None;
        let _ = id;
        // Stale check: the cursor token must still be the one searched for.
        match jcode_at_file::extract_at_prefix(&self.input, self.cursor_pos) {
            Some(prefix) if prefix.query == query => {
                self.at_file_set(prefix, items);
                true
            }
            _ => false,
        }
    }

    pub(super) fn handle_at_file_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        let Some(state) = self.at_file.as_mut() else {
            return false;
        };
        if state.items.is_empty() {
            return false;
        }
        let plain_nav = !modifiers.intersects(
            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER,
        );
        let len = state.items.len() as i32;
        let mut step = |state: &mut AtFileState, d: i32| {
            state.selected = (state.selected as i32 + d).rem_euclid(len) as usize;
        };
        match code {
            KeyCode::Down if plain_nav => step(state, 1),
            KeyCode::Up if plain_nav => step(state, -1),
            KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) => step(state, 1),
            KeyCode::Char('k') if modifiers.contains(KeyModifiers::CONTROL) => step(state, -1),
            KeyCode::Tab | KeyCode::Enter if modifiers.is_empty() => {
                self.at_file_accept();
            }
            KeyCode::Esc if modifiers.is_empty() => self.at_file_close(),
            _ => return false,
        }
        true
    }

    fn at_file_accept(&mut self) {
        let Some(state) = self.at_file.take() else {
            return;
        };
        self.at_file_pending = None;
        let Some(candidate) = state.items.get(state.selected) else {
            return;
        };
        self.at_file_apply(&state.prefix, candidate);
        // Directories keep the popup open so the user can keep drilling.
        if candidate.is_dir {
            self.at_file_after_edit();
        }
    }

    fn at_file_apply(&mut self, prefix: &AtPrefix, candidate: &Candidate) {
        self.remember_input_undo_state();
        let (text, cursor) =
            jcode_at_file::apply_completion(&self.input, self.cursor_pos, prefix, candidate);
        self.input = text;
        self.cursor_pos = cursor;
        self.tab_completion_state = None;
        self.sync_model_picker_preview_from_input();
    }

    /// Tab on a non-slash token: complete it as a path. One match applies at
    /// once, several open the popup. Returns whether Tab was consumed.
    pub(crate) fn at_file_force_complete(&mut self) -> bool {
        let Some(prefix) = jcode_at_file::extract_path_token(&self.input, self.cursor_pos) else {
            return false;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let timer = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(FORCE_BUDGET);
            timer.store(true, Ordering::Relaxed);
        });
        let items = jcode_at_file::search(&self.at_file_cwd(), &prefix.query, &cancel);
        cancel.store(true, Ordering::Relaxed);
        match items.len() {
            0 => false,
            1 => {
                let candidate = items[0].clone();
                self.at_file_apply(&prefix, &candidate);
                if candidate.is_dir {
                    self.at_file_after_edit();
                }
                true
            }
            _ => {
                self.at_file_set(prefix, items);
                true
            }
        }
    }
}
