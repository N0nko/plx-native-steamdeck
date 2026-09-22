//! Stored-file version picker on Detail's owned modal stack. No UI globals or network I/O.
//! The row captures Part identity; the Detail owner validates it and playback re-resolves it.
use std::borrow::Cow;

use crate::media_companion::{
    Action as CompanionAction, Key as CompanionKey, Request as CompanionRequest,
    Snapshot as CompanionSnapshot,
};
use crate::metadata::preplay::{ChoiceRow, TrackChoice};
use crate::metadata::MediaVersion;
use crate::plex::ServerId;
use crate::screens::registry::{AppFx, AppLike, AppMsg, PageMemory};
use crate::ui::consts::{SCR_H, SCR_W};
use crate::ui::frame::Budget;
use crate::ui::machine::{
    Canon, Cx, Delivery, Edge, Effects, EntryId, FocusKey, Fx, GroupId, Handled, InputKind,
    InstanceId, Key, LogicalState, Machine, MachineId, NavOp,
};
use crate::ui::screen::{
    Activate, At, AxisMask, Dir, DrawFrame, EdgeRule, ElemKind, FocusSource, Focusable, GroupKind,
    GroupSpec, HitSource, Hover, Placed, RenderStrategy, Screen, ScreenEvent, Scrim, Seat, Step,
    Stop,
};
use crate::ui::table::{Badge, Row, Section, TableView};
use crate::ui::{theme, Rect};

/// The fields [`VersionsScreen::write`] canonicalises, for the recorder's shape pin (§5.4). The
/// selected ROW is in it deliberately: this panel's UP/DOWN changes nothing else in the app, so
/// without it a replay grades the panel opening and closing and nothing between.
pub(crate) const SHAPE: [&str; 2] = [
    "VersionsScreen{profile:u32,arg:VersionsArg{host:u32,sid:u32,rk:str,anchor:[u32;4]},rows:[{sid:u32,rk:str,part:str,playable:bool,label:str,detail:str,value:opt<str>,badge:opt<str>,checked:bool,audio:[{id:opt<i64>,label:str,detail:str}],subs:[{id:opt<i64>,label:str,detail:str}]}],selected_part:str,preplay_audio:opt<u64>,preplay_subtitle:opt<u64>,companion:opt<{key:{sid:u32,rk:str,part:str},label:str,configured:bool,busy:bool,repair:bool,prepare:bool}>,last_poll:opt<u32>,confirm:opt<u8>,active:bool,sel:i32,table:TableViewMotion}",
    TableView::MOTION_SHAPE,
];

/// What the container is asked to present.
///
/// `host` is the Detail instance the choice is reported back to; it rides on the argument rather
/// than being looked up, for `LibraryMenuArg`'s reason — the entry outlives any one frame's idea of
/// which page is on top. `sid`/`rk` are the copy the page is STANDING ON: the tick's other half,
/// and the pair the addressed store is read with.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct VersionsArg {
    pub(crate) host: InstanceId,
    pub(crate) sid: ServerId,
    pub(crate) rk: String,
    /// Bit-preserving rest rectangle of the pill that opened it; valid in canonical arguments
    /// without float equality.
    pub(crate) anchor: [u32; 4],
}

impl LogicalState for VersionsArg {
    fn write(&self, c: &mut Canon) {
        c.u32(self.host.0)
            .u32(u32::from(self.sid.raw()))
            .str(&self.rk);
        for value in self.anchor {
            c.u32(value);
        }
    }
    fn probe(&self, out: &mut String) {
        out.push_str("versions_arg");
    }
}

// ---- geometry --------------------------------------------------------------------------------

