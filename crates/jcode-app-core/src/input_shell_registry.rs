//! Process-global registry of running input-line shells, so a cancellation
//! request can stop a command that is not a model turn.
//!
//! The shell handlers run on detached tasks and are not represented in the
//! turn cancellation registry. Without this, Ctrl-C during `!sleep 60` (or a
//! project command) abandoned the child and let it keep running.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use tokio::sync::Notify;

static SHELLS: LazyLock<Mutex<HashMap<String, Vec<(u64, Arc<Notify>)>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// RAII registration for one running shell. Dropping it removes the entry.
pub struct InputShellGuard {
    session_id: String,
    request_id: u64,
    cancel: Arc<Notify>,
}

impl InputShellGuard {
    /// Wake this shell's cancellation future.
    pub(crate) fn cancel_handle(&self) -> Arc<Notify> {
        Arc::clone(&self.cancel)
    }
}

impl Drop for InputShellGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = SHELLS.lock()
            && let Some(entries) = map.get_mut(&self.session_id)
        {
            entries.retain(|(id, notify)| {
                !(*id == self.request_id && Arc::ptr_eq(notify, &self.cancel))
            });
            if entries.is_empty() {
                map.remove(&self.session_id);
            }
        }
    }
}

pub(crate) fn register(session_id: &str, request_id: u64) -> InputShellGuard {
    let cancel = Arc::new(Notify::new());
    if let Ok(mut map) = SHELLS.lock() {
        map.entry(session_id.to_string())
            .or_default()
            .push((request_id, Arc::clone(&cancel)));
    }
    InputShellGuard {
        session_id: session_id.to_string(),
        request_id,
        cancel,
    }
}

/// Cancel every shell running for `session_id`. Returns how many were asked to
/// stop. The registrations remove themselves when their tasks observe it.
pub(crate) fn cancel_session(session_id: &str) -> usize {
    let Ok(map) = SHELLS.lock() else {
        return 0;
    };
    let Some(entries) = map.get(session_id) else {
        return 0;
    };
    for (_, notify) in entries {
        notify.notify_one();
    }
    entries.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_registers_and_removes_itself() {
        let session = "ses_input_shell_guard";
        let guard = register(session, 7);
        assert_eq!(
            super::SHELLS.lock().unwrap().get(session).map(Vec::len),
            Some(1)
        );
        drop(guard);
        assert!(super::SHELLS.lock().unwrap().get(session).is_none());
    }

    #[tokio::test]
    async fn cancel_session_wakes_every_registered_shell() {
        let session = "ses_input_shell_cancel";
        let first = register(session, 1);
        let second = register(session, 2);
        let first_cancel = first.cancel_handle();
        let second_cancel = second.cancel_handle();

        assert_eq!(cancel_session(session), 2);
        tokio::time::timeout(std::time::Duration::from_secs(1), first_cancel.notified())
            .await
            .expect("first shell should be cancelled");
        tokio::time::timeout(std::time::Duration::from_secs(1), second_cancel.notified())
            .await
            .expect("second shell should be cancelled");
        drop(first);
        drop(second);
    }

    #[test]
    fn cancel_unknown_session_is_a_noop() {
        assert_eq!(cancel_session("ses_input_shell_missing"), 0);
    }
}
