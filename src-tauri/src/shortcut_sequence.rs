//! Recognize two distinct Ctrl+C presses without consuming ordinary copy input.

#[derive(Default)]
pub struct ModifierKeys(u8);

impl ModifierKeys {
    pub fn update(&mut self, key: u32, down: bool) {
        let bit = match key {
            0x11 | 0xa2 => 1,
            0xa3 => 2,
            0x10 | 0xa0 => 4,
            0xa1 => 8,
            0x12 | 0xa4 => 16,
            0xa5 => 32,
            0x5b => 64,
            0x5c => 128,
            _ => 0,
        };
        if down {
            self.0 |= bit;
        } else {
            self.0 &= !bit;
        }
    }
    pub fn control(&self) -> bool {
        self.0 & 3 != 0
    }
    pub fn other(&self) -> bool {
        self.0 & !3 != 0
    }
    pub fn snip(&self) -> bool {
        self.0 & 192 != 0 && self.0 & 12 != 0 && self.0 & 51 == 0
    }
}

#[derive(Default)]
pub struct DoubleCopyDetector {
    first: Option<(u32, usize)>,
    c_down: bool,
}

impl DoubleCopyDetector {
    #[allow(clippy::too_many_arguments)]
    pub fn observe(
        &mut self,
        key: u32,
        down: bool,
        control: bool,
        other_modifier: bool,
        own_input: bool,
        timestamp: u32,
        window: usize,
    ) -> bool {
        if own_input {
            return false;
        }
        if key != 0x43 {
            if down && !matches!(key, 0x11 | 0xa2 | 0xa3) {
                self.first = None;
            }
            return false;
        }
        if !down {
            self.c_down = false;
            return false;
        }
        if std::mem::replace(&mut self.c_down, true) {
            return false;
        }
        if !control || other_modifier || window == 0 {
            self.first = None;
            return false;
        }
        if let Some((time, source)) = self.first.take()
            && source == window
            && timestamp.wrapping_sub(time) <= 500
        {
            return true;
        }
        self.first = Some((timestamp, window));
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(detector: &mut DoubleCopyDetector, down: bool, time: u32, window: usize) -> bool {
        detector.observe(0x43, down, true, false, false, time, window)
    }
    #[test]
    fn modifiers_follow_hook_events_even_before_async_key_state_is_updated() {
        let mut keys = ModifierKeys::default();
        let mut detector = DoubleCopyDetector::default();
        keys.update(0xa2, true);
        assert!(!detector.observe(0x43, true, keys.control(), keys.other(), false, 10, 1));
        detector.observe(0x43, false, keys.control(), keys.other(), false, 20, 1);
        keys.update(0xa3, true);
        keys.update(0xa2, false);
        assert!(detector.observe(0x43, true, keys.control(), keys.other(), false, 150, 1));
        keys.update(0xa3, false);
        keys.update(0x5b, true);
        keys.update(0xa0, true);
        assert!(keys.snip());
        keys.update(0xa4, true);
        assert!(!keys.snip());
    }
    #[test]
    fn requires_two_distinct_presses_and_does_not_repeat_on_hold() {
        let mut detector = DoubleCopyDetector::default();
        assert!(!key(&mut detector, true, 10, 1));
        assert!(!key(&mut detector, true, 20, 1));
        assert!(!key(&mut detector, false, 30, 1));
        assert!(key(&mut detector, true, 300, 1));
        assert!(!key(&mut detector, true, 330, 1));
    }
    #[test]
    fn own_input_other_keys_timeouts_and_window_changes_do_not_trigger() {
        for scenario in 0..5 {
            let mut detector = DoubleCopyDetector::default();
            key(&mut detector, true, 0, 1);
            key(&mut detector, false, 10, 1);
            match scenario {
                0 => assert!(!detector.observe(0x43, true, true, false, true, 100, 1)),
                1 => {
                    detector.observe(0x41, true, true, false, false, 100, 1);
                    assert!(!key(&mut detector, true, 200, 1));
                }
                2 => assert!(!key(&mut detector, true, 501, 1)),
                3 => assert!(!key(&mut detector, true, 100, 2)),
                _ => assert!(!detector.observe(0x43, true, true, true, false, 100, 1)),
            }
        }
    }
    #[test]
    fn supports_releasing_control_between_copies_and_tick_count_wrap() {
        let mut detector = DoubleCopyDetector::default();
        key(&mut detector, true, u32::MAX - 200, 1);
        key(&mut detector, false, u32::MAX - 100, 1);
        detector.observe(0x11, false, false, false, false, 0, 1);
        detector.observe(0x11, true, false, false, false, 50, 1);
        assert!(key(&mut detector, true, 100, 1));
    }
}
