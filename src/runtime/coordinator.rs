//! Process-local coordinator for session execution, wake, and interrupt.
//!
//! Enforces:
//! - At most one active drain thread per session.
//! - Different sessions run concurrently.
//! - Rapid wakes coalesce into at most one follow-up turn.
//! - Interrupts cancel the active generation, clear pending wakes, and
//!   prevent wakes during cleanup from starting a new drain.
//! - Preserves unpromoted pending inputs in SQLite.

use std::collections::HashMap;
use std::sync::Mutex;

/// Outcome of waking a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeOutcome {
    /// Session was idle; caller should spawn a drain thread.
    Scheduled,
    /// Session was active; a follow-up turn was marked pending.
    Coalesced,
    /// Session was cleaning up from an interrupt; wake was ignored.
    Ignored,
}

impl WakeOutcome {
    /// True if this wake scheduled a new drain thread.
    pub fn is_scheduled(&self) -> bool {
        matches!(self, Self::Scheduled)
    }
}

/// Parameters for running generations in a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    pub agent_mode: String,
    pub provider: String,
    pub model: String,
}

/// Process-local state for one session.
#[derive(Debug, Default, Clone)]
pub struct SessionState {
    pub active: bool,
    pub active_generation_id: Option<i64>,
    pub wake_pending: bool,
    pub cleaning_up: bool,
    pub config: Option<SessionConfig>,
}

/// Thread-safe coordinator keyed by session id.
#[derive(Debug, Default)]
pub struct SessionCoordinator {
    sessions: Mutex<HashMap<i64, SessionState>>,
}

impl SessionCoordinator {
    /// Create a new coordinator with an empty session registry.
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Wake a session:
    /// - If currently `cleaning_up`, ignores wake and returns [`WakeOutcome::Ignored`].
    /// - If currently `active`, marks `wake_pending = true` and returns [`WakeOutcome::Coalesced`].
    /// - If idle, marks `active = true`, `wake_pending = false`, and returns [`WakeOutcome::Scheduled`].
    pub fn wake(&self, session_id: i64) -> WakeOutcome {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let state = sessions.entry(session_id).or_default();
        if state.cleaning_up {
            WakeOutcome::Ignored
        } else if state.active {
            state.wake_pending = true;
            WakeOutcome::Coalesced
        } else {
            state.active = true;
            state.wake_pending = false;
            WakeOutcome::Scheduled
        }
    }

    /// Complete a turn:
    /// - If `cleaning_up` is true, resets `active = false`, `active_generation_id = None`,
    ///   `wake_pending = false`, and returns `false`.
    /// - If `wake_pending` is true, clears `wake_pending`, keeps `active = true`,
    ///   clears `active_generation_id`, and returns `true` (indicating another turn should run).
    /// - If `wake_pending` is false, marks `active = false`, `active_generation_id = None`,
    ///   and returns `false`.
    pub fn complete_turn(&self, session_id: i64) -> bool {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let Some(state) = sessions.get_mut(&session_id) else {
            return false;
        };
        if state.cleaning_up {
            state.active = false;
            state.active_generation_id = None;
            state.wake_pending = false;
            false
        } else if state.wake_pending {
            state.wake_pending = false;
            state.active = true;
            state.active_generation_id = None;
            true
        } else {
            state.active = false;
            state.active_generation_id = None;
            false
        }
    }