/// A wide table for codec, bitrate and filename descriptions.
const PANEL_W: f32 = 980.0;
/// The pinned ~20px corner radius, the item menu's.
const PANEL_RAD: f32 = 20.0;
/// Air between the button that opened the panel and the panel itself — one `space` rung.
const BTN_GAP: f32 = theme::space::MD;
/// Keep-out from the screen edges, so a panel opened low on the page is still whole.
///
/// Per AXIS, for `item_menu::EDGE`'s reason: `space::XL` 64 clears `MARGIN_Y` vertically and misses
/// `MARGIN_X` horizontally by 32px.
const EDGE: f32 = theme::space::XL;
const EDGE_X: f32 = crate::ui::consts::MARGIN_X;
/// How far the panel rises into place, matching those same chip menus. The container's appear
/// spring drives it now (`DrawFrame::page_alpha` IS `Surface::motion.appear`), so the translate is
/// applied here rather than by `Popover::painter`; at rest it contributes nothing, which is what
/// makes a settled capture of this panel byte-identical across the conversion.
const RISE: f32 = crate::ui::popover::Popover::RISE;

/// Under the button when there is room, above it when there is not — never over it, so the control
/// that opened the panel stays readable beside its own list. Horizontally it hangs off the
/// button's left edge (the list and the label it came from share a margin), pulled inside the
/// screen's keep-out. Pure (anchor + measured height in, rect out), so the placement rules are
/// host-testable without mounting anything.
pub(crate) fn panel_at(a: Rect, content_h: f32) -> Rect {
    let h = content_h.clamp(120.0, SCR_H - 2.0 * EDGE);
    let below = a.y + a.h + BTN_GAP;
    let y = if below + h <= SCR_H - EDGE {
        below
    } else {
        (a.y - BTN_GAP - h).max(EDGE)
    };
    let x = a.x.clamp(EDGE_X, (SCR_W - EDGE_X - PANEL_W).max(EDGE_X));
    Rect::new(x, y.clamp(EDGE, SCR_H - EDGE - h), PANEL_W, h)
}

// ---- the model -------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VersionRow {
    pub(crate) label: String,
    pub(crate) detail: String,
    pub(crate) value: Option<String>,
    pub(crate) badge: Option<String>,
    pub(crate) checked: bool,
    pub(crate) playable: bool,
    pub(crate) sid: ServerId,
    pub(crate) rk: String,
    pub(crate) part: String,
    pub(crate) audio: Vec<ChoiceRow>,
    pub(crate) subs: Vec<ChoiceRow>,
}

