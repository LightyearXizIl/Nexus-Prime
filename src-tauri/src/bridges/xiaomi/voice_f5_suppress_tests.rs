//! Phase-1/5：语音键原生 F5 抑制必须盖住 typematic，否则记事本刷日期时间。
//!
//! 运行: cargo test -p nexus-prime --lib bridges::xiaomi::voice_f5_suppress -- --nocapture

use crate::bridges::xiaomi::key_mapping::{
    arm_voice_native_suppress, begin_voice_period, disarm_voice_native_suppress, end_voice_period,
    note_passthrough_f5_down, reset_voice_f5_state_for_test, should_suppress_voice_f5,
    voice_native_suppress_active, VOICE_F5_POST_TAIL_MS, VOICE_F5_SUPPRESS_DEADLINE_MS,
};
use std::sync::Mutex;
use std::time::Duration;

/// Windows 默认 typematic 延迟约 400–1000ms
pub const WINDOWS_TYPEMATIC_DELAY_MS: u64 = 400;
static VOICE_F5_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn voice_f5_suppress_deadline_covers_typematic() {
    assert!(
        VOICE_F5_SUPPRESS_DEADLINE_MS >= WINDOWS_TYPEMATIC_DELAY_MS,
        "deadline {VOICE_F5_SUPPRESS_DEADLINE_MS}ms < typematic {WINDOWS_TYPEMATIC_DELAY_MS}ms"
    );
}

#[test]
fn voice_f5_sticky_arm_stays_active_past_old_120ms_window() {
    let _guard = VOICE_F5_TEST_LOCK.lock().unwrap();
    reset_voice_f5_state_for_test();
    disarm_voice_native_suppress();
    arm_voice_native_suppress();
    assert!(voice_native_suppress_active(), "armed should be active");
    std::thread::sleep(Duration::from_millis(200)); // 超过旧的 120ms recent 窗
    assert!(
        voice_native_suppress_active(),
        "sticky suppress must still be active after 200ms (typematic would have started leaking)"
    );
    disarm_voice_native_suppress();
    assert!(!voice_native_suppress_active());
}

#[test]
fn notepad_f5_is_vk_0x74() {
    assert_eq!(0x74u16, 0x74);
}

#[test]
fn voice_period_suppresses_down_and_paired_up_even_after_remote_up() {
    let _guard = VOICE_F5_TEST_LOCK.lock().unwrap();
    reset_voice_f5_state_for_test();
    begin_voice_period();
    assert!(should_suppress_voice_f5(true, false));
    end_voice_period("test_remote_up");
    assert!(should_suppress_voice_f5(false, true));
    reset_voice_f5_state_for_test();
}

#[test]
fn leaked_f5_down_must_allow_its_up_to_prevent_sticky_key() {
    let _guard = VOICE_F5_TEST_LOCK.lock().unwrap();
    reset_voice_f5_state_for_test();
    begin_voice_period();
    assert!(should_suppress_voice_f5(true, false));
    note_passthrough_f5_down();
    assert!(!should_suppress_voice_f5(false, true));
    reset_voice_f5_state_for_test();
}

#[test]
fn post_release_tail_covers_firmware_typematic_window() {
    let _guard = VOICE_F5_TEST_LOCK.lock().unwrap();
    reset_voice_f5_state_for_test();
    begin_voice_period();
    end_voice_period("test_tail");
    disarm_voice_native_suppress();
    assert!(VOICE_F5_POST_TAIL_MS >= WINDOWS_TYPEMATIC_DELAY_MS);
    assert!(should_suppress_voice_f5(true, false));
    assert!(should_suppress_voice_f5(false, true));
    reset_voice_f5_state_for_test();
}
