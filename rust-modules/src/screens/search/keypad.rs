//! Remote keyboard geometry/actions. Focus is owned by the upstream FocusEngine.
use crate::ui::Rect;
pub(super) const BASE: u32 = 0x7000;
pub(super) const FILTER_BASE: u32 = 0x7100;
pub(super) const FIRST_KEY: usize = 2;
pub(super) const WIDTH: f32 = 396.0;
pub(super) const TOP: f32 = 360.0;
const STEP: f32 = 56.0;
const HEIGHT: f32 = 48.0;
pub(super) fn len(numeric: bool) -> usize { if numeric { 11 } else { 38 } }
pub(super) fn columns(numeric: bool) -> usize { if numeric { 3 } else { 6 } }
pub(super) fn label(index: usize, numeric: bool) -> String {
    if numeric { return ["1 ⌫", "2 abc", "3 def", "4 ghi", "5 jkl", "6 mno",
        "7 pqrs", "8 tuv", "9 wxyz", "0 ␣ / −", "ABC"][index].into(); }
    match index {
        0 => "Space".into(),
        1 => "⌫".into(),
        _ => (b"ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890"[index-FIRST_KEY] as char).to_string(),
    }
}
pub(super) fn input(index: usize) -> char {
    b"abcdefghijklmnopqrstuvwxyz1234567890"[index-FIRST_KEY] as char
}
pub(super) fn rect(index: usize, numeric: bool) -> Rect {
    if numeric {let cell=if index>=9 {index+1}else{index};return Rect::new(96.0+(cell%3) as f32*(WIDTH/3.0),TOP+(cell/3) as f32*STEP,WIDTH/3.0-8.0,HEIGHT);}
    let cols = columns(numeric);
    let (row,col,width) = if index<FIRST_KEY {(0,index,WIDTH/2.0)} else {
        let key=index-FIRST_KEY;
        (1+key/cols,if numeric && key==9 {1} else {key%cols},WIDTH/cols as f32)
    };
    Rect::new(96.0+col as f32*width,TOP+row as f32*STEP,width-8.0,HEIGHT)
}
pub(super) fn extent(numeric: bool) -> Rect {
    Rect::new(96.0,TOP,WIDTH-8.0,rect(len(numeric)-1,numeric).y+HEIGHT-TOP)
}
pub(super) fn filter_rect(index: usize) -> Rect {
    let width=192.0;let gap=8.0;
    Rect::new((1920.0-(3.0*width+2.0*gap))/2.0+index as f32*(width+gap),278.0,width,HEIGHT)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_keyboard_and_centered_filters() {
        assert_eq!(len(false),38);assert_eq!(len(true),11);
        assert_eq!(label(0,true),"1 ⌫");assert_eq!(label(10,true),"ABC");
        for i in 0..len(true) {let cell=if i>=9{i+1}else{i};let row=cell/3;let col=cell%3;
            let key=rect(i,true);
            assert_eq!(key.w,124.0);assert_eq!(key.h,48.0);
            assert_eq!(key.x,96.0+col as f32*132.0);assert_eq!(key.y,360.0+row as f32*56.0);
        }
        let first = filter_rect(0);
        let last = filter_rect(2);
        assert_eq!((first.x + last.x + last.w) / 2.0, 960.0);
        for numeric in [false, true] {
            for i in 0..len(numeric) {
                let r = rect(i, numeric);
                assert!(r.x + r.w <= 484.0 && r.y + r.h <= 744.0);
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