fn version_row(sid: ServerId, rk: &str, selected: &str, v: &MediaVersion) -> VersionRow {
    let resolution = crate::ui::fmt::resolution(&v.video_resolution, v.width, v.height)
        .unwrap_or_else(|| "Unknown resolution".into());
    let range = if v.dovi.profile > 0 {
        " · Dolby Vision"
    } else if v.hdr {
        " · HDR"
    } else {
        ""
    };
    let audio: Vec<_> =
        v.audio
            .iter()
            .map(|s| s.codec.to_uppercase())
            .fold(Vec::new(), |mut all, codec| {
                if !all.contains(&codec) {
                    all.push(codec);
                }
                all
            });
    let mut details = Vec::new();
    if v.bitrate > 0 {
        details.push(format!("{:.1} Mbps", v.bitrate as f64 / 1000.0));
    }
    if !audio.is_empty() {
        details.push(audio.join(" / "));
    }
    if v.size > 0 {
        details.push(format!("{:.1} GiB", v.size as f64 / 1_073_741_824.0));
    }
    let filename = v.file.rsplit(['/', '\\']).next().unwrap_or("");
    if !filename.is_empty() {
        details.push(filename.into());
    }
    if !v.playable() {
        details.insert(0, "Multi-part or unavailable — playback disabled".into());
    }
    VersionRow {
        label: format!("{resolution} · {}{range}", v.vcodec.to_uppercase()),
        detail: details.join(" · "),
        value: None,
        badge: Some(v.container.to_uppercase()).filter(|s| !s.is_empty()),
        checked: selected == v.source.part_key,
        playable: v.playable(),
        sid,
        rk: rk.into(),
        part: v.source.part_key.clone(),
        audio: crate::metadata::preplay::options(&v.audio, false),
        subs: crate::metadata::preplay::options(&v.subs, true),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    None,
    Play {
        sid: ServerId,
        rk: String,
        part: String,
    },
}

pub(crate) fn action_at(rows: &[VersionRow], sel: i32) -> Action {
    let Some(row) = usize::try_from(sel).ok().and_then(|i| rows.get(i)) else {
        return Action::None;
    };
    if !row.playable || row.rk.is_empty() || row.part.is_empty() {
        return Action::None;
    }
    Action::Play {
        sid: row.sid,
        rk: row.rk.clone(),
        part: row.part.clone(),
    }
}

// ---- the surface -----------------------------------------------------------------------------

/// Where the selection lands when the table is rebuilt.
enum Sel {
    /// Opening: on the copy you are on — the row the tick is against — because that is where the
    /// eye already is, and one DOWN from it is the alternative.
    OnTheCopyYouAreOn,
}

pub(crate) struct VersionsScreen {
    entry: EntryId,
    arg: VersionsArg,
    /// Version descriptions retained only for this mounted picker.
    rows: Vec<VersionRow>,
    pub(crate) table: TableView,
    selected_part: String,
    choice: TrackChoice,
    companion: Option<CompanionSnapshot>,
    last_poll: Option<u32>,
    confirm: Option<CompanionAction>,
    profile: u32,
    active: bool,
}

impl VersionsScreen {
    pub(crate) fn new(
        entry: EntryId,
        arg: VersionsArg,
        meta: crate::metadata::MetadataView<'_>,
    ) -> Self {
        let mut screen = Self {
            entry,
            arg,
            rows: Vec::new(),
            table: TableView::new(),
            selected_part: String::new(),
            choice: TrackChoice::default(),
            companion: None,
            last_poll: None,
            confirm: None,
            profile:crate::plex::session::current_gen(),
            active: false,
        };
        screen.rebuild(Sel::OnTheCopyYouAreOn, meta);
        screen
    }

    /// Read the leaf actually played by this Detail page, within the page's server.
    fn live_rows(&self, meta: crate::metadata::MetadataView<'_>) -> Vec<VersionRow> {
        let Some(d) = meta
            .current()
            .filter(|d| d.sid == self.arg.sid && d.rk == self.arg.rk)
        else {
            return Vec::new();
        };
        let Some((rk, selected, versions)) = crate::metadata::version_target(d) else {
            return Vec::new();
        };
        versions
            .iter()
            .map(|v| version_row(d.sid, rk, selected, v))
            .collect()
    }

    fn target(&self) -> Option<CompanionKey> {
        let mut matches = self
            .rows
            .iter()
            .filter(|r| r.part == self.selected_part && r.playable);
        let row = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        CompanionKey::new(row.sid, &row.rk, &row.part)
    }

    fn selected_row(&self) -> Option<&VersionRow> {
        self.rows
            .iter()
            .find(|r| r.part == self.selected_part && r.playable)
    }
    fn companion_offset(&self) -> usize {
        self.rows.len()
            + self
                .selected_row()
                .map_or(0, |r| r.audio.len() + r.subs.len())
    }
    fn row_count(&self) -> usize {
        self.companion_offset() + if self.target().is_some() { 2 } else { 0 }
    }
    fn sync_choice(&mut self, meta: crate::metadata::MetadataView<'_>) -> bool {
        let next = self
            .selected_row()
            .map(|r| meta.preplay(r.sid, &r.rk, &r.part))
            .unwrap_or_default();
        if next == self.choice {
            return false;
        }
        self.choice = next;
        self.rebuild_table(self.table.sel);
        true
    }

    /// Keep the highlighted Part, not its array position, when metadata is replaced.
    fn refresh(&mut self, meta: crate::metadata::MetadataView<'_>) -> bool {
        let next = self.live_rows(meta);
        if next == self.rows {
            return self.sync_choice(meta);
        }
        let target = self.target();
        self.rows = next;
        let retained: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                target.is_some() && CompanionKey::new(row.sid, &row.rk, &row.part) == target
            })
            .collect();
        let index = if retained.len() == 1 {
            retained[0].0
        } else {
            self.rows.iter().position(|row| row.checked).unwrap_or(0)
        };
        self.selected_part = self
            .rows
            .get(index)
            .map(|row| row.part.clone())
            .unwrap_or_default();
        if self.target() != target {
            self.companion = None;
            self.confirm = None;
            self.last_poll = None;
        }
        self.choice = self
            .selected_row()
            .map(|r| meta.preplay(r.sid, &r.rk, &r.part))
            .unwrap_or_default();
        self.rebuild_table(index as i32);
        true
    }

    fn rebuild(&mut self, _sel_mode: Sel, meta: crate::metadata::MetadataView<'_>) {
        self.rows = self.live_rows(meta);
        let index = self.rows.iter().position(|row| row.checked).unwrap_or(0);
        self.selected_part = self
            .rows
            .get(index)
            .map(|row| row.part.clone())
            .unwrap_or_default();
        self.choice = self
            .selected_row()
            .map(|r| meta.preplay(r.sid, &r.rk, &r.part))
            .unwrap_or_default();
        self.rebuild_table(index as i32);
    }

    fn rebuild_table(&mut self, selected: i32) {
        let mut sec = Section::new("Play Version");
        for r in &self.rows {
            let mut row = Row::new(r.label.clone())
                .detail(r.detail.clone())
                .checked(r.checked)
                .dim(!r.playable);
            if let Some(value) = &r.value {
                row = row.value(value.clone());
            }
            if let Some(badge) = &r.badge {
                row = row.badge(Badge::Text(badge.clone()));
            }
            sec = sec.row(row);
        }
        let mut sections = vec![sec];
        if let Some(selected_file) = self.selected_row() {
            for (name, options, selected_id) in [
                (
                    "Audio · highlighted file",
                    &selected_file.audio,
                    self.choice.audio,
                ),
                (
                    "Subtitles · highlighted file",
                    &selected_file.subs,
                    self.choice.subtitle,
                ),
            ] {
                if options.is_empty() {
                    continue;
                }
                let mut tracks = Section::new(name);
                for option in options {
                    tracks = tracks.row(
                        Row::new(option.label.clone())
                            .detail(option.detail.clone())
                            .checked(option.id == selected_id),
                    );
                }
                sections.push(tracks);
            }
        }
        if let Some(target) = self.target() {
            let state = self.companion.as_ref().filter(|state| state.key == target);
            let status = state
                .map(|state| state.label.as_str())
                .unwrap_or("Checking highlighted file");
            let can_repair = state.is_some_and(|state| state.can_repair && !state.busy);
            let can_prepare = state.is_some_and(|state| state.can_prepare && !state.busy);
            let repair = self.confirm == Some(CompanionAction::Repair);
            let prepare = self.confirm == Some(CompanionAction::Prepare);
            sections.push(
                Section::new(format!("Media companion · {status}"))
                    .row(
                        Row::new(if repair {
                            "Confirm Repair"
                        } else {
                            "Repair highlighted file"
                        })
                        .detail(if repair {
                            "Press again to send this file's repair request"
                        } else {
                            "Requires a paired companion and a fresh repair permission"
                        })
                        .licon(crate::ui::icons::Icon::Repair)
                        .dim(!can_repair),
                    )
                    .row(
                        Row::new(if prepare {
                            "Confirm Get Ready"
                        } else {
                            "Get Ready"
                        })
                        .detail(if prepare {
                            "Press again to prepare the highlighted file"
                        } else {
                            "Prepare the highlighted file through the paired companion"
                        })
                        .licon(crate::ui::icons::Icon::ReadyStore)
                        .dim(!can_prepare),
                    ),
            );
        }
        self.table.compact = false;
        let max = self.row_count().saturating_sub(1) as i32;
        self.table
            .set_sections(sections, selected.clamp(0, max), false);
    }

    fn focus_row(&mut self, index: i32) {
        self.table.sel = index;
        if let Some(row) = usize::try_from(index).ok().and_then(|i| self.rows.get(i)) {
            if self.selected_part != row.part {
                self.selected_part = row.part.clone();
                self.choice = TrackChoice::default();
                self.companion = None;
                self.confirm = None;
                self.last_poll = None;
                self.rebuild_table(index);
            }
        }
    }

    fn observe<H: AppLike>(&mut self, now: u32, fx: &mut Effects<'_, H>) {
        if !self.active
            || self
                .last_poll
                .is_some_and(|last| now.wrapping_sub(last) < 1_000)
        {
            return;
        }
        let Some(key) = self.target() else {
            return;
        };
        self.last_poll = Some(now);
        fx.push(Fx::App(AppFx::Companion(CompanionRequest {
            key,
            action: CompanionAction::Observe,
        })));
    }

    pub(crate) fn frame(&self) -> Rect {
        let [x, y, w, h] = self.arg.anchor.map(f32::from_bits);
        panel_at(Rect::new(x, y, w, h), self.table.measured_height())
    }

    fn dismiss<H: AppLike>(&mut self, fx: &mut Effects<'_, H>) {
        self.active = false;
        self.confirm = None;
        fx.push(Fx::Nav(NavOp::Dismiss(self.entry)));
    }

    /// OK on a version starts it. Mutating companion actions require a second deliberate OK,
    /// are exact-Part scoped, and are re-authorized by the resource adapter's fresh observation.
    fn commit<H: AppLike>(&mut self, elem: u32, fx: &mut Effects<'_, H>) {
        if !self.active {
            return;
        }
        if (elem as usize) < self.rows.len() {
            if let Action::Play { sid, rk, part } = action_at(&self.rows, elem as i32) {
                self.dismiss(fx);
                fx.push(Fx::Deliver(
                    MachineId::Instance(self.arg.host),
                    Delivery::Screen(ScreenEvent::App(AppMsg::VersionSelected { sid, rk, part })),
                ));
            }
            return;
        }
        let track_index = (elem as usize).saturating_sub(self.rows.len());
        if let Some(row) = self.selected_row() {
            let mut choice = self.choice;
            let is_track = if let Some(option) = row.audio.get(track_index) {
                choice.audio = option.id;
                true
            } else if let Some(option) = track_index
                .checked_sub(row.audio.len())
                .and_then(|i| row.subs.get(i))
            {
                choice.subtitle = option.id;
                true
            } else {
                false
            };
            if is_track {
                fx.push(Fx::App(AppFx::Store(
                    crate::stores::StoreId::Metadata,
                    crate::stores::StoreCmd::Metadata(
                        crate::stores::metadata::MetadataCmd::SetPreplay {
                            sid: row.sid,
                            rk: row.rk.clone(),
                            part: row.part.clone(),
                            choice,
                        },
                    ),
                )));
                self.choice = choice;
                self.confirm = None;
                self.rebuild_table(elem as i32);
                fx.invalidate(crate::ui::present::Provenance::Input);
                return;
            }
        }
        let action = match (elem as usize).checked_sub(self.companion_offset()) {
            Some(0) => CompanionAction::Repair,
            Some(1) => CompanionAction::Prepare,
            _ => return,
        };
        let Some(key) = self.target() else {
            return;
        };
        let Some(state) = self
            .companion
            .as_ref()
            .filter(|state| state.key == key && !state.busy)
        else {
            return;
        };
        let allowed = match action {
            CompanionAction::Repair => state.can_repair,
            CompanionAction::Prepare => state.can_prepare,
            CompanionAction::Observe => false,
        };
        if !allowed {
            return;
        }
        if self.confirm != Some(action) {
            self.confirm = Some(action);
        } else {
            self.confirm = None;
            if let Some(state) = &mut self.companion {
                state.busy = true;
                state.can_prepare = false;
                state.can_repair = false;
                state.label = "Requesting".into();
            }
            fx.push(Fx::App(AppFx::Companion(CompanionRequest { key, action })));
        }
        self.rebuild_table(elem as i32);
        fx.invalidate(crate::ui::present::Provenance::Input);
    }
}

