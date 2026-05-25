//! Preview-webview lifecycle.
//!
//! [`PreviewWebviewManager`] is the host-side controller for the per-tab
//! preview webview pool (ADR-009 §"Performance budgets > Pre-warming"):
//!
//! - Warm pool of size 1 held in [`PreviewWebviewManager::pool`] and
//!   replenished on the next idle tick after each consumption.
//! - Per-tab assignments tracked in [`PreviewWebviewManager::assignments`].
//! - Idle-pause / idle-recycle policy: a webview that has not rendered for
//!   30 s is paused; after 5 min of paused state it is recycled.
//! - Dev-tools disabled in release builds via the `#[cfg(...)]` gate
//!   forwarded into the builder factory.
//!
//! The actual `WebviewWindowBuilder` calls are not exercised in Phase 1
//! unit tests (they need a running Tauri app). Phase 2's
//! `<PreviewPane>` integration test drives the spawn path end-to-end on
//! the CI matrix.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use writ_core::preview::{ContentTypeId, WindowId};

/// State of a single tracked preview webview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebviewState {
    /// Held in the warm pool, never rendered.
    Warm,
    /// Mounted in a layout slot and actively rendering.
    Active,
    /// Mounted but paused after the 30-second idle threshold.
    Paused,
    /// Removed; will be recreated on next demand.
    Recycled,
}

/// Internal handle for a preview webview slot.
///
/// Phase 1 carries the metadata that drives the warm-pool / idle-pause /
/// recycle policy. Phase 2 will attach a `tauri::WebviewWindow` reference
/// alongside.
#[derive(Debug, Clone)]
pub struct WebviewSlot {
    /// Logical identifier — opaque to the host, stable for the slot's
    /// lifetime. Phase 2 spawn paths derive a concrete Tauri label from
    /// this.
    pub id: SlotId,
    /// Current lifecycle state.
    pub state: WebviewState,
    /// Buffer the slot is currently serving, if any.
    pub buffer_id: Option<String>,
    /// Content type the slot is currently serving, if any.
    pub content_type: Option<ContentTypeId>,
    /// Last render timestamp; absence means "warm, never rendered".
    pub last_render: Option<Instant>,
}

/// Monotonic slot identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId(pub u64);

/// Policy constants (ADR-009 §"Performance budgets").
pub const IDLE_PAUSE_AFTER: Duration = Duration::from_secs(30);
pub const IDLE_RECYCLE_AFTER: Duration = Duration::from_secs(5 * 60);
pub const POOL_SIZE: usize = 1;

/// Lifecycle controller for the per-tab preview webview pool.
pub struct PreviewWebviewManager {
    inner: Mutex<Inner>,
}

struct Inner {
    next_slot_id: u64,
    pool: Vec<WebviewSlot>,
    assignments: HashMap<(WindowId, String), SlotId>,
    slots: HashMap<SlotId, WebviewSlot>,
}

