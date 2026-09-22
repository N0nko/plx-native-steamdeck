//! Related-shelf geometry and actions for [`super::DetailScreen`].

use crate::metadata::Detail;
use crate::pms::PmsMovie;
use crate::ui::card_row::{self, CardRow, RowStyle};
use crate::ui::machine::GroupId;
use crate::ui::widgets::Art;
use crate::ui::{theme, Painter, Rect};

pub(crate) const RELATED_ELEM_RANGE_START: u32 = 640;
pub(crate) const RELATED_ELEM_RANGE_END: u32 = 1152;
pub(crate) const RELATED_GROUP: GroupId = GroupId(3);
/// Heading cap top to card top — the SHARED shelf pitch (`consts::TITLE_DY + CARD_DY`), the same
/// 60 a Home or Library shelf puts between its heading and its posters. It was a local 46, so the
/// one object this page shares with every browsing screen sat 14px tighter here than anywhere else.
pub(crate) const LABEL_H: f32 = crate::ui::consts::TITLE_DY + crate::ui::consts::CARD_DY;

pub(crate) fn elem(index: usize) -> Option<u32> {
    (index < (RELATED_ELEM_RANGE_END - RELATED_ELEM_RANGE_START) as usize)
        .then_some(RELATED_ELEM_RANGE_START + index as u32)
}

pub(crate) fn locate(key: u32) -> Option<usize> {
    (RELATED_ELEM_RANGE_START..RELATED_ELEM_RANGE_END)
        .contains(&key)
        .then(|| (key - RELATED_ELEM_RANGE_START) as usize)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    None,
    OpenDetail(crate::plex::ServerId, String),
}

pub(crate) fn action(d: &Detail, key: u32) -> Action {
    let Some(m) = locate(key).and_then(|i| d.related.get(i)) else {
        return Action::None;
    };
    if m.rk.is_empty() {
        return Action::None;
    }
    Action::OpenDetail(m.sid, m.rk.clone())
}

pub(crate) fn item<'a>(d: &'a Detail, key: u32) -> Option<&'a PmsMovie> {
    d.related.get(locate(key)?)
}

pub(crate) fn rect(row: &CardRow, index: usize, top: f32, at_drawn: bool) -> Rect {
    let base = card_row::tile_rect(
        index,
        crate::ui::consts::MARGIN_X,
        RowStyle::HOME.w + RowStyle::HOME.gap,
        row.scroll_x(),
        top + LABEL_H,
        (RowStyle::HOME.w, RowStyle::HOME.h),
    );
    if at_drawn {
        base.scaled(row.scale(index))
    } else {
        base.scaled(RowStyle::HOME.focus_scale)
    }
}

/// `band` is this shelf's live label-band expansion ([`CardRow::band_expand`]), 0 collapsed → 1
/// focused. The band is the SHARED collapse every other screen uses, not a fixed reservation: a
/// shelf that holds no focus draws no label, so it gives the room back and the next section's
/// heading rises to the design system's own region gap behind it.
pub(crate) fn block_h(band: f32) -> f32 {
    LABEL_H + RowStyle::HOME.h + card_row::under_band(band)
}