impl<H: AppLike<Memory = PageMemory> + crate::screens::registry::MetadataLike> Machine<H>
    for VersionsScreen
{
    type Ev = ScreenEvent<H>;
    fn step(&mut self, ev: &Self::Ev, cx: &Cx<'_, H>, fx: &mut Effects<'_, H>) -> Handled {
        if self.profile!=crate::plex::session::current_gen(){self.dismiss(fx);return Handled::Yes;}
        match ev {
            ScreenEvent::Mount
            | ScreenEvent::Enter(_)
            | ScreenEvent::Uncover
            | ScreenEvent::Resume => {
                self.active = true;
                self.last_poll = None;
                self.refresh(H::metadata(cx));
                Handled::Yes
            }
            ScreenEvent::WillLeave(_)
            | ScreenEvent::Unmount
            | ScreenEvent::Suspend
            | ScreenEvent::Cover => {
                self.active = false;
                self.confirm = None;
                Handled::Yes
            }
            ScreenEvent::App(AppMsg::Companion(snapshot)) => {
                if self.active
                    && self.target().as_ref() == Some(&snapshot.key)
                    && self.companion.as_ref() != Some(snapshot)
                {
                    self.companion = Some(snapshot.clone());
                    if snapshot.busy || !(snapshot.can_repair || snapshot.can_prepare) {
                        self.confirm = None;
                    }
                    self.rebuild_table(self.table.sel);
                    fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
                }
                Handled::Yes
            }
            ScreenEvent::StoreChanged(ord, _) => {
                if *ord == crate::stores::StoreId::Metadata.ord()
                    && (self.refresh(H::metadata(cx)) | self.sync_choice(H::metadata(cx)))
                {
                    fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
                }
                Handled::Yes
            }
            ScreenEvent::Tick(tick) => {
                // Re-derive the drawn selection from the engine's own tracked focus every tick,
                // exactly as `AccountMenuScreen::step` does — `FocusMoved` already keeps the two
                // in step, this is the belt to its suspenders for a focus change this screen was
                // not told about directly (a remembered-cursor seat on `Enter`, say).
                let selected = cx
                    .focus
                    .current
                    .filter(|key| key.entry == self.entry)
                    .map(|key| key.elem as i32)
                    .unwrap_or(self.table.sel);
                self.focus_row(selected);
                self.sync_choice(H::metadata(cx));
                self.observe(tick.ms, fx);
                self.table.update(tick.dt(), self.frame().h);
                Handled::Yes
            }
            ScreenEvent::FocusMoved { to, .. } => {
                self.focus_row(to.elem as i32);
                self.sync_choice(H::metadata(cx));
                fx.invalidate(crate::ui::present::Provenance::Input);
                Handled::Yes
            }
            // The engine's OK arm, for a `Bare` element, delivers `Activate` directly rather than
            // arming a hold — see `Focusable::groups` below. A pointer click on a registered row
            // stop reaches the same arm through the hit map's own `Activate::Immediate` policy.
            ScreenEvent::Activate(elem) => {
                self.commit(*elem, fx);
                Handled::Yes
            }
            ScreenEvent::PressCommit(_) => {
                if let Some(key) = cx.focus.current.filter(|key| key.entry == self.entry) {
                    self.commit(key.elem, fx);
                }
                Handled::Yes
            }
            // UP/DOWN, OK and the pointer are no longer read here at all — the engine's own
            // direction/OK/hit-map machinery drives them through `Focusable` and the arms above,
            // exactly as it does for every other Engine screen (`AccountMenuScreen` is the
            // worked example this module follows). A click BESIDE every registered row stop is a
            // HIT-MAP MISS, and `Style::Compact`'s `on_miss` (`OnMiss::Dismiss`) is what closes
            // the panel — the same "click outside a popover dismisses it" rule this arm used to
            // implement by hand against raw pointer coordinates.
            ScreenEvent::Input(input) => match input.kind {
                InputKind::Key {
                    key: Key::Back,
                    edge: Edge::Down,
                    ..
                } => {
                    if self.confirm.take().is_some() {
                        self.rebuild_table(self.table.sel);
                        fx.invalidate(crate::ui::present::Provenance::Input);
                    } else {
                        self.dismiss(fx);
                    }
                    Handled::Yes
                }
                _ => Handled::No,
            },
            _ => Handled::No,
        }
    }
}

