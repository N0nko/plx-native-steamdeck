//! Remote digit composition, independent of the TV's on-screen keyboard.
#[derive(Default)]
pub(super) struct MultiTap {
    pending: Option<(u8, usize, u32)>,
    echo: Option<(u8, u32)>,
}
impl MultiTap {
    pub(super) fn write(&self, c: &mut crate::ui::machine::Canon) {
        c.option(self.pending, |c, (digit, index, time)| {
            c.u32(digit as u32).u64(index as u64).u32(time);
        });
        c.option(self.echo, |c, (digit, time)| {
            c.u32(digit as u32).u32(time);
        });
    }
    pub(super) const fn new() -> Self {
        Self {
            pending: None,
            echo: None,
        }
    }
    pub(super) fn cancel(&mut self) {
        self.pending = None;
    }
    pub(super) fn press(&mut self, digit: u8, now: u32) -> Option<(bool, char)> {
        let letters = *[
            " 0", ".,?!1", "abc2", "def3", "ghi4", "jkl5", "mno6", "pqrs7", "tuv8", "wxyz9",
        ]
        .get(digit as usize)?;
        let same = self
            .pending
            .filter(|(d, _, t)| *d == digit && now.wrapping_sub(*t) < 850);
        let index = same.map_or(0, |(_, i, _)| (i + 1) % letters.len());
        self.pending = Some((digit, index, now));
        self.echo = Some((digit, now));
        Some((same.is_some(), letters.as_bytes()[index] as char))
    }
    pub(super) fn eat_echo(&mut self, text: &str, now: u32) -> bool {
        if self
            .echo
            .is_some_and(|(d, t)| now.wrapping_sub(t) < 300 && text.as_bytes() == [b'0' + d])
        {
            self.echo = None;
            true
        } else {
            false
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cycles_commits_and_handles_clock_wrap() {
        let mut m = MultiTap::default();
        assert_eq!(m.press(2, 10), Some((false, 'a')));
        assert_eq!(m.press(2, 100), Some((true, 'b')));
        assert_eq!(m.press(2, 200), Some((true, 'c')));
        assert_eq!(m.press(3, 300), Some((false, 'd')));
        assert_eq!(m.press(3, 1200), Some((false, 'd')));
        m.cancel();
        assert_eq!(m.press(9, u32::MAX - 20), Some((false, 'w')));
        assert_eq!(m.press(9, 10), Some((true, 'x')));
        assert_eq!(m.press(10, 20), None);
    }
    #[test]
    fn only_matching_immediate_digit_echo_is_dropped_once() {
        let mut m = MultiTap::default();
        m.press(2, 100);
        assert!(!m.eat_echo("a", 101));
        assert!(m.eat_echo("2", 102));
        assert!(!m.eat_echo("2", 103));
        m.press(3, 200);
        assert!(!m.eat_echo("3", 600));
    }
}
