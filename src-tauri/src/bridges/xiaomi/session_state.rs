//! 与平台无关的连接会话令牌。
//!
//! 一个桥接 worker 可跨多次重连存活；每次设备连接都有独立会话号，旧回调无法
//! 结束新会话。

use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct SessionState {
    sequence: AtomicU64,
    active: AtomicU64,
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionState {
    pub(crate) const fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            active: AtomicU64::new(0),
        }
    }

    pub(crate) fn begin(&self) -> u64 {
        let id = self.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        self.active.store(id, Ordering::Release);
        id
    }

    pub(crate) fn is_active(&self, id: u64) -> bool {
        self.active.load(Ordering::Acquire) == id
    }

    pub(crate) fn end(&self, id: u64) -> bool {
        self.active
            .compare_exchange(id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn cancel(&self) -> u64 {
        self.active.swap(0, Ordering::AcqRel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_callback_cannot_end_new_session() {
        let state = SessionState::new();
        let first = state.begin();
        let second = state.begin();
        assert!(!state.end(first));
        assert!(state.is_active(second));
        assert!(state.end(second));
    }

    #[test]
    fn cancel_invalidates_current_session() {
        let state = SessionState::new();
        let session = state.begin();
        assert_eq!(state.cancel(), session);
        assert!(!state.is_active(session));
    }
}