/// **A single-column `Focusable` over the table's own cursor** (phase 12, D2) — the same shape
/// `AccountMenuScreen::groups` uses for its own `TableView`: one `GroupKind::Column` of
/// `self.rows.len()` `Bare` elements (OK/click activates on the down edge, never arms a hold —
/// there is nothing here to hold), `EdgeRule::Stop` on every side since this is a standalone
/// surface with no page to escape onto, and `place`/`neighbour` read straight off `TableView`'s
/// own row geometry (`row_frame`, `next_selectable`) rather than a second copy of it.
impl<H: AppLike<Memory = PageMemory>> Focusable<H> for VersionsScreen {
    fn groups(&self, _cx: &Cx<'_, H>, out: &mut Vec<GroupSpec>) {
        out.push(GroupSpec {
            id: GroupId(0),
            kind: GroupKind::Column,
            seat: Seat::Remembered,
            reachable: AxisMask::BOTH,
            edge: [EdgeRule::Stop; 4],
            extent: self.frame(),
            len: self.row_count(),
            elem: ElemKind::Bare,
        });
    }
    fn group_of(&self, elem: &u32, _cx: &Cx<'_, H>) -> Option<GroupId> {
        ((*elem as usize) < self.row_count()).then_some(GroupId(0))
    }
    fn neighbour(&self, key: FocusKey<u32>, dir: Dir, _cx: &Cx<'_, H>) -> Step<u32> {
        let next = match dir {
            Dir::Up => self.table.next_selectable(key.elem as i32, -1),
            Dir::Down => self.table.next_selectable(key.elem as i32, 1),
            _ => None,
        };
        match next {
            Some(i) => Step::Move(FocusKey {
                entry: self.entry,
                elem: i as u32,
            }),
            None => Step::Edge,
        }
    }
    fn place(&self, elem: &u32, _cx: &Cx<'_, H>, _at: At) -> Option<Placed> {
        let rect = self.table.row_frame(self.frame(), *elem as i32)?;
        Some(Placed {
            rect,
            rest_rect: rect,
            clip: self.frame(),
            index: Some(*elem),
        })
    }
    fn reconcile(&self, want: FocusKey<u32>, cx: &Cx<'_, H>) -> FocusKey<u32> {
        if self.group_of(&want.elem, cx).is_some() {
            want
        } else {
            FocusKey {
                entry: self.entry,
                elem: 0,
            }
        }
    }
    fn seat(&self, _g: GroupId, _from: Placed, _cx: &Cx<'_, H>) -> FocusKey<u32> {
        FocusKey {
            entry: self.entry,
            elem: self.table.sel.max(0) as u32,
        }
    }
}

