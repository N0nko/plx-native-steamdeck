//! Offline library composed from the shared table and focus system.
use std::borrow::Cow;
use crate::deck_downloads::{Command, Entry, State};
use crate::screens::registry::{AppFx, AppLike};
use crate::ui::{Rect, frame::Budget, table::{Row, Section, TableView}};
use crate::ui::machine::{Canon, Cx, Edge, Effects, EntryId, FocusKey, Fx, GroupId, Handled, InputKind, Key, LogicalState, Machine, NavOp};
use crate::ui::screen::{Activate, At, AxisMask, Dir, DrawFrame, EdgeRule, ElemKind, Focusable, GroupKind, GroupSpec, Hover, Placed, RenderStrategy, Screen, ScreenEvent, Seat, Step, Stop};

pub(crate) struct DownloadsScreen {
    entry: EntryId,
    table: TableView,
    entries: Vec<Entry>,
    actions: Vec<Option<Command>>,
    last_poll: std::time::Instant,
    signature: String,
}
impl DownloadsScreen {
    pub(crate) fn new(entry: EntryId) -> Self {
        let mut screen = Self { entry, table: TableView::new(), entries: Vec::new(), actions: Vec::new(), last_poll: std::time::Instant::now(), signature: String::new() };
        screen.refresh(); screen
    }
    fn frame(&self) -> Rect { Rect::new(120., 130., 1680., 820.) }
    fn refresh(&mut self) -> bool {
        let (entries, message) = crate::deck_downloads::snapshot();
        let signature = format!("{}:{message}", serde_json::to_string(&entries).unwrap_or_default());
        if signature == self.signature { return false; }
        self.signature = signature;
        let mut sections = Vec::new(); let mut actions = Vec::new();
        for entry in &entries {
            let percent = 100. * entry.received_bytes as f64 / entry.expected_bytes.max(1) as f64;
            let status = match entry.state { State::Complete => "Ready offline", State::Downloading => "Downloading", State::Paused => "Paused", State::Failed => "Interrupted" };
            let detail = format!("{status} · {percent:.0}% · {:.1} GiB", entry.expected_bytes as f64 / 1073741824.);
            let id = entry.identity.clone();
            let mut section = Section::new(entry.title.clone());
            section = section.row(Row::new(if entry.state == State::Complete { "Play offline" } else { "Download in progress" }).detail(detail));
            actions.push((entry.state == State::Complete).then(|| Command::Play(id.clone())));
            if entry.state != State::Complete {
                let downloading = entry.state == State::Downloading;
                section = section.row(Row::new(if downloading { "Pause download" } else { "Resume download" }));
                actions.push(Some(if downloading { Command::Pause(id.clone()) } else { Command::Resume(id.clone()) }));
            }
            section = section.row(Row::new("Delete download").destructive(true));
            actions.push(Some(Command::Delete(id)));
            sections.push(section);
        }
        if sections.is_empty() {
            sections.push(Section::new("Downloads").row(Row::new("No downloaded videos").detail("Hold OK on a movie or episode and choose Download original")));
            actions.push(None);
        }
        if !message.is_empty() { sections.push(Section::new("").row(Row::new(message))); actions.push(None); }
        let sel = self.table.sel.clamp(0, actions.len().saturating_sub(1) as i32);
        self.table.set_sections(sections, sel, false);
        self.actions = actions; self.entries = entries;
        true
    }
    fn activate<H: AppLike>(&self, elem: u32, fx: &mut Effects<'_, H>) {
        if let Some(Some(command)) = self.actions.get(elem as usize) { fx.push(Fx::App(AppFx::Deck(command.clone()))); }
    }
}
impl LogicalState for DownloadsScreen {
    fn write(&self, c: &mut Canon) { c.str(&self.signature).u32(self.table.sel as u32); self.table.write_motion(c); }
    fn probe(&self, out: &mut String) { out.push_str("downloads"); }
}
impl<H: AppLike> Machine<H> for DownloadsScreen {
    type Ev = ScreenEvent<H>;
    fn step(&mut self, ev: &Self::Ev, cx: &Cx<'_, H>, fx: &mut Effects<'_, H>) -> Handled {
        match ev {
            ScreenEvent::Tick(tick) => {
                if self.last_poll.elapsed().as_millis() >= 500 {
                    self.last_poll = std::time::Instant::now();
                    if self.refresh() { fx.invalidate(crate::ui::present::Provenance::Landing(crate::ui::machine::MachineId::Session)); }
                }
                self.table.update(tick.dt(), self.frame().h);
            }
            ScreenEvent::FocusMoved { to, .. } => self.table.sel = to.elem as i32,
            ScreenEvent::Activate(elem) => self.activate(*elem, fx),
            ScreenEvent::PressCommit(_) => if let Some(key) = cx.focus.current { self.activate(key.elem, fx); },
            ScreenEvent::Input(input) => if matches!(input.kind, InputKind::Key { key: Key::Back, edge: Edge::Down, .. }) {
                fx.push(Fx::Nav(NavOp::Pop)); return Handled::Yes;
            },
            _ => {}
        }
        Handled::No
    }
}
impl<H: AppLike> Focusable<H> for DownloadsScreen {
    fn groups(&self, _: &Cx<'_, H>, out: &mut Vec<GroupSpec>) {
        out.push(GroupSpec { id: GroupId(0), kind: GroupKind::Column, seat: Seat::Remembered, reachable: AxisMask::BOTH, edge: [EdgeRule::Stop; 4], extent: self.frame(), len: self.actions.len(), elem: ElemKind::Bare });
    }
    fn group_of(&self, elem: &u32, _: &Cx<'_, H>) -> Option<GroupId> { ((*elem as usize) < self.actions.len()).then_some(GroupId(0)) }
    fn neighbour(&self, key: FocusKey<u32>, dir: Dir, _: &Cx<'_, H>) -> Step<u32> {
        let next = match dir { Dir::Up => key.elem.checked_sub(1), Dir::Down => key.elem.checked_add(1), _ => None };
        next.filter(|n| (*n as usize) < self.actions.len()).map_or(Step::Edge, |elem| Step::Move(FocusKey { entry: self.entry, elem }))
    }
    fn place(&self, elem: &u32, _: &Cx<'_, H>, _: At) -> Option<Placed> {
        let rect = self.table.row_frame(self.frame(), *elem as i32)?;
        Some(Placed { rect, rest_rect: rect, clip: self.frame(), index: Some(*elem) })
    }
    fn reconcile(&self, want: FocusKey<u32>, _: &Cx<'_, H>) -> FocusKey<u32> {
        FocusKey { entry: self.entry, elem: want.elem.min(self.actions.len().saturating_sub(1) as u32) }
    }
    fn seat(&self, _: GroupId, _: Placed, _: &Cx<'_, H>) -> FocusKey<u32> { FocusKey { entry: self.entry, elem: self.table.sel.max(0) as u32 } }
}
impl<H: AppLike> Screen<H> for DownloadsScreen {
    fn name(&self) -> &'static str { "downloads" }
    fn state(&self) -> &dyn LogicalState { self }
    fn crumb(&self, _: &Cx<'_, H>) -> Option<Cow<'_, str>> { Some(Cow::Borrowed("Downloads")) }
    fn prepare(&mut self, _: &mut Budget, _: &Cx<'_, H>) {}
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, H>) {
        let p = f.painter.alpha(f.page_alpha); let r = self.frame();
        crate::ui::widgets::panel_ground(p, r, 24., f.underlay);
        self.table.draw(p, r, f.measure);
        for elem in 0..self.actions.len() as u32 {
            if let Some(placed) = <Self as Focusable<H>>::place(self, &elem, f.cx, At::Drawn) {
                f.stop(p, Stop { key: FocusKey { entry: self.entry, elem }, rect: placed.rect, rest_rect: placed.rest_rect, clip: placed.clip, hover: Hover::Focus, activate: Activate::Immediate });
            }
        }
    }
    fn render(&self) -> RenderStrategy { RenderStrategy::Page }
}
