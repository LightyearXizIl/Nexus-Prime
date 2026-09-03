//! Coordination for moving the low-level keyboard hook to the front of the
//! Windows hook chain without waiting from the hook thread itself.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BumpOutcome {
    Settled,
    TimedOut,
    SelfDeadlock,
    NoHookThread,
}

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
static HANDLED_GENERATION: AtomicU64 = AtomicU64::new(0);
static LAST_REQUEST: Mutex<Option<u64>> = Mutex::new(None);

pub fn next_generation() -> u64 {
    let generation = NEXT_GENERATION.fetch_add(1, Ordering::AcqRel);
    *LAST_REQUEST.lock() = Some(generation);
    generation
}

pub fn mark_handled() {
    if let Some(generation) = *LAST_REQUEST.lock() {
        HANDLED_GENERATION.store(generation, Ordering::Release);
    }
}

pub fn wait_for(
    generation: u64,
    current_thread_id: u32,
    hook_thread_id: u32,
    settle_ms: u64,
) -> BumpOutcome {
    if hook_thread_id == 0 {
        return BumpOutcome::NoHookThread;
    }
    if current_thread_id == hook_thread_id {
        return BumpOutcome::SelfDeadlock;
    }
    let deadline = Instant::now() + Duration::from_millis(settle_ms.max(1));
    while Instant::now() < deadline {
        if HANDLED_GENERATION.load(Ordering::Acquire) >= generation {
            return BumpOutcome::Settled;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    BumpOutcome::TimedOut
}

#[cfg(test)]
pub fn reset_for_test() {
    NEXT_GENERATION.store(1, Ordering::Release);
    HANDLED_GENERATION.store(0, Ordering::Release);
    *LAST_REQUEST.lock() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_wait_from_the_hook_thread() {
        reset_for_test();
        assert_eq!(wait_for(1, 9, 9, 8), BumpOutcome::SelfDeadlock);
        assert_eq!(wait_for(1, 9, 0, 8), BumpOutcome::NoHookThread);
    }

    #[test]
    fn marks_the_requested_generation_as_settled() {
        reset_for_test();
        let generation = next_generation();
        mark_handled();
        assert_eq!(wait_for(generation, 1, 2, 8), BumpOutcome::Settled);
    }
}