impl LogicalState for VersionsScreen {
    fn write(&self, c: &mut Canon) {
        c.u32(self.profile);
        self.arg.write(c);
        c.seq(self.rows.len());
        for r in &self.rows {
            c.u32(u32::from(r.sid.raw()))
                .str(&r.rk)
                .str(&r.part)
                .bool(r.playable)
                .str(&r.label)
                .str(&r.detail)
                .option(r.value.as_deref(), |c, v| {
                    c.str(v);
                })
                .option(r.badge.as_deref(), |c, b| {
                    c.str(b);
                })
                .bool(r.checked);
            for options in [&r.audio, &r.subs] {
                c.seq(options.len());
                for option in options {
                    c.option(option.id, |c, id| {
                        c.u64(id as u64);
                    });
                    c.str(&option.label).str(&option.detail);
                }
            }
        }
        c.str(&self.selected_part);
        c.option(self.choice.audio, |c, id| {
            c.u64(id as u64);
        });
        c.option(self.choice.subtitle, |c, id| {
            c.u64(id as u64);
        });
        c.option(self.companion.as_ref(), |c, state| {
            c.u32(u32::from(state.key.sid.raw()))
                .str(&state.key.rk)
                .str(&state.key.part)
                .str(&state.label)
                .bool(state.configured)
                .bool(state.busy)
                .bool(state.can_repair)
                .bool(state.can_prepare);
        });
        c.option(self.last_poll, |c, value| {
            c.u32(value);
        });
        c.option(self.confirm, |c, action| {
            c.u8(match action {
                CompanionAction::Observe => 0,
                CompanionAction::Repair => 1,
                CompanionAction::Prepare => 2,
            });
        });
        c.bool(self.active);
        c.u32(self.table.sel as u32);
        self.table.write_motion(c);
    }
    fn probe(&self, out: &mut String) {
        out.push_str("versions");
    }
}

