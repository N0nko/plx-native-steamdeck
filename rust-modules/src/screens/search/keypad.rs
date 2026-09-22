//! Remote keyboard geometry/actions. Focus is owned by the upstream FocusEngine.
use crate::ui::Rect;
pub(super) const BASE: u32 = 0x7000;
pub(super) const FILTER_BASE: u32 = 0x7100;
pub(super) const LABELS: [&str; 6] = ["ABC", "123", "Space", "Delete", "System", "Done"];
pub(super) fn len(numeric: bool) -> usize {
    if numeric {
        18
    } else {
        42
    }
}
pub(super) fn columns(numeric: bool) -> usize {
    if numeric {
        3
    } else {
        6
    }
}
pub(super) fn label(index: usize, numeric: bool) -> String {
    if index < 6 {
        return LABELS[index].into();
    }
    if numeric {
        return [
            "1 .?!", "2 abc", "3 def", "4 ghi", "5 jkl", "6 mno", "7 pqrs", "8 tuv", "9 wxyz",
            "Space", "0", "Delete",
        ][index - 6]
            .into();
    }
    (b"abcdefghijklmnopqrstuvwxyz1234567890"[index - 6] as char).to_string()
}
pub(super) fn rect(index: usize, numeric: bool) -> Rect {
    let cols = if index < 6 { 3 } else { columns(numeric) };
    let w = 600.0 / cols as f32;
    let row = if index < 6 {
        index / 3
    } else {
        2 + (index - 6) / cols
    };
    let col = if index < 6 {
        index % 3
    } else {
        (index - 6) % cols
    };
    Rect::new(
        96.0 + col as f32 * w,
        360.0 + row as f32 * 82.0,
        w - 10.0,
        68.0,
    )
}
pub(super) fn filter_rect(index: usize) -> Rect {
    Rect::new(96.0 + index as f32 * 260.0, 278.0, 244.0, 60.0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_key_is_on_screen_and_labels_cover_both_layouts() {
        for numeric in [false, true] {
            for i in 0..len(numeric) {
                let r = rect(i, numeric);
                assert!(r.x >= 96.0 && r.x + r.w <= 696.0 && r.y + r.h <= 1080.0);
                assert!(!label(i, numeric).is_empty());
            }
        }
    }
}

pub(super) fn next(index: usize, dir: crate::ui::screen::Dir, numeric: bool) -> Option<usize> {
    use crate::ui::screen::Dir;
    let from = rect(index, numeric);
    (0..len(numeric))
        .filter(|i| *i != index)
        .filter_map(|i| {
            let to = rect(i, numeric);
            let dx = to.cx() - from.cx();
            let dy = to.cy() - from.cy();
            let score = match dir {
                Dir::Left if dy == 0.0 && dx < 0.0 => -dx,
                Dir::Right if dy == 0.0 && dx > 0.0 => dx,
                Dir::Up if dy < 0.0 => -dy * 100.0 + dx.abs(),
                Dir::Down if dy > 0.0 => dy * 100.0 + dx.abs(),
                _ => return None,
            };
            Some((i, score))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|x| x.0)
}
