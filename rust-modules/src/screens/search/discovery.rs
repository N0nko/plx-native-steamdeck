//! Entry-owned discovery trail and debounced contextual previews.
use super::*;
use crate::discovery::{Page, Reply, Request};
#[derive(Clone, Debug, Default)]
pub(super) struct Browse {
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
        c.bool(self.home_failed).seq(self.cache.len());
        for (seed, page) in &self.cache {
            match seed {
                crate::plex::catalog::DiscoverySeed::Title { id } => {
                    c.u32(0).str(id);
                }
                crate::plex::catalog::DiscoverySeed::Person { id } => {
                    c.u32(1).str(id);
                }
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
        if cx.focus.current.is_some_and(|key| {
            self.rows
                .iter()
                .skip(self.base_rows)
                .any(|r| r.elems.contains(&key.elem))
        }) {
            return;
        }
        let selected = cx
            .focus
            .current
            .and_then(|key| {
                self.rows
                    .iter()
                    .take(self.base_rows)
                    .enumerate()
                    .find_map(|(row, r)| {
                        r.elems
                            .iter()
                            .position(|e| *e == key.elem)
                            .map(|col| (row, col))
                    })
            })
            .and_then(|(row, col)| self.shown.rows.get(row)?.items.get(col));
        let next = selected.and_then(|item| {
            crate::discovery::seed(item).map(|seed| (seed, item.title().to_owned(), item.sid()))
        });
        if next.as_ref().map(|n| &n.0)
            != self
                .discovery
                .pending
                .as_ref()
                .and_then(|r| r.seed.as_ref())
        {
            self.discovery.serial = self.discovery.serial.wrapping_add(1);
            self.discovery.request = None;
            self.discovery.pending_down = None;
            self.discovery.preview = None;
            self.discovery.failed = false;
            self.discovery.delay = 0;
            self.content_dirty = true;
            self.discovery.pending = next.map(|(seed, name, sid)| Request {
                profile,
                serial: self.discovery.serial,
                seed: Some(seed),
                name,
                sid,
            });
            if let Some(req) = &self.discovery.pending {
                if let Some((_, page)) = self
                    .discovery
                    .cache
                    .iter()
                    .find(|(seed, _)| Some(seed) == req.seed.as_ref())
                {
                    self.discovery.preview = Some(page.clone());
                    self.discovery.request = Some(req.clone());
                }
            }
        }
        if self.discovery.request.is_none()
            && self.discovery.pending.is_some()
            && !self.discovery.failed
        {
            self.discovery.delay = self.discovery.delay.saturating_add(tick.dt_us);
            if self.discovery.delay >= 300_000 {
                let req = self.discovery.pending.clone().unwrap();
                self.discovery.request = Some(req.clone());
                fx.push(Fx::App(AppFx::Discovery(req)));
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
        let mut page = reply.page.clone();
        if let Some(p) = &mut page {
            p.upgrade(H::search(cx).shelves());
        }
        if reply.request.seed.is_none() {
            self.discovery.home_failed = page.is_none();
            self.discovery.home = page;
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
            self.discovery.preview = page;
        } else {
            return;
        }
        self.content_dirty = true;
        self.sync(cx, false, fx);
        fx.invalidate(Provenance::Input);
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