impl PreviewWebviewManager {
    /// Construct an empty manager. The warm pool is populated lazily on
    /// the first idle tick (Phase 2 wires that scheduler hook).
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                next_slot_id: 1,
                pool: Vec::with_capacity(POOL_SIZE),
                assignments: HashMap::new(),
                slots: HashMap::new(),
            }),
        })
    }

    /// Number of warm slots currently in the pool.
    pub fn pool_len(&self) -> usize {
        let inner = self.inner.lock().expect("preview manager mutex");
        inner.pool.len()
    }

    /// Add a warm slot to the pool. Phase 2's spawner calls this after
    /// successfully constructing a fresh webview.
    pub fn record_warm_slot(&self) -> SlotId {
        let mut inner = self.inner.lock().expect("preview manager mutex");
        let id = SlotId(inner.next_slot_id);
        inner.next_slot_id += 1;
        let slot = WebviewSlot {
            id,
            state: WebviewState::Warm,
            buffer_id: None,
            content_type: None,
            last_render: None,
        };
        if inner.pool.len() < POOL_SIZE {
            inner.pool.push(slot.clone());
        }
        inner.slots.insert(id, slot);
        id
    }

    /// Consume the warm slot for a tab, returning its id. Returns `None`
    /// when the pool is empty (the caller falls back to the cold-spawn
    /// path).
    pub fn take_warm_for(
        &self,
        window_id: WindowId,
        buffer_id: &str,
        content_type: ContentTypeId,
    ) -> Option<SlotId> {
        let mut inner = self.inner.lock().expect("preview manager mutex");
        let mut slot = inner.pool.pop()?;
        slot.state = WebviewState::Active;
        slot.buffer_id = Some(buffer_id.to_string());
        slot.content_type = Some(content_type);
        slot.last_render = Some(Instant::now());
        let id = slot.id;
        inner.slots.insert(id, slot);
        inner
            .assignments
            .insert((window_id, buffer_id.to_string()), id);
        Some(id)
    }

    /// Look up the slot currently assigned to a `(window, buffer)` pair.
    pub fn slot_for(&self, window_id: WindowId, buffer_id: &str) -> Option<SlotId> {
        let inner = self.inner.lock().expect("preview manager mutex");
        inner
            .assignments
            .get(&(window_id, buffer_id.to_string()))
            .copied()
    }

    /// Record a successful render — refreshes the idle timer.
    pub fn note_render(&self, slot: SlotId) {
        let mut inner = self.inner.lock().expect("preview manager mutex");
        if let Some(s) = inner.slots.get_mut(&slot) {
            s.state = WebviewState::Active;
            s.last_render = Some(Instant::now());
        }
    }

    /// Tear down a webview slot and clear its assignment.
    pub fn close(&self, window_id: WindowId, buffer_id: &str) {
        let mut inner = self.inner.lock().expect("preview manager mutex");
        if let Some(id) = inner.assignments.remove(&(window_id, buffer_id.to_string())) {
            if let Some(s) = inner.slots.get_mut(&id) {
                s.state = WebviewState::Recycled;
                s.buffer_id = None;
                s.content_type = None;
            }
        }
    }

    /// Mark a slot as crashed; the next render attempt will spawn fresh.
    pub fn record_crash(&self, slot: SlotId) {
        let mut inner = self.inner.lock().expect("preview manager mutex");
        if let Some(s) = inner.slots.get_mut(&slot) {
            s.state = WebviewState::Recycled;
            s.last_render = None;
        }
    }

    /// Apply the idle policy at the given instant. Returns the slot ids
    /// transitioned to [`WebviewState::Paused`] and [`WebviewState::Recycled`].
    ///
    /// Phase 2 schedules this on an interval timer. Splitting the policy
    /// out of the timer makes it deterministic to unit-test here.
    pub fn apply_idle_policy(&self, now: Instant) -> IdleTransitions {
        let mut transitions = IdleTransitions::default();
        let mut inner = self.inner.lock().expect("preview manager mutex");
        for slot in inner.slots.values_mut() {
            let Some(last) = slot.last_render else {
                continue;
            };
            let elapsed = now.saturating_duration_since(last);
            match slot.state {
                WebviewState::Active if elapsed >= IDLE_PAUSE_AFTER => {
                    slot.state = WebviewState::Paused;
                    transitions.paused.push(slot.id);
                }
                WebviewState::Paused if elapsed >= IDLE_RECYCLE_AFTER => {
                    slot.state = WebviewState::Recycled;
                    transitions.recycled.push(slot.id);
                }
                _ => {}
            }
        }
        // Drop assignments that point at recycled slots.
        let recycled: std::collections::HashSet<SlotId> =
            transitions.recycled.iter().copied().collect();
        inner.assignments.retain(|_, id| !recycled.contains(id));
        transitions
    }
}

