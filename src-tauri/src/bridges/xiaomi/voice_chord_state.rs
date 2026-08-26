//! 与平台无关的语音组合键状态机。
//!
//! Windows 注入层提供一个实际路由（虚拟 HID 或 SendInput）；此处保证记录实际
//! DOWN 的键位与路由、DOWN 失败立即补偿，以及 KEYUP 至多重试一次。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VoiceInjectionRoute {
    VirtualHid,
    SendInputFallback,
}

#[derive(Debug)]
struct HeldVoiceChord {
    keys: Vec<u16>,
    route: VoiceInjectionRoute,
}

#[derive(Default, Debug)]
pub(crate) struct VoiceChordState {
    held: Option<HeldVoiceChord>,
}

impl VoiceChordState {
    #[allow(dead_code)]
    pub(crate) const fn empty() -> Self {
        Self { held: None }
    }

    pub(crate) fn is_held(&self) -> bool {
        self.held.is_some()
    }

    pub(crate) fn held_route(&self) -> Option<VoiceInjectionRoute> {
        self.held.as_ref().map(|held| held.route)
    }

    pub(crate) fn press_with<F, C>(
        &mut self,
        keys: &[u16],
        mut inject_down: F,
        mut compensate: C,
    ) -> bool
    where
        F: FnMut(&[u16]) -> Option<VoiceInjectionRoute>,
        C: FnMut(&[u16]),
    {
        if self.held.is_some() {
            return false;
        }
        if let Some(route) = inject_down(keys) {
            self.held = Some(HeldVoiceChord {
                keys: keys.to_vec(),
                route,
            });
            true
        } else {
            // 任一路由可能已写入前半段，必须补一组反向 KEYUP。
            compensate(keys);
            false
        }
    }

    pub(crate) fn release_with<F>(
        &mut self,
        mut inject: F,
    ) -> Option<(Vec<u16>, VoiceInjectionRoute, bool)>
    where
        F: FnMut(&[u16], VoiceInjectionRoute) -> bool,
    {
        let held = self.held.take()?;
        let released = inject(&held.keys, held.route) || inject(&held.keys, held.route);
        Some((held.keys, held.route, released))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_original_keys_and_is_idempotent() {
        let keys = vec![0xA2, 0x5B];
        let mut state = VoiceChordState::default();
        assert!(state.press_with(
            &keys,
            |_keys| Some(VoiceInjectionRoute::VirtualHid),
            |_keys| {},
        ));
        assert_eq!(
            state.release_with(|_keys, route| route == VoiceInjectionRoute::VirtualHid),
            Some((keys, VoiceInjectionRoute::VirtualHid, true))
        );
        assert!(state.release_with(|_keys, _up| true).is_none());
    }

    #[test]
    fn partial_down_is_compensated() {
        let mut state = VoiceChordState::default();
        let mut compensated = false;
        assert!(!state.press_with(
            &[0xA2, 0x5B],
            |_keys| None,
            |_keys| compensated = true,
        ));
        assert!(compensated);
        assert!(!state.is_held());
    }

    #[test]
    fn release_retries_once() {
        let mut state = VoiceChordState::default();
        assert!(state.press_with(
            &[0xA2, 0x5B],
            |_keys| Some(VoiceInjectionRoute::SendInputFallback),
            |_keys| {},
        ));
        let mut attempts = 0;
        assert_eq!(
            state.release_with(|_keys, route| {
                assert_eq!(route, VoiceInjectionRoute::SendInputFallback);
                attempts += 1;
                attempts == 2
            }),
            Some((vec![0xA2, 0x5B], VoiceInjectionRoute::SendInputFallback, true))
        );
        assert_eq!(attempts, 2);
    }

    #[test]
    fn right_alt_space_releases_the_original_chord_once() {
        let keys = vec![0xA5, 0x20];
        let mut state = VoiceChordState::default();
        let mut calls = Vec::new();

        assert!(state.press_with(
            &keys,
            |injected| {
                calls.push((injected.to_vec(), VoiceInjectionRoute::VirtualHid));
                Some(VoiceInjectionRoute::VirtualHid)
            },
            |_keys| {},
        ));
        assert_eq!(
            state.release_with(|injected, route| {
                calls.push((injected.to_vec(), route));
                true
            }),
            Some((keys.clone(), VoiceInjectionRoute::VirtualHid, true))
        );
        assert!(state.release_with(|_injected, _route| true).is_none());
        assert_eq!(
            calls,
            vec![
                (keys.clone(), VoiceInjectionRoute::VirtualHid),
                (keys, VoiceInjectionRoute::VirtualHid),
            ]
        );
    }

    #[test]
    fn release_uses_the_route_selected_on_press() {
        let mut state = VoiceChordState::default();
        assert!(state.press_with(
            &[0xA5],
            |_keys| Some(VoiceInjectionRoute::VirtualHid),
            |_keys| {},
        ));
        assert_eq!(
            state.release_with(|_keys, route| route == VoiceInjectionRoute::VirtualHid),
            Some((vec![0xA5], VoiceInjectionRoute::VirtualHid, true))
        );
    }
}
