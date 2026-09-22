//! Entry-owned discovery trail and debounced contextual previews.
use super::*;
use crate::discovery::{Page, Reply, Request};
#[derive(Clone, Debug, Default)]
pub(super) struct Browse {
    pub levels: Vec<Level>,
    pub pending_from: Option<(usize, u32)>,
    pub home_failed: bool,
    pub cache: Vec<(crate::plex::catalog::DiscoverySeed, Page)>,
    pub page: Option<Page>,
    pub home: Option<Page>,
    pub preview: Option<Page>,
    pub trail: Vec<Return>,
    pub serial: u32,
    pub request: Option<Request>,
    pub pending: Option<Request>,
    pub delay: u32,
    pub failed: bool,
    pub home_requested: bool,
    pub pending_down: Option<u32>,
}
pub(super) const DWELL_US: u32 = 2_000_000;
#[derive(Clone, Debug)]
pub(super) struct Level {
    pub row: usize,
    pub focus: u32,
    pub seed: crate::plex::catalog::DiscoverySeed,
    pub page: Page,
}
impl Browse {
    fn owner_level(&self, base_rows: usize, row: usize) -> Option<usize> {
        let mut start=base_rows;
        for (i,level) in self.levels.iter().enumerate() {
            let end=start+level.page.rows.len();
            if (start..end).contains(&row) { return Some(i); }
            start=end;
        }
        None
    }
    fn branch(&mut self, base_rows: usize, row: usize, focus: u32, seed: crate::plex::catalog::DiscoverySeed, page: Page) {
        // Sibling cast/similar shelves share a parent. A new sibling's branch replaces the
        // previous sibling's descendants, even when its row number is greater.
        let keep=self.owner_level(base_rows,row).map_or(0,|i|i+1);
        self.levels.truncate(keep);
        if self.levels.len() < 20 { self.levels.push(Level { row, focus, seed, page }); }
    }
}
#[derive(Clone, Debug)]
pub(super) struct Return {
    keys: Vec<KeyEntry>,
    page: Option<Page>,
    focus: u32,
    scroll: f32,
    rows: Vec<(GroupId, f32)>,
}
impl Browse {
    pub fn write(&self, c: &mut Canon) {
        c.seq(self.levels.len());
        for level in &self.levels { c.u64(level.row as u64).u32(level.focus).str(&format!("{:?}",level.seed)); level.page.write(c); }
        c.option(self.pending_from, |c,(row,focus)| { c.u64(row as u64).u32(focus); });
        c.bool(self.home_failed).seq(self.cache.len());
        for (seed, page) in &self.cache {
            match seed {
                crate::plex::catalog::DiscoverySeed::Title { id } => {
                    c.u32(0).str(id);
                }
                crate::plex::catalog::DiscoverySeed::Person { id } => {
                    c.u32(1).str(id);
                }
                crate::plex::catalog::DiscoverySeed::Hub { key } => {c.u32(2).str(key);}
            }
            page.write(c);
        }
        c.u32(self.serial)
            .u32(self.delay)
            .bool(self.failed)
            .bool(self.home_requested)
            .option(self.pending_down, |c, x| {
                c.u32(x);
            });
        for page in [&self.page, &self.home, &self.preview] {
            c.option(page.as_ref(), |c, p| p.write(c));
        }
        for req in [&self.request, &self.pending] {
            c.option(req.as_ref(), |c, r| {
                c.u32(r.profile)
                    .u32(r.serial)
                    .str(&r.name)
                    .u32(r.sid.raw() as u32);
                c.option(r.seed.as_ref(), |c, s| match s {
                    crate::plex::catalog::DiscoverySeed::Title { id } => {
                        c.u32(0).str(id);
                    }
                    crate::plex::catalog::DiscoverySeed::Person { id } => {
                        c.u32(1).str(id);
                    }
                    crate::plex::catalog::DiscoverySeed::Hub { key } => {c.u32(2).str(key);}
                });
            });
        }
        c.seq(self.trail.len());
        for ret in &self.trail {
            c.seq(ret.keys.len());
            for key in &ret.keys {
                key.write(c);
            }
            c.option(ret.page.as_ref(), |c, p| p.write(c))
                .u32(ret.focus)
                .f32(ret.scroll)
                .seq(ret.rows.len());
            for (g, x) in &ret.rows {
                c.u32(g.0).f32(*x);
            }
        }
    }
}
impl SearchScreen {
    fn directory_pending(&self) -> bool {
        self.discovery.request.as_ref().is_some_and(|r|
            matches!(r.seed, Some(crate::plex::catalog::DiscoverySeed::Hub { .. })))
    }