/// Slots transitioned by the idle policy on a single pass.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct IdleTransitions {
    /// Slots newly paused.
    pub paused: Vec<SlotId>,
    /// Slots newly recycled.
    pub recycled: Vec<SlotId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctype() -> ContentTypeId {
        ContentTypeId::new("html")
    }

    #[test]
    fn record_warm_slot_grows_pool_up_to_capacity() {
        let m = PreviewWebviewManager::new();
        assert_eq!(m.pool_len(), 0);
        m.record_warm_slot();
        assert_eq!(m.pool_len(), POOL_SIZE);
        // Recording above capacity does not grow the pool but the slot
        // is still tracked.
        m.record_warm_slot();
        assert_eq!(m.pool_len(), POOL_SIZE);
    }

    #[test]
    fn take_warm_consumes_one_slot_and_assigns_it() {
        let m = PreviewWebviewManager::new();
        m.record_warm_slot();
        let id = m
            .take_warm_for(WindowId::MAIN, "buf-1", ctype())
            .expect("warm slot");
        assert_eq!(m.pool_len(), 0);
        assert_eq!(m.slot_for(WindowId::MAIN, "buf-1"), Some(id));
    }

    #[test]
    fn take_warm_returns_none_when_pool_is_empty() {
        let m = PreviewWebviewManager::new();
        assert!(m.take_warm_for(WindowId::MAIN, "buf-1", ctype()).is_none());
    }

    #[test]
    fn close_clears_assignment() {
        let m = PreviewWebviewManager::new();
        m.record_warm_slot();
        m.take_warm_for(WindowId::MAIN, "buf-1", ctype());
        m.close(WindowId::MAIN, "buf-1");
        assert!(m.slot_for(WindowId::MAIN, "buf-1").is_none());
    }

    #[test]
    fn idle_policy_pauses_then_recycles() {
        let m = PreviewWebviewManager::new();
        m.record_warm_slot();
        let id = m
            .take_warm_for(WindowId::MAIN, "buf-1", ctype())
            .expect("warm slot");

        let t0 = Instant::now();
        // Just past pause threshold: paused.
        let transitions = m.apply_idle_policy(t0 + IDLE_PAUSE_AFTER + Duration::from_millis(1));
        assert_eq!(transitions.paused, vec![id]);
        assert!(transitions.recycled.is_empty());

        // Past recycle threshold from the same last_render baseline:
        // paused → recycled.
        let transitions = m.apply_idle_policy(t0 + IDLE_RECYCLE_AFTER + Duration::from_millis(1));
        assert!(transitions.paused.is_empty());
        assert_eq!(transitions.recycled, vec![id]);
        // Assignment is dropped on recycle.
        assert!(m.slot_for(WindowId::MAIN, "buf-1").is_none());
    }

    #[test]
    fn note_render_resets_idle_timer() {
        let m = PreviewWebviewManager::new();
        m.record_warm_slot();
        let id = m
            .take_warm_for(WindowId::MAIN, "buf-1", ctype())
            .expect("warm slot");
        let t0 = Instant::now();
        // Pause it.
        m.apply_idle_policy(t0 + IDLE_PAUSE_AFTER + Duration::from_millis(1));
        // Note a fresh render: state goes back to Active, timer resets.
        m.note_render(id);
        // A subsequent pass before the new pause threshold does not move
        // it.
        let transitions = m.apply_idle_policy(Instant::now() + Duration::from_secs(1));
        assert!(transitions.paused.is_empty());
        assert!(transitions.recycled.is_empty());
    }

    #[test]
    fn record_crash_marks_slot_recycled() {
        let m = PreviewWebviewManager::new();
        m.record_warm_slot();
        let id = m
            .take_warm_for(WindowId::MAIN, "buf-1", ctype())
            .expect("warm slot");
        m.record_crash(id);
        // Subsequent idle pass treats the slot as already-recycled and
        // doesn't double-transition.
        let transitions = m.apply_idle_policy(Instant::now() + Duration::from_secs(10 * 60));
        assert!(transitions.paused.is_empty());
        assert!(transitions.recycled.is_empty());
    }
}