impl<H: AppLike<Memory = PageMemory> + crate::screens::registry::MetadataLike> Screen<H>
    for VersionsScreen
{
    fn name(&self) -> &'static str {
        "versions"
    }
    fn state(&self) -> &dyn LogicalState {
        self
    }
    fn crumb(&self, _cx: &Cx<'_, H>) -> Option<Cow<'_, str>> {
        None
    }
    fn prepare(&mut self, _b: &mut Budget, _cx: &Cx<'_, H>) {}
    /// The modal dim, asked for rather than drawn. Nothing is lifted back out of it: this picker
    /// hangs under the detail page's Source chip, and the chip is a control that says which copy
    /// is playing rather than the subject of the panel — unlike the card menu's tile or the
    /// profile menu's chip, there is nothing here whose dimming contradicts what the panel is
    /// about.
    ///
    /// **The ordering this replaces was load-bearing and is now the container's** (§16.3): the
    /// panel's ground once sampled the framebuffer (`Glass::CACHED`), so the dim had to be down
    /// before the backdrop was taken; the ground is the latched underlay field now, latched at the
    /// head of the dims from the undimmed page. `ModalStack::draw_scrims` draws it at the end of the PAGE pass,
    /// which is strictly earlier than the surface pass this `draw` runs in, and multiplies by the
    /// appear spring and by `nav::page_alpha` — the second of which the in-`draw` version could
    /// not reach, `DrawFrame::page_alpha` being the surface's own spring alone.
    fn scrim(&self) -> Scrim {
        // The PANEL role (`theme::underlay::DIM_PANEL`): a picker in the middle of the frame. The
        // page recedes; it is not blanked, and the hero behind stays readable.
        Scrim::dim(theme::underlay::DIM_PANEL)
    }
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, H>) {
        let appear = f.page_alpha;
        let r = self.frame();
        let p = f
            .painter
            .alpha(appear)
            .translate(0.0, RISE * (1.0 - appear));
        let measure = f.measure;
        // Named for `/tmp/plxnative-cpuprof` beside the page's own phases, so a slow frame while
        // this panel is up can be read as the PANEL or as the host under it.
        let field = f.underlay;
        crate::ui::profile::phase("dt.versions", || {
            crate::ui::widgets::panel_ground(p, r, PANEL_RAD, field);
            self.table.draw(p, r, measure);
        });
        // The hit map's stops are registered against the SETTLED geometry (`self.frame()`, what
        // `Focusable::place` answers) rather than the transient slide `p` draws with — at rest
        // (`appear == 1`) the two coincide exactly (`RISE * (1.0 - appear)` is 0), and only during
        // the open/close fade would they differ; `ui/hit.rs`'s own module doc already carries the
        // wider version of this gap (no container feeds a fading surface's alpha into the map
        // yet), so this keeps the one known edge rather than inventing a second.
        let hit_p = f.painter.alpha(appear);
        for elem in 0..self.row_count() as u32 {
            if let Some(placed) = <Self as Focusable<H>>::place(self, &elem, f.cx, At::Drawn) {
                f.stop(
                    hit_p,
                    Stop {
                        key: FocusKey {
                            entry: self.entry,
                            elem,
                        },
                        rect: placed.rect,
                        rest_rect: placed.rest_rect,
                        clip: placed.clip,
                        hover: Hover::Focus,
                        activate: Activate::Immediate,
                    },
                );
            }
        }
    }
    fn render(&self) -> RenderStrategy {
        RenderStrategy::Page
    }
    fn focus_source(&self) -> FocusSource {
        FocusSource::Engine
    }
    fn hit_source(&self) -> HitSource {
        HitSource::Engine
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
#[path = "versions_tests.rs"]
mod tests;
