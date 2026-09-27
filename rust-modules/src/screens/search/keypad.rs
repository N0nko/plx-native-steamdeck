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
fn label_run(index: usize, numeric: bool) -> &'static std::ffi::CStr {
    const FULL: [&std::ffi::CStr; 38] = [
        c"Space", c"⌫", c"A", c"B", c"C", c"D", c"E", c"F", c"G", c"H", c"I", c"J",
        c"K", c"L", c"M", c"N", c"O", c"P", c"Q", c"R", c"S", c"T", c"U", c"V",
        c"W", c"X", c"Y", c"Z", c"1", c"2", c"3", c"4", c"5", c"6", c"7", c"8", c"9", c"0",
    ];
    const T9: [&std::ffi::CStr; 11] = [c"1 ⌫", c"2 abc", c"3 def", c"4 ghi", c"5 jkl",
        c"6 mno", c"7 pqrs", c"8 tuv", c"9 wxyz", c"0 ␣ / −", c"ABC"];
    if numeric { T9[index] } else { FULL[index] }
}
pub(super) fn input(index: usize) -> char {
    b"abcdefghijklmnopqrstuvwxyz1234567890"[index-FIRST_KEY] as char
}
pub(super) fn rect(index: usize, numeric: bool) -> Rect {
    if numeric {let cell=match index {9=>10,10=>9,_=>index};return Rect::new(96.0+(cell%3) as f32*(WIDTH/3.0),TOP+(cell/3) as f32*STEP,WIDTH/3.0-8.0,HEIGHT);}
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

pub(super) fn draw(p: crate::ui::Painter, numeric: bool, focused: Option<usize>) {
    use crate::ui::theme;
    use crate::ui::widgets::{Button, ControlStyle};
    // Bounded stack storage and static labels: drawing the full grid does not allocate a
    // String/Vec per key or manufacture a separate blur chain for any of its 38 buttons.
    let mut buttons: [Option<Button>; 38] = std::array::from_fn(|_| None);
    for (i, slot) in buttons.iter_mut().take(len(numeric)).enumerate() {
        *slot = Some(Button::new(label_run(i, numeric).as_ptr(), theme::size::LABEL, rect(i, numeric))
            .style(ControlStyle::Glass)
            .focused(focused == Some(i)));
    }
    Button::draw_group(buttons.iter().flatten(), p);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_glass_shares_one_source_and_reflects_neighboring_focus() {
        use crate::ui::{frame::backdrop, theme, Painter};
        use std::{cell::RefCell, rc::Rc};
        let _serial = crate::testlock::serial();
        for numeric in [false, true] {
            let sources = Rc::new(RefCell::new(backdrop::Sources::default()));
            for (frame, focused) in [4, 5, 5].into_iter().enumerate() {
                sources.borrow_mut().begin(vec![]);
                {
                    let _walk = backdrop::discover(sources.clone());
                    let p = Painter::root();
                    p.rect(Rect::FULL, 0.0, theme::SURFACE_APP, theme::SURFACE_APP, 0.0);
                    draw(p, numeric, Some(focused));
                }
                sources.borrow_mut().resolve();
                assert_eq!(sources.borrow().jobs().len(), usize::from(frame != 2),
                    "numeric={numeric}, frame={frame}: one shared source must refresh for neighboring focused-key light, then rest");
                sources.borrow_mut().retain_declared_for_test();
            }
        }
    }
    #[test]
    fn compact_keyboard_and_centered_filters() {
        assert_eq!(len(false),38);assert_eq!(len(true),11);
        assert_eq!(label_run(0,true),c"1 ⌫");assert_eq!(label_run(10,true),c"ABC");
        for index in FIRST_KEY..len(false) {
            assert_eq!(label_run(index,false).to_str().unwrap(), input(index).to_ascii_uppercase().to_string());
        }
        for i in 0..len(true) {let cell=match i {9=>10,10=>9,_=>i};let row=cell/3;let col=cell%3;
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
    fn abc_is_left_of_zero_and_navigation_follows_geometry() {
        use crate::ui::screen::Dir;
        assert!(rect(10,true).x < rect(9,true).x);
        assert_eq!(next(9,Dir::Left,true),Some(10));
        assert_eq!(next(10,Dir::Right,true),Some(9));
        assert_eq!(next(10,Dir::Up,true),Some(6));
    }
    #[test]
    fn every_key_is_on_screen_and_labels_cover_both_layouts() {
        for numeric in [false, true] {
            for i in 0..len(numeric) {
                let r = rect(i, numeric);
                assert!(r.x >= 96.0 && r.x + r.w <= 696.0 && r.y + r.h <= 1080.0);
                assert!(!label_run(i, numeric).is_empty());
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