    /// Interrupt a session:
    /// - Clears `wake_pending`.
    /// - Marks `cleaning_up = true` if session was active.
    /// - Returns the `active_generation_id` (if any) to cancel.
    /// - Idempotent: returns `None` if session is idle or unknown.
    pub fn interrupt(&self, session_id: i64) -> Option<i64> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let state = sessions.get_mut(&session_id)?;
        if !state.active {
            return None;
        }
        state.wake_pending = false;
        state.cleaning_up = true;
        state.active_generation_id
    }

    /// Reset session state to idle (`active = false`, `cleaning_up = false`,
    /// `wake_pending = false`, `active_generation_id = None`).
    pub fn finish_interrupt(&self, session_id: i64) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(state) = sessions.get_mut(&session_id) {
            state.active = false;
            state.cleaning_up = false;
            state.wake_pending = false;
            state.active_generation_id = None;
        }
    }

    /// Finish a drain when no more turns should run.
    pub fn finish_drain(&self, session_id: i64) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(state) = sessions.get_mut(&session_id) {
            state.active = false;
            state.active_generation_id = None;
            state.wake_pending = false;
        }
    }

    /// Record the currently running generation id for a session.
    pub fn set_active_generation(&self, session_id: i64, generation_id: i64) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let state = sessions.entry(session_id).or_default();
        state.active_generation_id = Some(generation_id);
    }

    /// Clear the active generation id if it matches `generation_id`.
    pub fn clear_active_generation(&self, session_id: i64, generation_id: i64) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(state) = sessions.get_mut(&session_id)
            && state.active_generation_id == Some(generation_id)
        {
            state.active_generation_id = None;
        }
    }

    /// Check if the session is currently active.
    pub fn is_active(&self, session_id: i64) -> bool {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        sessions.get(&session_id).map(|s| s.active).unwrap_or(false)
    }

    /// Check if the session is currently cleaning up from an interrupt.
    pub fn is_cleaning_up(&self, session_id: i64) -> bool {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        sessions
            .get(&session_id)
            .map(|s| s.cleaning_up)
            .unwrap_or(false)
    }

    /// Check if a follow-up wake is pending for this session.
    pub fn is_wake_pending(&self, session_id: i64) -> bool {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        sessions
            .get(&session_id)
            .map(|s| s.wake_pending)
            .unwrap_or(false)
    }

    /// Return the active generation id if any.
    pub fn active_generation_id(&self, session_id: i64) -> Option<i64> {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        sessions
            .get(&session_id)
            .and_then(|s| s.active_generation_id)
    }

    /// Store session generation parameters.
    pub fn set_session_config(&self, session_id: i64, config: SessionConfig) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let state = sessions.entry(session_id).or_default();
        state.config = Some(config);
    }

    /// Fetch session generation parameters.
    pub fn session_config(&self, session_id: i64) -> Option<SessionConfig> {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        sessions.get(&session_id).and_then(|s| s.config.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_idle() {
        let coord = SessionCoordinator::new();
        assert!(!coord.is_active(1));
        assert!(!coord.is_cleaning_up(1));
        assert!(!coord.is_wake_pending(1));
        assert_eq!(coord.active_generation_id(1), None);
    }

    #[test]
    fn wake_on_idle_schedules() {
        let coord = SessionCoordinator::new();
        assert_eq!(coord.wake(1), WakeOutcome::Scheduled);
        assert!(coord.is_active(1));
        assert!(!coord.is_wake_pending(1));
    }

    #[test]
    fn wake_on_active_coalesces() {
        let coord = SessionCoordinator::new();
        assert_eq!(coord.wake(1), WakeOutcome::Scheduled);
        assert_eq!(coord.wake(1), WakeOutcome::Coalesced);
        assert!(coord.is_active(1));
        assert!(coord.is_wake_pending(1));

        // Repeated wake remains coalesced without duplicate scheduling
        assert_eq!(coord.wake(1), WakeOutcome::Coalesced);
        assert!(coord.is_wake_pending(1));
    }

    #[test]
    fn complete_turn_with_wake_pending_continues() {
        let coord = SessionCoordinator::new();
        coord.wake(1);
        coord.set_active_generation(1, 42);
        coord.wake(1); // coalesced

        assert!(coord.complete_turn(1));
        assert!(coord.is_active(1));
        assert!(!coord.is_wake_pending(1));
        assert_eq!(coord.active_generation_id(1), None);

        // Next turn completion with no pending wake marks idle
        assert!(!coord.complete_turn(1));
        assert!(!coord.is_active(1));
        assert_eq!(coord.active_generation_id(1), None);
    }

    #[test]
    fn interrupt_clears_pending_wake_and_sets_cleaning_up() {
        let coord = SessionCoordinator::new();
        coord.wake(1);
        coord.set_active_generation(1, 100);
        coord.wake(1); // wake_pending = true

        assert_eq!(coord.interrupt(1), Some(100));
        assert!(!coord.is_wake_pending(1));
        assert!(coord.is_cleaning_up(1));

        // Wake during cleanup is ignored
        assert_eq!(coord.wake(1), WakeOutcome::Ignored);

        // Finishing interrupt resets state to idle
        coord.finish_interrupt(1);
        assert!(!coord.is_active(1));
        assert!(!coord.is_cleaning_up(1));
        assert!(!coord.is_wake_pending(1));
        assert_eq!(coord.active_generation_id(1), None);

        // New wake after cleanup schedules normally
        assert_eq!(coord.wake(1), WakeOutcome::Scheduled);
    }

    #[test]
    fn interrupt_on_idle_or_unknown_is_safe_noop() {
        let coord = SessionCoordinator::new();
        assert_eq!(coord.interrupt(999), None);
        assert!(!coord.is_cleaning_up(999));

        coord.wake(1);
        coord.complete_turn(1); // idle
        assert_eq!(coord.interrupt(1), None);
        assert!(!coord.is_cleaning_up(1));
    }

    #[test]
    fn cross_session_isolation() {
        let coord = SessionCoordinator::new();
        assert_eq!(coord.wake(1), WakeOutcome::Scheduled);
        assert_eq!(coord.wake(2), WakeOutcome::Scheduled);

        coord.set_active_generation(1, 10);
        coord.set_active_generation(2, 20);

        assert_eq!(coord.interrupt(1), Some(10));
        assert!(coord.is_cleaning_up(1));
        assert!(!coord.is_cleaning_up(2));
        assert!(coord.is_active(2));
        assert_eq!(coord.active_generation_id(2), Some(20));

        coord.finish_interrupt(1);
        assert!(!coord.is_active(1));
        assert!(coord.is_active(2));
    }
}