/// Per-hub motion; metadata identities never depend on a transient row number.
pub(crate) struct Rows { rows: Vec<((String, String), CardRow)>, empty: CardRow }
impl Rows {
    pub(crate) fn new() -> Self { Self { rows: Vec::new(), empty: CardRow::new() } }
    fn row(&self, index: usize) -> &CardRow { self.rows.get(index).map(|r| &r.1).unwrap_or(&self.empty) }
    pub(crate) fn update(&mut self, d: &Detail, focus: Option<usize>, dt: f32) {
        let count = shelf_count(d);
        let identity = |i: usize| d.related_shelves.get(i)
            .map(|s| (s.identifier.as_str(), s.title.as_str())).unwrap_or(("related", "Related"));
        if self.rows.len() != count || self.rows.iter().enumerate().any(|(i, (id, _))|
            (id.0.as_str(), id.1.as_str()) != identity(i)) {
            let mut previous = std::mem::take(&mut self.rows);
            self.rows = (0..count).map(|i| {
                let (key, title) = identity(i);
                let id = (key.to_owned(), title.to_owned());
                let row = previous.iter().position(|(old, _)| old == &id)
                    .map(|i| previous.remove(i).1).unwrap_or_else(CardRow::new);
                (id, row)
            }).collect();
        }
        for (i, (_, row)) in self.rows.iter_mut().enumerate() {
            let (_, start, len) = shelf(d, i);
            row.update(len, focus.filter(|f| *f >= start && *f < start + len).map(|f| f - start), &RowStyle::HOME, dt);
        }
    }
    pub(crate) fn band_lift(&self) -> f32 {
        self.rows.iter().map(|(_, row)| card_row::BAND_OPEN * row.band_expand().clamp(0.0, 1.0)).sum()
    }
    fn row_top(&self, d: &Detail, index: usize, top: f32) -> f32 {
        top + (0..index.min(shelf_count(d))).map(|i|
            block_h(self.row(i).band_expand()) + crate::ui::consts::UNDER_LABEL_AIR).sum::<f32>()
    }
    pub(crate) fn rect(&self, d: &Detail, index: usize, top: f32, drawn: bool) -> Rect {
        let (group, col) = position(d, index).unwrap_or((0, 0));
        rect(self.row(group), col, self.row_top(d, group, top), drawn)
    }
    pub(crate) fn nearest(&self, d: &Detail, from: Rect, top: f32) -> usize {
        (0..d.related.len()).min_by(|&a, &b| {
            let distance = |i| { let r = self.rect(d, i, top, false);
                (r.cx() - from.cx()).abs() + (r.cy() - from.cy()).abs() };
            distance(a).total_cmp(&distance(b))
        }).unwrap_or(0)
    }
}

fn shelf_count(d: &Detail) -> usize {
    if d.related.is_empty() { 0 } else { d.related_shelves.len().max(1) }
}
fn shelf(d: &Detail, i: usize) -> (&str, usize, usize) {
    if d.related_shelves.is_empty() { return ("Related", 0, d.related.len()); }
    let Some(s) = d.related_shelves.get(i) else { return ("Related", 0, 0); };
    let start = s.start.min(d.related.len());
    (s.title.as_str(), start, s.len.min(d.related.len() - start))
}
fn position(d: &Detail, index: usize) -> Option<(usize, usize)> {
    (0..shelf_count(d)).find_map(|i| {
        let (_, start, len) = shelf(d, i);
        (index >= start && index < start + len).then_some((i, index.saturating_sub(start)))
    })
}
pub(crate) fn base_height(d: &Detail) -> f32 {
    let n = shelf_count(d);
    n as f32 * block_h(0.0) + n.saturating_sub(1) as f32 * crate::ui::consts::UNDER_LABEL_AIR
}
pub(crate) fn neighbour(d: &Detail, index: usize, dir: crate::ui::screen::Dir) -> Option<usize> {
    use crate::ui::screen::Dir;
    let (row, col) = position(d, index)?;
    let (_, _start, len) = shelf(d, row);
    match dir {
        Dir::Left if col > 0 => Some(index - 1),
        Dir::Right if col + 1 < len => Some(index + 1),
        Dir::Up if row > 0 => { let (_, start, len) = shelf(d, row - 1); (len > 0).then_some(start + col.min(len.saturating_sub(1))) },
        Dir::Down if row + 1 < shelf_count(d) => { let (_, start, len) = shelf(d, row + 1); (len > 0).then_some(start + col.min(len.saturating_sub(1))) },
        _ => None,
    }
}