    pub(super) fn discover_tick<H: SearchLike>(
        &mut self,
        tick: crate::ui::machine::Tick,
        cx: &Cx<'_, H>,
        fx: &mut Effects<'_, H>,
    ) {
        let profile = self.draft.profile();
        if !self.real_query()
            && self.draft.query().trim().is_empty()
            && !self.discovery.home_requested
        {
            self.discovery.home_requested = true;
            self.discovery.home_failed = false;
            self.discovery.serial = self.discovery.serial.wrapping_add(1);
            fx.push(Fx::App(AppFx::Discovery(Request {
                profile,
                serial: self.discovery.serial,
                seed: None,
                name: "Discover".into(),
                sid: crate::plex::server_ids()
                    .next()
                    .unwrap_or(crate::plex::ServerId::UNSET),
            })));
        }
        if !self.context_enabled() {
            if self.directory_pending() { return; }
            if self.discovery.pending.is_some() || self.discovery.preview.is_some() || self.discovery.request.is_some() {
                self.discovery.serial = self.discovery.serial.wrapping_add(1);
                self.discovery.pending = None;
                self.discovery.preview = None;
                self.discovery.request = None;
                self.discovery.pending_down = None;
                self.content_dirty = true;
            }
            return;
        }
        let selected = cx.focus.current.filter(|key| key.entry == self.entry).and_then(|key| {
            self.rows.iter().enumerate().find_map(|(row,r)| r.elems.iter().position(|e| *e==key.elem)
                .and_then(|col| self.shown.rows.get(row)?.items.get(col).map(|item|(row,key.elem,item))))
        });
        let next = selected.and_then(|(row,focus,item)| crate::discovery::seed(item)
            .map(|seed|(row,focus,seed,item.title().to_owned(),item.sid())));
        let unchanged = next.as_ref().map_or(self.discovery.pending.is_none(), |(row,focus,seed,_,_)|
            self.discovery.pending_from == Some((*row,*focus)) && self.discovery.pending.as_ref().and_then(|r|r.seed.as_ref()) == Some(seed)
            || self.discovery.pending.is_none() && self.discovery.levels.iter().any(|l|l.row==*row && l.focus==*focus && l.seed==*seed));
        if !unchanged {
            self.discovery.serial = self.discovery.serial.wrapping_add(1);
            self.discovery.request = None;
            self.discovery.pending = None;
            self.discovery.pending_from = None;
            self.discovery.delay = 0;
            self.discovery.failed = false;
            if let Some((row,focus,seed,name,sid)) = next {
                // Revisiting the parent that already owns a branch preserves ALL descendants.
                if self.discovery.levels.iter().any(|l|l.row==row && l.focus==focus && l.seed==seed) { return; }
                self.discovery.pending_from=Some((row,focus));
                self.discovery.pending=Some(Request { profile, serial:self.discovery.serial, seed:Some(seed), name, sid });
            }
        }
        if self.discovery.request.is_none() && self.discovery.pending.is_some() && !self.discovery.failed {
            self.discovery.delay=self.discovery.delay.saturating_add(tick.dt_us);
            if self.discovery.delay >= DWELL_US {
                let req=self.discovery.pending.clone().unwrap();
                self.discovery.request=Some(req.clone());
                if let Some(page)=self.discovery.cache.iter().find(|(seed,_)|Some(seed)==req.seed.as_ref()).map(|(_,page)|page.clone()) {
                    self.discover_land(&Reply { request:req, page:Some(page) },cx,fx);
                } else { fx.push(Fx::App(AppFx::Discovery(req))); }
            }
        }
    }
    pub(super) fn discover_land<H: SearchLike>(
        &mut self,
        reply: &Reply,
        cx: &Cx<'_, H>,
        fx: &mut Effects<'_, H>,
    ) {
        if reply.request.profile != self.draft.profile() {
            return;
        }
        if reply.request.seed.is_some() && !matches!(reply.request.seed, Some(crate::plex::catalog::DiscoverySeed::Hub { .. })) && !self.context_enabled() { return; }
        let mut page = reply.page.clone();
        if let Some(p) = &mut page {
            p.upgrade(H::search(cx).shelves());
        }
        if reply.request.seed.is_none() {
            self.discovery.home_failed = page.is_none();
            if page.is_some(){self.discovery.home = page;}
        } else if self.discovery.request.as_ref() == Some(&reply.request) {
            if let Some(p) = &page {
                if self.discovery.cache.len() == 8 {
                    self.discovery.cache.remove(0);
                }
                self.discovery
                    .cache
                    .push((reply.request.seed.clone().unwrap(), p.clone()));
            }
            self.discovery.failed = page.is_none();
            if matches!(reply.request.seed, Some(crate::plex::catalog::DiscoverySeed::Hub { .. })) {
                self.discovery.preview = page;
            } else if let (Some((row,focus)),Some(page),Some(seed)) = (self.discovery.pending_from,page,reply.request.seed.clone()) {
                if cx.focus.current != Some(self.key(focus)) { return; }
                self.discovery.branch(self.base_rows,row,focus,seed,page);
            }
        } else {
            return;
        }
        self.content_dirty = true;
        self.sync(cx, false, fx);
        fx.invalidate(Provenance::Input);
        if reply.request.seed.is_none(){return;}
        if let Some(elem) = self.discovery.pending_down.take() {
            if cx.focus.current == Some(self.key(elem)) {
                self.discover_enter(0, 0, elem, fx);
                self.sync(cx, false, fx);
            }
        }
    }
    pub(super) fn discover_enter<H: SearchLike>(
        &mut self,
        row: usize,
        col: usize,
        from: u32,
        fx: &mut Effects<'_, H>,
    ) -> bool {
        if !self.directory_pending() { return false; }
        let Some(page) = self.discovery.preview.take().filter(|p| !p.rows.is_empty()) else {
            return false;
        };
        if self.discovery.trail.len() == 6 {
            self.discovery.trail.remove(0);
        }
        self.discovery.trail.push(Return {
            keys: self
                .keys
                .iter()
                .filter(|key| {
                    self.rows
                        .iter()
                        .take(self.base_rows)
                        .any(|r| r.elems.contains(&key.elem))
                })
                .cloned()
                .collect(),
            page: self.discovery.page.take(),
            focus: from,
            scroll: self.scroll_target,
            rows: self
                .rows
                .iter()
                .take(self.base_rows)
                .map(|r| (r.group, r.motion.scroll_x()))
                .collect(),
        });
        self.discovery.page = Some(page);
        self.discovery.pending = None;
        self.discovery.request = None;
        self.discovery.serial = self.discovery.serial.wrapping_add(1);
        self.discovery.pending_down = None;
        self.scroll.jump(0.0);
        self.scroll_target = 0.0;
        self.content_dirty = true;
        self.rows.clear();
        // The actual element is minted by sync; a projected seat selects the chosen preview column.
        self.discovery_seat = Some((row, col));
        fx.invalidate(Provenance::Input);
        true
    }
    pub(super) fn context_back<H: SearchLike>(&self, focus: Option<crate::ui::machine::FocusKey<u32>>, fx: &mut Effects<'_, H>) -> bool {
        let Some(focus)=focus else { return false; };
        let Some(row)=self.rows.iter().position(|r|r.elems.contains(&focus.elem)) else { return false; };
        let Some(level)=self.discovery.owner_level(self.base_rows,row).and_then(|i|self.discovery.levels.get(i)) else { return false; };
        self.reseat(FocusTarget::Elem(self.key(level.focus)),fx);
        true
    }
    pub(super) fn discover_back<H: SearchLike>(&mut self, fx: &mut Effects<'_, H>) -> bool {
        if self.discovery.pending_down.take().is_some() {
            return true;
        }
        let Some(ret) = self.discovery.trail.pop() else {
            return false;
        };
        for key in ret.keys {
            if !self.keys.iter().any(|k| k.elem == key.elem) {
                self.keys.push(key);
            }
        }
        self.discovery.page = ret.page;
        self.discovery.preview = None;
        self.discovery.request = None;
        self.discovery.pending = None;
        self.discovery.serial = self.discovery.serial.wrapping_add(1);
        self.scroll.jump(ret.scroll);
        self.scroll_target = ret.scroll;
        self.content_dirty = true;
        self.rows.clear();
        self.discovery_return = Some(ret.rows);
        self.reseat(FocusTarget::Elem(self.key(ret.focus)), fx);
        fx.invalidate(Provenance::Input);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page() -> Page { Page { rows:vec![crate::search::Shelf {kind:Kind::Person,items:vec![]},crate::search::Shelf {kind:Kind::Movie,items:vec![]}],headings:vec!["Cast".into(),"More like".into()],..Default::default() } }
    fn seed(n: &str) -> crate::plex::catalog::DiscoverySeed { crate::plex::catalog::DiscoverySeed::Title {id:n.into()} }
    #[test]
    fn branches_retain_parent_rows_and_sibling_replacement_discards_only_descendants() {
        let mut b=Browse::default();
        b.branch(1,0,100,seed("a"),page()); b.branch(1,1,200,seed("actor"),page());
        b.branch(1,3,300,seed("b"),page()); assert_eq!(b.levels.len(),3);
        assert_eq!(b.owner_level(1,2),Some(0));
        b.branch(1,2,201,seed("sibling"),page());
        assert_eq!(b.levels.len(),2); assert_eq!(b.levels[0].focus,100); assert_eq!(b.levels[1].focus,201);
        b.branch(1,0,101,seed("new root"),page()); assert_eq!(b.levels.len(),1);
    }
}
