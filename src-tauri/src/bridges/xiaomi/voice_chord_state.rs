//! 与平台无关的语音组合键状态机。
//!
//! Windows 注入层只提供 `inject(keys, key_up)`；此处保证记录实际 DOWN 的键位、
//! DOWN 失败立即补偿，以及 KEYUP 至多重试一次。

#[derive(Default, Debug)]
pub(crate) struct VoiceChordState {
    held: Option<Vec<u16>>,
}

impl VoiceChordState {
    #[allow(dead_code)]
    pub(crate) const fn empty() -> Self {
        Self { held: None }
    }

    pub(crate) fn is_held(&self) -> bool {
        self.held.is_some()
    }

    pub(crate) fn press_with<F>(&mut self, keys: &[u16], mut inject: F) -> bool
    where
        F: FnMut(&[u16], bool) -> bool,
    {
        if self.held.is_some() {
            return false;
        }
        if inject(keys, false) {
            self.held = Some(keys.to_vec());
            true
        } else {
            // SendInput 可能已写入前半段，必须补一组反向 KEYUP。
            let _ = inject(keys, true);
            false
        }
    }

    pub(crate) fn release_with<F>(&mut self, mut inject: F) -> Option<(Vec<u16>, bool)>
    where
        F: FnMut(&[u16], bool) -> bool,
    {
        let keys = self.held.take()?;
        let released = inject(&keys, true) || inject(&keys, true);
        Some((keys, released))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_original_keys_and_is_idempotent() {
        let keys = vec![0xA2, 0x5B];
        let mut state = VoiceChordState::default();
        assert!(state.press_with(&keys, |_keys, _up| true));
        assert_eq!(state.release_with(|_keys, _up| true), Some((keys, true)));
        assert!(state.release_with(|_keys, _up| true).is_none());
    }

    #[test]
    fn partial_down_is_compensated() {
        let mut state = VoiceChordState::default();
        let mut phases = Vec::new();
        assert!(!state.press_with(&[0xA2, 0x5B], |_keys, up| {
            phases.push(up);
            false
        }));
        assert_eq!(phases, vec![false, true]);
        assert!(!state.is_held());
    }

    #[test]
    fn release_retries_once() {
        let mut state = VoiceChordState::default();
        assert!(state.press_with(&[0xA2, 0x5B], |_keys, _up| true));
        let mut attempts = 0;
        assert_eq!(
            state.release_with(|_keys, up| {
                assert!(up);
                attempts += 1;
                attempts == 2
            }),
            Some((vec![0xA2, 0x5B], true))
        );
        assert_eq!(attempts, 2);
    }

    #[test]
    fn right_alt_space_releases_the_original_chord_once() {
        let keys = vec![0xA5, 0x20];
        let mut state = VoiceChordState::default();
        let mut calls = Vec::new();

        assert!(state.press_with(&keys, |injected, up| {
            calls.push((injected.to_vec(), up));
            true
        }));
        assert_eq!(state.release_with(|injected, up| {
            calls.push((injected.to_vec(), up));
            true
        }), Some((keys.clone(), true)));
        assert!(state.release_with(|_injected, _up| true).is_none());
        assert_eq!(calls, vec![(keys.clone(), false), (keys, true)]);
    }
}