pub(crate) fn draw(p: Painter, d: &Detail, rows: &Rows, top: f32, focused: Option<usize>,
    measure: &dyn crate::ui::machine::Measure) {
    for i in 0..shelf_count(d) {
        let row = rows.row(i);
        let top = rows.row_top(d, i, top);
        if top > crate::ui::consts::SCR_H || top + block_h(1.0) < 0.0 { continue; }
        let (title, start, len) = shelf(d, i);
        let text = std::ffi::CString::new(title.replace('\0', "")).unwrap_or_default();
        p.text(text.as_ptr(), crate::ui::consts::MARGIN_X, top - row.lift(),
            theme::size::HEADLINE, theme::TEXT_HEADING, 0, 1);
        let items = &d.related[start..start + len];
        let selected = focused.filter(|f| *f >= start && *f < start + len).map(|f| f - start);
        card_row::strip(p, row, len, selected.map_or(-1, |i| i as i32), top + LABEL_H,
            (RowStyle::HOME.w, RowStyle::HOME.h), RowStyle::HOME.w + RowStyle::HOME.gap,
            &RowStyle::HOME, crate::ui::consts::SCR_W,
            |i| Art::Poster(items.get(i)), |i| items.get(i).and_then(|m| m.resume_frac()),
            |i| card_row::TileLabel::title(&items[i].title), |_, _, _, _| {}, measure);
    }
}

pub(crate) fn draw_focused(p: Painter, d: &Detail, rows: &Rows, index: usize, top: f32,
    press: f32, measure: &dyn crate::ui::machine::Measure) {
    let Some(item) = d.related.get(index) else { return; };
    let Some((group, col)) = position(d, index) else { return; };
    let row = rows.row(group);
    let base = rect(row, col, rows.row_top(d, group, top), true);
    let scale = row.scale(col) * press;
    card_row::draw_focused(p, Art::Poster(Some(item)), base.scaled(press), scale,
        &RowStyle::HOME, item.resume_frac(), &card_row::TileLabel::title(&item.title), measure);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_related_key_round_trips() {
        for i in 0..512 {
            assert_eq!(locate(elem(i).unwrap()), Some(i));
        }
    }

    #[test]
    fn the_related_menus_anchor_is_the_tile_the_shelf_drew() {
        let mut row = CardRow::new();
        for _ in 0..120 {
            row.update(12, Some(11), &RowStyle::HOME, 1.0 / 60.0);
        }
        for i in 0..12 {
            let expected = card_row::tile_rect(
                i,
                crate::ui::consts::MARGIN_X,
                RowStyle::HOME.w + RowStyle::HOME.gap,
                row.scroll_x(),
                200.0 + LABEL_H,
                (RowStyle::HOME.w, RowStyle::HOME.h),
            )
            .scaled(row.scale(i));
            let actual = rect(&row, i, 200.0, true);
            assert_eq!(
                (actual.x, actual.y, actual.w, actual.h),
                (expected.x, expected.y, expected.w, expected.h)
            );
        }
    }
}

#[cfg(test)]
mod grouped_navigation_tests {
    use super::*;
    use crate::ui::screen::Dir;
    fn fixture() -> Detail {
        Detail { related: (0..5).map(|_| crate::pms::PmsMovie::default()).collect(),
            related_shelves: vec![
                crate::metadata::RelatedShelf { title:"First".into(), start:0, len:3, ..Default::default() },
                crate::metadata::RelatedShelf { title:"Second".into(), start:3, len:2, ..Default::default() }],
            ..Default::default() }
    }
    #[test] fn horizontal_navigation_never_spills_into_the_next_category() {
        let d=fixture(); assert_eq!(neighbour(&d,2,Dir::Right),None);
        assert_eq!(neighbour(&d,3,Dir::Left),None);
        assert_eq!(neighbour(&d,1,Dir::Right),Some(2));
    }
    #[test] fn vertical_navigation_keeps_column_and_clamps_shorter_shelves() {
        let d=fixture(); assert_eq!(neighbour(&d,2,Dir::Down),Some(4));
        assert_eq!(neighbour(&d,4,Dir::Up),Some(1));
        assert_eq!(neighbour(&d,0,Dir::Up),None);
        assert_eq!(neighbour(&d,4,Dir::Down),None);
    }
    #[test] fn legacy_flat_metadata_remains_one_related_shelf() {
        let mut d=fixture();d.related_shelves.clear();
        assert_eq!(shelf_count(&d),1); assert_eq!(position(&d,4),Some((0,4)));
        assert_eq!(neighbour(&d,2,Dir::Right),Some(3));
    }
}
