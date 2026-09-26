//! On-demand text subtitles, shared by pre-play and the player. I/O stays on a worker;
//! each result belongs to one profile, server, title and selected Part.
use crate::plex::subtitles::Candidate;
use crate::screens::registry::{AppFx, AppLike, MetadataLike, PageMemory, PlayerLike, PlayerReq};
use crate::ui::machine::*;
use crate::ui::screen::*;
use crate::ui::table::{Row, Section, TableView};
use crate::ui::{theme, Rect};
use std::{
    borrow::Cow,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};
pub(crate) const SHAPE: &str = "SubtitleScreen{arg:{sid:u32,rk:str,part:str,playing:bool},profile:u32,language:u32,notice:str,sel:u32,pending:bool,rows:[{key:str,title:str,language:str,codec:str}]}";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SubtitleArg {
    pub sid: crate::plex::ServerId,
    pub rk: String,
    pub part: String,
    pub playing: bool,
}
impl LogicalState for SubtitleArg {
    fn write(&self, c: &mut Canon) {
        c.u32(self.sid.raw().into())
            .str(&self.rk)
            .str(&self.part)
            .bool(self.playing);
    }
    fn probe(&self, out: &mut String) {
        out.push_str("subtitle_arg");
    }
}
enum Reply {
    Results(Vec<Candidate>),
    Downloaded(crate::metadata::PlayingItem, Option<i64>),
    Failed(&'static str),
}
pub(crate) struct SubtitleScreen {
    entry: EntryId,
    arg: SubtitleArg,
    profile: u32,
    table: TableView,
    rows: Vec<Candidate>,
    notice: String,
    rx: Option<mpsc::Receiver<Reply>>,
    cancel: Arc<AtomicBool>,
    language: usize,
}
const LANGUAGES: &[(&str, &str)] = &[
    ("English", "en"),
    ("Lithuanian", "lt"),
    ("Spanish", "es"),
    ("French", "fr"),
    ("German", "de"),
    ("Italian", "it"),
    ("Portuguese", "pt"),
    ("Polish", "pl"),
    ("Ukrainian", "uk"),
    ("Russian", "ru"),
    ("Japanese", "ja"),
    ("Korean", "ko"),
    ("Chinese", "zh"),
    ("Swedish", "sv"),
    ("Norwegian", "no"),
    ("Danish", "da"),
    ("Finnish", "fi"),
    ("Dutch", "nl"),
    ("Turkish", "tr"),
    ("Hindi", "hi"),
    ("Arabic", "ar"),
];

impl SubtitleScreen {
    pub(crate) fn new(entry: EntryId, arg: SubtitleArg) -> Self {
        let mut s = Self {
            entry,
            arg,
            profile: crate::plex::session::current_gen(),
            table: TableView::new(),
            rows: vec![],
            notice: "Choose a language, then search".into(),
            rx: None,
            cancel: Arc::new(AtomicBool::new(false)),
            language: 0,
        };
        #[cfg(feature = "devtriggers")]
        if crate::dev::flag("subtitle-results-fixture") {
            s.rows = vec![
                Candidate {
                    key: "/library/streams/1".into(),
                    title: "Example Movie 2160p WEB-DL — full dialogue".into(),
                    codec: "srt".into(),
                    language: "English".into(),
                    language_code: "eng".into(),
                    provider: "OpenSubtitles".into(),
                },
                Candidate {
                    key: "/library/streams/2".into(),
                    title: "Example Movie BluRay — hearing impaired".into(),
                    codec: "srt".into(),
                    language: "English".into(),
                    language_code: "eng".into(),
                    provider: "OpenSubtitles".into(),
                },
            ];
            s.notice = "Choose a release matching your file".into();
        }
        s.rebuild();
        s
    }
    fn frame(&self) -> Rect {
        let height = self.table.measured_height().clamp(240.0, 800.0);
        Rect::new(490.0, (1080.0 - height) * 0.5, 940.0, height)
    }
    fn rebuild(&mut self) {
        let mut section = Section::new("Download subtitles")
            .row(
                Row::new(format!("Language: {}", LANGUAGES[self.language].0))
                    .detail("Press OK to change"),
            )
            .row(
                Row::new(if self.rx.is_some() {
                    "Working…"
                } else {
                    "Search subtitles"
                })
                .detail(self.notice.clone()),
            );
        for row in &self.rows {
            section = section.row(
                Row::new(if row.title.is_empty() {
                    row.language.clone()
                } else {
                    row.title.clone()
                })
                .detail(format!(
                    "{} · {} · {}",
                    row.language,
                    row.codec.to_uppercase(),
                    row.provider
                )),
            );
        }
        self.table
            .set_sections(vec![section], self.table.sel.max(0), false);
    }
    fn start(&mut self, choice: Option<Candidate>) {
        if self.rx.is_some() {
            return;
        }
        let Some(client) = crate::plex::client_for(self.arg.sid) else {
            self.notice = "Server unavailable. Reopen this item when it reconnects.".into();
            self.rebuild();
            return;
        };
        let generation = client.token_gen();
        let profile = self.profile;
        let arg = self.arg.clone();
        let cancel = self.cancel.clone();
        let language = LANGUAGES[self.language].1;
        let (tx, rx) = mpsc::channel();
        self.notice = if choice.is_some() {
            "Downloading the selected subtitle…"
        } else {
            "Searching Plex’s subtitle provider…"
        }
        .into();
        self.rx = Some(rx);
        if !crate::task::spawn_small("subtitle-search", move || {
            let valid = || {
                !cancel.load(Ordering::Relaxed)
                    && profile == crate::plex::session::current_gen()
                    && crate::plex::client_for(arg.sid).is_some_and(|c| c.token_gen() == generation)
            };
            let reply = if !valid() {
                return;
            } else if let Some(candidate) = choice {
                download(&arg, &candidate, &valid)
            } else {
                client
                    .search_subtitles(&arg.rk, language)
                    .map(Reply::Results)
                    .unwrap_or(Reply::Failed("Search unavailable. Press Search to retry."))
            };
            if valid() {
                let _ = tx.send(reply);
                crate::ui::present::wake_from_worker();
            }
        }) {
            self.rx = None;
            self.notice = "Could not start subtitle request. Try again.".into();
        }
        self.rebuild();
    }
    fn store<H: AppLike>(fx: &mut Effects<'_, H>, cmd: crate::stores::metadata::MetadataCmd) {
        fx.push(Fx::App(AppFx::Store(
            crate::stores::StoreId::Metadata,
            crate::stores::StoreCmd::Metadata(cmd),
        )));
    }
}
impl Drop for SubtitleScreen {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_download_never_selects_an_unrelated_or_ambiguous_subtitle() {
        let pick = Candidate {
            key: "/library/streams/900?provider=test".into(),
            title: "exact release".into(),
            codec: "srt".into(),
            language: "English".into(),
            language_code: "eng".into(),
            provider: "test".into(),
        };
        let before = Vec::new();
        let mut after = Vec::new();
        let stream = crate::metadata::Stream {
            id: 1,
            external: true,
            codec: "srt".into(),
            key: "/library/streams/1".into(),
            lang_code: "eng".into(),
            title: pick.title.clone(),
            ..Default::default()
        };
        after.push(stream.clone());
        assert_eq!(downloaded_id(&before, &after, &pick), Some(1));
        assert_eq!(
            downloaded_id(&after, &after, &pick),
            Some(1),
            "a previously downloaded exact release can be selected again"
        );
        after[0].title = "another release".into();
        assert_eq!(downloaded_id(&before, &after, &pick), None);
        after[0] = stream.clone();
        after.push(crate::metadata::Stream { id: 2, ..stream });
        assert_eq!(downloaded_id(&before, &after, &pick), None);
    }
    #[test]
    fn closing_a_search_cancels_its_worker_and_never_starts_network_on_mount() {
        let _guard = crate::testlock::serial();
        let screen = SubtitleScreen::new(
            EntryId(1),
            SubtitleArg {
                sid: crate::plex::ServerId::UNSET,
                rk: "42".into(),
                part: "/library/parts/7/file.mkv".into(),
                playing: false,
            },
        );
        assert!(screen.rx.is_none());
        assert_eq!(screen.table.n_rows(), 2);
        let cancel = screen.cancel.clone();
        drop(screen);
        assert!(cancel.load(Ordering::Relaxed));
    }
}

fn downloaded_id(
    before: &[crate::metadata::Stream],
    after: &[crate::metadata::Stream],
    pick: &Candidate,
) -> Option<i64> {
    let matches: Vec<_> = after
        .iter()
        .filter(|s| {
            s.sidecar_renderable()
                && !pick.title.is_empty()
                && s.lang_code == pick.language_code
                && s.title == pick.title
        })
        .collect();
    let fresh: Vec<_> = matches
        .iter()
        .filter(|s| !before.iter().any(|old| old.id == s.id))
        .collect();
    if fresh.len() == 1 {
        Some(fresh[0].id)
    } else {
        (matches.len() == 1).then(|| matches[0].id)
    }
}
fn download(arg: &SubtitleArg, pick: &Candidate, valid: &impl Fn() -> bool) -> Reply {
    let Some(client) = crate::plex::client_for(arg.sid) else {
        return Reply::Failed("Server unavailable");
    };
    let Some(before) = crate::metadata::fetch_playing_version(arg.sid, &arg.rk, &arg.part) else {
        return Reply::Failed("Selected file is unavailable. Reopen its details.");
    };
    let Some(item) = client.metadata(&arg.rk) else {
        return Reply::Failed("Could not check the selected file");
    };
    let matches: Vec<_> = item
        .media
        .iter()
        .filter(|m| m.part.len() == 1 && m.part[0].key == arg.part)
        .collect();
    if matches.len() != 1 || !valid() {
        return Reply::Failed("Selected file changed. Reopen its details.");
    }
    if !client.download_subtitle(&arg.rk, matches[0].id, pick) {
        return Reply::Failed(
            "Download was not confirmed. Check the subtitle list before retrying.",
        );
    }
    // PMS acknowledges before attaching the stream. Never resend the write while waiting.
    for _ in 0..12 {
        if !valid() {
            return Reply::Failed("Request cancelled");
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Some(after) = crate::metadata::fetch_playing_version(arg.sid, &arg.rk, &arg.part) {
            let id = downloaded_id(&before.subs, &after.subs, pick);
            if id.is_some() {
                return Reply::Downloaded(after, id);
            }
        }
    }
    Reply::Failed(
        "Plex is still attaching the subtitle. Close this panel and check the subtitle list.",
    )
}

impl<H: AppLike<Memory = PageMemory> + MetadataLike + PlayerLike> Machine<H> for SubtitleScreen {
    type Ev = ScreenEvent<H>;
    fn step(&mut self, ev: &Self::Ev, cx: &Cx<'_, H>, fx: &mut Effects<'_, H>) -> Handled {
        let meta = H::metadata(cx);
        if self.profile != crate::plex::session::current_gen()
            || (self.arg.playing
                && !meta.playing().is_some_and(|p| {
                    p.sid == self.arg.sid
                        && p.rk == self.arg.rk
                        && p.version
                            .as_ref()
                            .is_some_and(|v| v.source.part_key == self.arg.part)
                }))
        {
            self.cancel.store(true, Ordering::Relaxed);
            fx.push(Fx::Nav(NavOp::Dismiss(self.entry)));
            return Handled::Yes;
        }
        match ev {
            ScreenEvent::WillLeave(_) | ScreenEvent::Unmount => {
                self.cancel.store(true, Ordering::Relaxed);
                self.rx = None;
                Handled::Yes
            }
            ScreenEvent::Input(input)
                if matches!(
                    input.kind,
                    InputKind::Key {
                        key: Key::Back,
                        edge: Edge::Down,
                        ..
                    }
                ) =>
            {
                self.cancel.store(true, Ordering::Relaxed);
                fx.push(Fx::Nav(NavOp::Dismiss(self.entry)));
                Handled::Yes
            }
            ScreenEvent::FocusMoved { to, .. } => {
                self.table.sel = to.elem as i32;
                Handled::Yes
            }
            ScreenEvent::Input(input)
                if self.arg.playing
                    && matches!(input.kind,
                InputKind::Key {sym, wcode, ..} if matches!(crate::ui::consts::classify(sym, wcode),
                    crate::ui::consts::Key::Play | crate::ui::consts::Key::Pause | crate::ui::consts::Key::PlayPause)) =>
            {
                if let InputKind::Key {
                    sym, wcode, edge, ..
                } = input.kind
                {
                    use crate::ui::consts::Key as RemoteKey;
                    let key = crate::ui::consts::classify(sym, wcode);
                    if matches!(
                        key,
                        RemoteKey::Play | RemoteKey::Pause | RemoteKey::PlayPause
                    ) {
                        if edge == Edge::Down {
                            fx.push(Fx::App(AppFx::Player(PlayerReq::Transport(match key {
                                RemoteKey::Play => Some(true),
                                RemoteKey::Pause => Some(false),
                                _ => None,
                            }))));
                        }
                        return Handled::Yes;
                    }
                }
                Handled::No
            }
            ScreenEvent::Input(input)
                if self.table.sel == 0
                    && self.rx.is_none()
                    && matches!(
                        input.kind,
                        InputKind::Key {
                            key: Key::Left | Key::Right,
                            edge: Edge::Down,
                            ..
                        }
                    ) =>
            {
                let backwards = matches!(input.kind, InputKind::Key { key: Key::Left, .. });
                self.language = (self.language + if backwards { LANGUAGES.len() - 1 } else { 1 })
                    % LANGUAGES.len();
                self.rows.clear();
                self.notice = "Press Search to find subtitles".into();
                self.rebuild();
                fx.invalidate(crate::ui::present::Provenance::Input);
                Handled::Yes
            }
            ScreenEvent::Activate(elem) => {
                if self.rx.is_none() {
                    match *elem {
                        0 => {
                            self.language = (self.language + 1) % LANGUAGES.len();
                            self.rows.clear();
                            self.notice = "Press Search to find subtitles".into();
                            self.rebuild();
                        }
                        1 => self.start(None),
                        i => {
                            if let Some(pick) = self.rows.get(i as usize - 2).cloned() {
                                self.start(Some(pick));
                            }
                        }
                    }
                }
                fx.invalidate(crate::ui::present::Provenance::Input);
                Handled::Yes
            }
            ScreenEvent::Tick(tick) => {
                if let Some(reply) = self.rx.as_ref().and_then(|rx| match rx.try_recv() {
                    Ok(reply) => Some(reply),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Reply::Failed("Subtitle request stopped. Try again."))
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                }) {
                    self.rx = None;
                    match reply {
                        Reply::Results(rows) => {
                            self.notice = if rows.is_empty() {
                                "No text subtitles found. Try another language."
                            } else {
                                "Choose a release matching your file"
                            }
                            .into();
                            self.rows = rows;
                        }
                        Reply::Failed(msg) => self.notice = msg.into(),
                        Reply::Downloaded(item, id) => {
                            self.notice =
                                "Downloaded. Close this panel to return to subtitles.".into();
                            if let Some(stream) =
                                id.and_then(|id| item.subs.iter().find(|s| s.id == id))
                            {
                                if self.arg.playing {
                                    fx.push(Fx::App(AppFx::Player(PlayerReq::CommitTrack(
                                        crate::ui::track_menu::TrackCommit::Subtitle {
                                            render_ordinal: -1,
                                            stream_id: stream.id,
                                            sidecar_key: Some(stream.key.clone()),
                                        },
                                    ))));
                                } else {
                                    let mut choice =
                                        meta.preplay(self.arg.sid, &self.arg.rk, &self.arg.part);
                                    choice.subtitle = Some(stream.id);
                                    Self::store(
                                        fx,
                                        crate::stores::metadata::MetadataCmd::SetPreplay {
                                            sid: self.arg.sid,
                                            rk: self.arg.rk.clone(),
                                            part: self.arg.part.clone(),
                                            choice,
                                        },
                                    );
                                }
                            }
                            if self.arg.playing {
                                Self::store(
                                    fx,
                                    crate::stores::metadata::MetadataCmd::InstallPlaying(Some(
                                        item,
                                    )),
                                );
                            }
                            if let Some(current) = meta.current() {
                                Self::store(
                                    fx,
                                    crate::stores::metadata::MetadataCmd::RequestDetail {
                                        sid: current.sid,
                                        rk: current.rk.clone(),
                                    },
                                );
                            }
                        }
                    }
                    self.rebuild();
                    fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
                }
                self.table.update(tick.dt(), self.frame().h);
                if self.arg.playing {
                    fx.push(Fx::App(AppFx::Player(PlayerReq::ExtendHud(30_000))));
                }
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}
impl LogicalState for SubtitleScreen {
    fn write(&self, c: &mut Canon) {
        self.arg.write(c);
        c.u32(self.profile)
            .u32(self.language as u32)
            .str(&self.notice)
            .u32(self.table.sel as u32)
            .bool(self.rx.is_some());
        c.seq(self.rows.len());
        for row in &self.rows {
            c.str(&row.key)
                .str(&row.title)
                .str(&row.language)
                .str(&row.codec);
        }
    }
    fn probe(&self, out: &mut String) {
        out.push_str("subtitle_search");
    }
}
impl<H: AppLike<Memory = PageMemory>> Focusable<H> for SubtitleScreen {
    fn groups(&self, _: &Cx<'_, H>, out: &mut Vec<GroupSpec>) {
        out.push(GroupSpec {
            id: GroupId(0),
            kind: GroupKind::Column,
            seat: Seat::Remembered,
            reachable: AxisMask::BOTH,
            edge: [EdgeRule::Stop; 4],
            extent: self.frame(),
            len: self.table.n_rows() as usize,
            elem: ElemKind::Bare,
        });
    }
    fn group_of(&self, elem: &u32, _: &Cx<'_, H>) -> Option<GroupId> {
        (*elem < self.table.n_rows() as u32).then_some(GroupId(0))
    }
    fn neighbour(&self, key: FocusKey<u32>, dir: Dir, _: &Cx<'_, H>) -> Step<u32> {
        match dir {
            Dir::Up | Dir::Down => self
                .table
                .next_selectable(key.elem as i32, if dir == Dir::Up { -1 } else { 1 })
                .map(|i| {
                    Step::Move(FocusKey {
                        entry: self.entry,
                        elem: i as u32,
                    })
                })
                .unwrap_or(Step::Edge),
            _ => Step::Edge,
        }
    }
    fn place(&self, elem: &u32, _: &Cx<'_, H>, _: At) -> Option<Placed> {
        let r = self.table.row_frame(self.frame(), *elem as i32)?;
        Some(Placed {
            rect: r,
            rest_rect: r,
            clip: self.frame(),
            index: Some(*elem),
        })
    }
    fn reconcile(&self, want: FocusKey<u32>, _: &Cx<'_, H>) -> FocusKey<u32> {
        FocusKey {
            entry: self.entry,
            elem: want.elem.min(self.table.n_rows().saturating_sub(1) as u32),
        }
    }
    fn seat(&self, _: GroupId, _: Placed, _: &Cx<'_, H>) -> FocusKey<u32> {
        FocusKey {
            entry: self.entry,
            elem: 0,
        }
    }
}
impl<H: AppLike<Memory = PageMemory> + MetadataLike + PlayerLike> Screen<H> for SubtitleScreen {
    fn name(&self) -> &'static str {
        "subtitle_search"
    }
    fn state(&self) -> &dyn LogicalState {
        self
    }
    fn crumb(&self, _: &Cx<'_, H>) -> Option<Cow<'_, str>> {
        None
    }
    fn prepare(&mut self, _: &mut crate::ui::frame::Budget, _: &Cx<'_, H>) {}
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, H>) {
        let r = self.frame();
        let p = f.painter.alpha(f.page_alpha);
        crate::ui::widgets::panel_ground(p, r, theme::ALERT_PANEL_RAD, f.underlay);
        self.table.draw(p, r, f.measure);
        for i in 0..self.table.n_rows() as u32 {
            if let Some(rect) = self.table.row_frame(r, i as i32) {
                f.stop(
                    p,
                    Stop {
                        key: FocusKey {
                            entry: self.entry,
                            elem: i,
                        },
                        rect,
                        rest_rect: rect,
                        clip: r,
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
    fn scrim(&self) -> Scrim {
        if self.arg.playing {
            Scrim::over_video(theme::underlay::DIM_PLAYER, None)
        } else {
            Scrim::dim(theme::underlay::DIM_PANEL)
        }
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
