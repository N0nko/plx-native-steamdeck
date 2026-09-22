//! Remote keyboard geometry/actions. Focus is owned by the upstream FocusEngine.
use crate::ui::Rect;
pub(super) const BASE: u32 = 0x7000;
pub(super) const FILTER_BASE: u32 = 0x7100;
pub(super) const FIRST_KEY: usize = 5;
pub(super) const WIDTH: f32 = 480.0;
pub(super) const TOP: f32 = 360.0;
const STEP: f32 = 64.0;
const HEIGHT: f32 = 56.0;
pub(super) fn len(numeric: bool) -> usize { FIRST_KEY + if numeric { 10 } else { 36 } }
pub(super) fn columns(numeric: bool) -> usize { if numeric { 3 } else { 6 } }
pub(super) fn label(index: usize, numeric: bool) -> String {
    match index {
        0 => if numeric { "ABC" } else { "123" }.into(),
        1 => "Space".into(),
        2 => "⌫".into(),
        3 => "More".into(),
        4 => "Done".into(),
        _ if numeric => ["1 .?!", "2 abc", "3 def", "4 ghi", "5 jkl", "6 mno",
            "7 pqrs", "8 tuv", "9 wxyz", "0"][index-FIRST_KEY].into(),
        _ => (b"ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890"[index-FIRST_KEY] as char).to_string(),
    }
}
pub(super) fn input(index: usize) -> char {
    b"abcdefghijklmnopqrstuvwxyz1234567890"[index-FIRST_KEY] as char
}
pub(super) fn rect(index: usize, numeric: bool) -> Rect {
    let cols = columns(numeric);
    let rows = if numeric { 4 } else { 6 };
    let (row, col, width) = match index {
        1 | 2 => (0, index-1, WIDTH/2.0),
        0 => (rows+1, 0, WIDTH/3.0),
        3 | 4 => (rows+1, index-2, WIDTH/3.0),
        _ => {
            let key=index-FIRST_KEY;
            (1+key/cols, if numeric && key==9 {1} else {key%cols}, WIDTH/cols as f32)
        }
    };
    Rect::new(96.0+col as f32*width, TOP+row as f32*STEP, width-8.0, HEIGHT)
}
pub(super) fn extent(numeric: bool) -> Rect {
    Rect::new(96.0,TOP,WIDTH-8.0,rect(4,numeric).y+HEIGHT-TOP)
}
pub(super) fn filter_rect(index: usize) -> Rect {
    let width=244.0; let gap=16.0;
    Rect::new((1920.0-(3.0*width+2.0*gap))/2.0+index as f32*(width+gap),278.0,width,60.0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_keyboard_and_centered_filters() {
        let first = filter_rect(0);
        let last = filter_rect(2);
        assert_eq!((first.x + last.x + last.w) / 2.0, 960.0);
        for numeric in [false, true] {
            for i in 0..len(numeric) {
                let r = rect(i, numeric);
                assert!(r.x + r.w <= 576.0 && r.y + r.h <= 872.0);
            }
        }
    }
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
