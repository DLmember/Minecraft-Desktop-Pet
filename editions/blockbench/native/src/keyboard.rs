//! Windows hook events own key state. Async polling only recovers a lost key-up.
//! A low-level hook runs BEFORE Windows updates GetAsyncKeyState; even a posted
//! message can reach our loop while a later hook in the chain is still running.
//! Never infer a release until polling has confirmed that particular press.

const RELEASE_CONFIRM_SECONDS: f64 = 0.06;

#[derive(Default)]
struct Press {
    vk: u32,
    observed_down: bool,
    up_since: Option<f64>,
}

#[derive(Default)]
pub struct KeyRecovery {
    presses: Vec<Press>,
}

pub fn physical_key(vk: u32) -> u32 {
    if vk == 0x1000 {
        13
    } else {
        vk
    }
}

impl KeyRecovery {
    pub fn event(&mut self, vk: u32, down: bool) {
        if !down {
            self.presses.retain(|p| p.vk != vk);
        } else if let Some(press) = self.presses.iter_mut().find(|p| p.vk == vk) {
            // A repeat is further evidence of a held key, not a new press.
            press.up_since = None;
        } else {
            self.presses.push(Press {
                vk,
                ..Press::default()
            });
        }
    }

    pub fn clear(&mut self) {
        self.presses.clear();
    }

    pub fn releases(
        &mut self,
        held: &[u32],
        now: f64,
        mut is_down: impl FnMut(u32) -> bool,
    ) -> Vec<u32> {
        self.presses.retain(|p| held.contains(&p.vk));
        let mut released = Vec::new();
        for press in &mut self.presses {
            if is_down(physical_key(press.vk)) {
                press.observed_down = true;
                press.up_since = None;
            } else if press.observed_down {
                let since = *press.up_since.get_or_insert(now);
                if now - since >= RELEASE_CONFIRM_SECONDS {
                    released.push(press.vk);
                }
            }
        }
        self.presses.retain(|p| !released.contains(&p.vk));
        released
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Settings,
        render::{Pose, Skin, DEFAULT_SKIN},
    };
    use glam::Vec2;

    #[test]
    fn initial_hook_down_survives_stale_async_state_without_waiting_for_repeat() {
        let mut keys = KeyRecovery::default();
        keys.event(81, true);
        // Correctness does not depend on a guessed hook scheduling delay.
        for now in [0., 0.005, 0.15, 0.5, 1.5] {
            assert!(keys.releases(&[81], now, |_| false).is_empty());
        }
        assert!(keys.releases(&[81], 1.6, |_| true).is_empty());
    }

    #[test]
    fn missing_keyup_is_recovered_only_after_confirmed_down_and_stable_up() {
        let mut keys = KeyRecovery::default();
        keys.event(81, true);
        assert!(keys.releases(&[81], 0., |_| true).is_empty());
        assert!(keys.releases(&[81], 0.1, |_| false).is_empty());
        assert!(keys.releases(&[81], 0.13, |_| false).is_empty());
        assert_eq!(keys.releases(&[81], 0.161, |_| false), vec![81]);
        assert!(keys.releases(&[81], 0.2, |_| false).is_empty());
    }

    #[test]
    fn real_keyup_and_repress_do_not_inherit_old_poll_confirmation() {
        let mut keys = KeyRecovery::default();
        keys.event(81, true);
        keys.releases(&[81], 0., |_| true);
        keys.event(81, false);
        assert!(keys.presses.is_empty());
        keys.event(81, true);
        assert!(keys.releases(&[81], 0.01, |_| false).is_empty());
        assert!(keys.releases(&[81], 0.2, |_| false).is_empty());
    }

    #[test]
    fn transient_zero_or_repeat_cancels_pending_recovery() {
        let mut keys = KeyRecovery::default();
        keys.event(81, true);
        keys.releases(&[81], 0., |_| true);
        keys.releases(&[81], 0.1, |_| false);
        assert!(keys.releases(&[81], 0.14, |_| true).is_empty());
        assert!(keys.releases(&[81], 0.17, |_| false).is_empty());
        keys.event(81, true);
        assert!(keys.releases(&[81], 0.24, |_| false).is_empty());
        assert_eq!(keys.releases(&[81], 0.31, |_| false), vec![81]);
    }

    #[test]
    fn reset_prunes_keys_and_preserves_extended_enter_identity() {
        let mut keys = KeyRecovery::default();
        for vk in [13, 0x1000, 81] {
            keys.event(vk, true);
        }
        let mut physical = Vec::new();
        keys.releases(&[13, 0x1000], 0., |vk| {
            physical.push(vk);
            true
        });
        assert_eq!(physical, vec![13, 13]);
        assert_eq!(keys.presses.len(), 2);
        keys.event(13, false);
        assert_eq!(keys.presses[0].vk, 0x1000);
        keys.clear();
        assert!(keys.presses.is_empty());
    }

    #[test]
    fn windows_polling_preserves_arms_and_one_squash_before_first_repeat() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        for posture in ["sitting", "custom"] {
            for mode in ["precise", "alternating"] {
                for fps in [30., 60., 90., 120., 144., 165., 240.] {
                    for vk in [81, 80, 0xA0, 13, 0x1000] {
                        let mut settings = Settings::default();
                        settings.posture = posture.into();
                        settings.custom_base = "sitting".into();
                        settings.typing_mode = mode.into();
                        let mut actual = Pose::new(&settings);
                        let mut expected = Pose::new(&settings);
                        let mut keys = KeyRecovery::default();
                        actual.key(vk, true, 0., &settings);
                        expected.key(vk, true, 0., &settings);
                        keys.event(vk, true);
                        for frame in 0..(fps as usize * 2) {
                            let now = frame as f32 / fps;
                            if frame == fps as usize {
                                keys.event(vk, true);
                                actual.key(vk, true, now, &settings);
                                expected.key(vk, true, now, &settings);
                            }
                            for key in
                                keys.releases(&actual.held_codes(), now as f64, |_| now >= 0.12)
                            {
                                actual.key(key, false, now, &settings);
                            }
                            for pose in [&mut actual, &mut expected] {
                                pose.update(
                                    &settings,
                                    &skin,
                                    Vec2::ZERO,
                                    360.,
                                    420.,
                                    false,
                                    now,
                                    1. / fps,
                                );
                            }
                            assert_eq!(actual.held_codes(), vec![vk]);
                            assert_eq!(actual.arms, expected.arms);
                            assert_eq!(actual.squash, expected.squash);
                            if now > 0.3 {
                                assert_eq!(actual.squash, 1.);
                            }
                        }
                        keys.event(vk, false);
                        actual.key(vk, false, 2., &settings);
                        assert!(actual.held_codes().is_empty());
                        assert!(keys.presses.is_empty());
                    }
                }
            }
        }
    }
}
