//! Presentation continuity only; never changes decoder state, timelines or error handling.
use super::player_hud::Busy;
use super::widgets::StatusKind;

const BUSY_GRACE_MS: u32 = 400;
const SEEK_HOLD_MS: u32 = 5_000;

#[derive(Default)]
pub(crate) struct State {
    pub(crate) duration: i64,
    pub(crate) position: i64,
    target: Option<(i64, u32)>,
    last_seek: Option<i64>,
    pub(crate) seen_picture: bool,
    busy_at: Option<u32>,
    quiet_busy: bool,
}

impl State {
    pub(crate) fn step(&mut self, now: u32, sample: Sample) {
        self.seen_picture |= sample.seen_picture;
        self.duration = self.duration.max(sample.duration);
        let seek = (sample.seek >= 0).then_some(sample.seek);
        if seek.is_some() && self.last_seek != seek {
            self.target = Some((sample.seek, now));
        }
        self.last_seek = seek;
        if let Some((target, at)) = self.target {
            if (!sample.loading && sample.position.abs_diff(target) < 2_000_000_000)
                || now.wrapping_sub(at) >= SEEK_HOLD_MS
                || sample.failed
            {
                self.target = None;
            }
        }
        self.position = if sample.scrub >= 0 {
            sample.scrub
        } else {
            self.target
                .map(|(target, _)| target)
                .unwrap_or(sample.position)
        };
        if sample.busy && !sample.failed {
            let since = *self.busy_at.get_or_insert(now);
            self.quiet_busy = self.seen_picture && now.wrapping_sub(since) < BUSY_GRACE_MS;
        } else {
            self.busy_at = None;
            self.quiet_busy = false;
        }
    }
}

pub(crate) struct Sample {
    pub(crate) duration: i64,
    pub(crate) position: i64,
    pub(crate) seek: i64,
    pub(crate) scrub: i64,
    pub(crate) loading: bool,
    pub(crate) busy: bool,
    pub(crate) failed: bool,
    pub(crate) seen_picture: bool,
}

impl State {
    pub(crate) fn timeline(&self) -> (i64, i64) {
        (self.position, self.duration)
    }
    pub(crate) fn busy(&self, actual: Busy) -> Busy {
        match actual {
            Busy::Readout(StatusKind::Failed, _) | Busy::None => actual,
            _ if self.quiet_busy => Busy::None,
            Busy::Readout(StatusKind::Working, _) if self.seen_picture => Busy::Transport,
            _ => actual,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sample {
        Sample {
            duration: 600_000_000_000,
            position: 10_000_000_000,
            seek: -1,
            scrub: -1,
            loading: false,
            busy: false,
            failed: false,
            seen_picture: true,
        }
    }

    #[test]
    fn timeline_does_not_reset_during_seek_reopen() {
        let mut state = State::default();
        state.step(0, sample());
        state.step(
            10,
            Sample {
                seek: 80_000_000_000,
                loading: true,
                busy: true,
                ..sample()
            },
        );
        state.step(
            20,
            Sample {
                duration: 0,
                position: 0,
                loading: true,
                busy: true,
                ..sample()
            },
        );
        assert_eq!(state.position, 80_000_000_000);
        assert_eq!(state.duration, 600_000_000_000);
        state.step(
            30,
            Sample {
                position: 80_100_000_000,
                ..sample()
            },
        );
        assert_eq!(state.position, 80_100_000_000);
        assert_eq!(state.target, None);
    }

    #[test]
    fn cursor_wins_while_previous_seek_is_pending() {
        let mut state = State::default();
        state.step(
            0,
            Sample {
                seek: 80_000_000_000,
                scrub: 120_000_000_000,
                loading: true,
                ..sample()
            },
        );
        assert_eq!(state.position, 120_000_000_000);
    }

    #[test]
    fn brief_busy_is_quiet_but_long_waits_and_errors_are_not() {
        let mut state = State::default();
        state.step(
            10,
            Sample {
                busy: true,
                ..sample()
            },
        );
        assert!(state.quiet_busy);
        state.step(
            410,
            Sample {
                busy: true,
                ..sample()
            },
        );
        assert!(!state.quiet_busy);
        state.step(
            411,
            Sample {
                failed: true,
                ..sample()
            },
        );
        assert!(!state.quiet_busy);
        let mut cold = State::default();
        cold.step(
            0,
            Sample {
                busy: true,
                seen_picture: false,
                ..sample()
            },
        );
        assert!(!cold.quiet_busy);
    }

    #[test]
    fn failed_seek_never_holds_a_fake_position_forever() {
        let mut state = State::default();
        state.step(
            0,
            Sample {
                seek: 80_000_000_000,
                loading: true,
                ..sample()
            },
        );
        state.step(
            5_000,
            Sample {
                seek: 80_000_000_000,
                loading: true,
                ..sample()
            },
        );
        assert_eq!(state.target, None);
        state.step(
            5_001,
            Sample {
                seek: 80_000_000_000,
                loading: true,
                ..sample()
            },
        );
        assert_eq!(state.target, None);
    }
}
