//! Seek stills from existing PMS indexes. No full BIF download or media read.
use crate::plex::ServerId;
use crate::ui::{Painter, Rect};

const WIDTH: f32 = 288.0;
const HEIGHT: f32 = 162.0;
const REQUEST_MS: u32 = 250;
const REQUEST_TIMEOUT_MS: u32 = 1_500;
const LINGER_MS: u32 = 1_200;
const STEP_MS: i64 = 2_000;

#[derive(Default)]
pub(crate) struct Selection {
    item: Option<(ServerId, i64)>,
    requested: Option<i64>,
    ready: Option<i64>,
    cached: Vec<i64>,
    request_at: u32,
    scrub_at: u32,
    position: Option<i64>,
}

impl Selection {
    pub(crate) fn lingering(&self, now: u32) -> bool {
        self.item.is_some()
            && self.position.is_some()
            && now.wrapping_sub(self.scrub_at) < LINGER_MS
    }

    fn update(&mut self, item: Option<(ServerId, i64)>, position: Option<i64>, now: u32) -> bool {
        if self.item != item {
            *self = Self {
                item,
                ..Self::default()
            };
        }
        if item.is_none() {
            return false;
        }
        if let Some(position) = position {
            self.position = Some(position);
            self.scrub_at = now;
        } else if self.position.is_none() || now.wrapping_sub(self.scrub_at) >= LINGER_MS {
            return false;
        }
        // Coalesce remote repeats without hiding the last decoded image.
        let elapsed = now.wrapping_sub(self.request_at);
        if self.requested.is_none()
            || (self.requested != self.position
                && elapsed >= REQUEST_MS
                && (self.requested == self.ready || elapsed >= REQUEST_TIMEOUT_MS))
        {
            self.requested = self.position;
            self.request_at = now;
        }
        true
    }
}

fn timestamp(position_ns: i64, duration_ns: i64) -> Option<i64> {
    if position_ns < 0 || duration_ns <= 0 {
        return None;
    }
    let ms = position_ns.min(duration_ns.saturating_sub(1)) / 1_000_000;
    Some(ms / STEP_MS * STEP_MS)
}

pub(crate) fn draw(
    selection: &mut Selection,
    p: Painter,
    item: Option<(ServerId, i64)>,
    scrub_ns: i64,
    duration_ns: i64,
    now: u32,
    rail: Rect,
) {
    if duration_ns <= 0 || !selection.update(item, timestamp(scrub_ns, duration_ns), now) {
        return;
    }
    let Some((sid, part)) = item else { return };
    let Some(requested) = selection.requested else {
        return;
    };
    let pending_path = format!("/library/parts/{part}/indexes/sd/{requested}");
    if crate::ui::widgets::resolve_tex_on(sid, &pending_path, 288, 162, 0) != 0 {
        selection.ready = Some(requested);
        if !selection.cached.contains(&requested) {
            if selection.cached.len() == 24 {
                selection.cached.remove(0);
            }
            selection.cached.push(requested);
        }
    }
    let position = selection.position.unwrap_or(requested);
    let available = selection.cached.iter().copied().filter(|ms| {
        let path = format!("/library/parts/{part}/indexes/sd/{ms}");
        crate::ui::tex::peek_on(sid.raw(), &path, 288, 162, false) != 0
    });
    let display = nearest(available, position).unwrap_or(requested);
    let path = format!("/library/parts/{part}/indexes/sd/{display}");
    let fraction = (position as f64 * 1_000_000.0 / duration_ns as f64).clamp(0.0, 1.0) as f32;
    let x = (rail.x + fraction * rail.w - WIDTH * 0.5).clamp(rail.x, rail.x + rail.w - WIDTH);
    let frame = Rect::new(x, rail.y - 160.0 - HEIGHT, WIDTH, HEIGHT);
    crate::ui::widgets::draw_card(p, frame, sid, &path, (288, 162), 8.0, false, 1.0);
}

fn nearest(available: impl Iterator<Item = i64>, target: i64) -> Option<i64> {
    available.min_by_key(|ms| ms.abs_diff(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reversing_seek_uses_nearest_cached_frame_not_last_loaded() {
        assert_eq!(
            nearest([10_000, 30_000, 80_000].into_iter(), 28_000),
            Some(30_000)
        );
        assert_eq!(
            nearest([10_000, 30_000, 80_000].into_iter(), 12_000),
            Some(10_000)
        );
        assert_eq!(nearest([].into_iter(), 12_000), None);
    }

    #[test]
    fn preview_uses_milliseconds_buckets_and_clamps_end() {
        assert_eq!(timestamp(60_500_000_000, 120_000_000_000), Some(60_000));
        assert_eq!(timestamp(130_000_000_000, 120_000_000_000), Some(118_000));
        assert_eq!(timestamp(-1, 120_000_000_000), None);
        assert_eq!(timestamp(0, 0), None);
    }

    #[test]
    fn repeated_seek_keeps_last_image_and_coalesces_requests() {
        let mut selection = Selection::default();
        let item = Some((ServerId::UNSET, 17));
        assert!(selection.update(item, Some(60_000), 0));
        selection.ready = Some(60_000);
        for now in (20..250).step_by(20) {
            assert!(selection.update(item, Some(62_000), now));
            assert_eq!(selection.requested, Some(60_000));
            assert_eq!(selection.ready, Some(60_000));
        }
        assert!(selection.update(item, Some(64_000), 250));
        assert_eq!(selection.requested, Some(64_000));
        assert_eq!(selection.ready, Some(60_000));
        assert!(selection.update(item, Some(66_000), 550));
        assert_eq!(selection.requested, Some(64_000));
        selection.ready = selection.requested;
        assert!(selection.update(item, Some(68_000), 600));
        assert_eq!(selection.requested, Some(68_000));
    }

    #[test]
    fn committed_taps_linger_but_never_leak_to_another_item() {
        let mut selection = Selection::default();
        let item = Some((ServerId::UNSET, 17));
        assert!(selection.update(item, Some(60_000), 100));
        assert!(selection.update(item, None, 1_299));
        assert!(!selection.update(item, None, 1_300));
        assert!(!selection.update(Some((ServerId::UNSET, 18)), None, 1_301));
        assert_eq!(selection.ready, None);
        assert!(!selection.update(None, Some(0), 1_302));
    }

    #[test]
    fn a_slow_or_failed_index_does_not_starve_new_targets() {
        let mut selection = Selection::default();
        let item = Some((ServerId::UNSET, 17));
        assert!(selection.update(item, Some(60_000), u32::MAX - 100));
        assert!(selection.update(item, Some(80_000), 1_500));
        assert_eq!(selection.requested, Some(80_000));
    }

    #[test]
    fn missing_index_or_invalid_part_never_requests_preview() {
        for raw in [
            r#"{"id":17}"#,
            r#"{"id":0,"indexes":"sd"}"#,
            r#"{"id":17,"indexes":"hd"}"#,
        ] {
            let part: crate::plex::MediaPart = serde_json::from_str(raw).unwrap();
            assert_eq!(part.preview_part_id(), None);
        }
        let part: crate::plex::MediaPart =
            serde_json::from_str(r#"{"id":17,"indexes":"sd"}"#).unwrap();
        assert_eq!(part.preview_part_id(), Some(17));
    }
}
