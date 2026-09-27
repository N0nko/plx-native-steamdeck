//! Catalog destination with explicit, profile-scoped Watchlist actions.
//! Never treat a provider metadata ID as a server-local ratingKey or offer a false Play action.
use crate::screens::registry::{AppFx, AppLike, AppMsg};
use crate::ui::machine::{
    Canon, Cx, Edge, Effects, EntryId, FocusKey, Fx, GroupId, Handled, InputKind, Key,
    LogicalState, Machine, NavOp,
};
use crate::ui::screen::{
    At, AxisMask, Dir, DrawFrame, EdgeRule, ElemKind, Enter, FocusSource, FocusTarget, Focusable,
    GroupKind, GroupSpec, HitSource, Placed, RenderStrategy, Screen, ScreenEvent, Scrim, Seat,
    Step,
};
use crate::ui::text_view::TextView;
use crate::ui::{theme, Rect};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CatalogDetailsArg {
    pub(crate) catalog_id: String,
    pub(crate) title: String,
    pub(crate) person: String,
    pub(crate) department: String,
    pub(crate) role: String,
    pub(crate) year: i32,
}
impl LogicalState for CatalogDetailsArg {
    fn write(&self, c: &mut Canon) {
        c.str(&self.catalog_id)
            .str(&self.title)
            .str(&self.person)
            .str(&self.department)
            .str(&self.role)
            .u32(self.year as u32);
    }
    fn probe(&self, out: &mut String) {
        out.push_str("catalog_credit");
    }
}
pub(crate) const SHAPE: &str = "CatalogDetails{watchlist_v4:profile_status_summary_local_sid_rk_poster_sid_thumb,ratings:[{art:u32,value:f64,critic:bool}],trailer:opt<{key:str,title:str}>,catalog_id:str,title:str,person:str,department:str,role:str,year:i32}";
pub(crate) struct CatalogDetailsScreen {
    entry: EntryId,
    arg: CatalogDetailsArg,
    state: Option<crate::catalog_action::Snapshot>,
    profile: u32,
}
const WATCHLIST: u32 = 1;
const LIBRARY: u32 = 2;
const TRAILER: u32 = 3;
const PANEL: Rect = Rect::new(336.0, 208.0, 1248.0, 664.0);
const POSTER: Rect = Rect::new(384.0, 256.0, 250.0, 375.0);
const TEXT_X: f32 = 666.0;
const TEXT_W: f32 = 870.0;
const BUTTON_Y: f32 = 720.0;
fn button_rect(elem: u32, buttons: &[u32]) -> Rect {
    let width=(1152.0 - 16.0 * (buttons.len()-1) as f32) / buttons.len() as f32;
    let index=buttons.iter().position(|e|*e==elem).unwrap_or(0);
    Rect::new(384.0 + index as f32 * (width+16.0), BUTTON_Y, width, 64.0)
}
impl CatalogDetailsScreen {
    pub(crate) fn new(entry: EntryId, arg: CatalogDetailsArg) -> Self {
        Self {
            entry,
            arg,
            state: None,
            profile: 0,
        }
    }
    fn has_local(&self)->bool {self.state.as_ref().is_some_and(|s|s.local.is_some())}
    fn buttons(&self) -> Vec<u32> {
        let mut buttons=Vec::new();
        if self.has_local(){buttons.push(LIBRARY);}
        if self.state.as_ref().is_some_and(|s|s.trailer.is_some()){buttons.push(TRAILER);}
        buttons.push(WATCHLIST);
        buttons
    }
    fn activate<H:AppLike>(&mut self, elem:u32, fx:&mut Effects<'_,H>) {
        if self.profile!=crate::plex::session::current_gen(){return;}
        let Some(state)=self.state.as_ref().filter(|s|!s.busy) else{return;};
        if elem==LIBRARY {
            if let Some((sid,rk))=&state.local {
                fx.push(Fx::App(AppFx::Content(crate::screens::registry::ContentReq::Present(
                    crate::screens::registry::ContentArg::Detail{sid:*sid,rk:rk.clone()}))));
            }
        } else if elem==TRAILER {
            if let Some(trailer)=&state.trailer {
                fx.push(Fx::App(AppFx::Content(crate::screens::registry::ContentReq::Play {
                    play: crate::screens::registry::PlayIntent::Item {
                        sid: crate::plex::server_ids().next().unwrap_or(crate::plex::ServerId::UNSET),
                        rk: String::new(), part: trailer.key.clone(), vcodec: "h264".into(), acodec: "aac".into(),
                        title: state.title.clone(), context: crate::metadata::TRAILER_CONTEXT.into(),
                    }, resume_ns: 0,
                })));
            }
        } else if elem==WATCHLIST {
            let request=crate::catalog_action::Request{id:self.arg.catalog_id.clone(),profile:self.profile,desired:state.watchlisted.map(|v|!v)};
            if let Some(state)=&mut self.state {state.busy=true;}
            fx.push(Fx::App(AppFx::Catalog(request)));
            fx.invalidate(crate::ui::present::Provenance::Input);
        }
    }
}
impl LogicalState for CatalogDetailsScreen {
    fn write(&self, c: &mut Canon) {
        self.arg.write(c);
        c.u32(self.profile);
        c.option(self.state.as_ref(), |c, s| {
            c.bool(s.busy).bool(s.failed).str(&s.summary).str(&s.title);
            c.option(s.local.as_ref(),|c,(sid,rk)|{c.u32(sid.raw() as u32).str(rk);});
            c.option(s.poster.as_ref(),|c,(sid,thumb)|{c.u32(sid.raw() as u32).str(thumb);});
            c.seq(s.ratings.len());
            for r in &s.ratings { c.u32(r.art as u32).u64(r.value.to_bits()).bool(r.critic); }
            c.option(s.trailer.as_ref(), |c,t| { c.str(&t.key).str(&t.title); });
            c.option(s.watchlisted, |c, v| {
                c.bool(v);
            });
        });
    }
    fn probe(&self, out: &mut String) {
        out.push_str("catalog_details");
    }
}
impl<H: AppLike> Machine<H> for CatalogDetailsScreen {
    type Ev = ScreenEvent<H>;
    fn step(&mut self, ev: &Self::Ev, cx: &Cx<'_, H>, fx: &mut Effects<'_, H>) -> Handled {
        match ev {
            ScreenEvent::Mount => {
                self.profile = crate::plex::session::current_gen();
                let request = crate::catalog_action::Request {
                    id: self.arg.catalog_id.clone(),
                    profile: self.profile,
                    desired: None,
                };
                self.state = Some(crate::catalog_action::Snapshot::pending(&request));
                #[cfg(feature="devtriggers")]
                if crate::dev::flag("catalog-details-fixture") {
                    let mut state=crate::catalog_action::Snapshot::pending(&request);
                    state.title="Dune".into(); state.year=2021; state.busy=false; state.watchlisted=Some(false);
                    state.summary="A young heir travels to the desert planet Arrakis, where rival houses compete for a resource that determines the future of the galaxy.".into();
                    state.local=Some((crate::plex::ServerId::from_raw(1),"42".into()));
                    state.ratings=crate::metadata::convert_ratings(&serde_json::from_value(serde_json::json!({"Rating":[
                        {"image":"imdb://image.rating","value":8.0,"type":"audience"},
                        {"image":"rottentomatoes://image.rating.ripe","value":8.3,"type":"critic"},
                        {"image":"rottentomatoes://image.rating.upright","value":9.0,"type":"audience"},
                        {"image":"themoviedb://image.rating","value":7.8,"type":"audience"}]})).unwrap());
                    state.trailer=Some(crate::plex::catalog::Trailer {title:"Dune".into(),key:"/library/metadata/5d776cac47dd6e001f6ee059/extras/620427be133b2900559ccaa1/parts/mp4-2500".into()});
                    self.state=Some(state);
                } else { fx.push(Fx::App(AppFx::Catalog(request))); }
                #[cfg(not(feature="devtriggers"))]
                fx.push(Fx::App(AppFx::Catalog(request)));
                fx.push(Fx::Deliver(
                    fx.from(),
                    crate::ui::machine::Delivery::Screen(ScreenEvent::Enter(Enter::Fresh {
                        focus: FocusTarget::Elem(FocusKey {
                            entry: self.entry,
                            elem: WATCHLIST,
                        }),
                    })),
                ));
            }
            ScreenEvent::App(AppMsg::Catalog(state))
                if state.id == self.arg.catalog_id && state.profile == self.profile =>
            {
                self.state = Some(state.clone());
                fx.invalidate(crate::ui::present::Provenance::Landing(fx.from()));
            }
            ScreenEvent::Activate(elem)=>self.activate(*elem,fx),
            ScreenEvent::PressCommit(_)=>{if let Some(key)=cx.focus.current {self.activate(key.elem,fx);}},
            ScreenEvent::Tick(_) if self.profile != crate::plex::session::current_gen() => {
                fx.push(Fx::Nav(NavOp::Dismiss(self.entry)));
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
                fx.push(Fx::Nav(NavOp::Dismiss(self.entry)));
            }
            _ => return Handled::No,
        }
        Handled::Yes
    }
}
impl<H: AppLike> Focusable<H> for CatalogDetailsScreen {
    fn groups(&self, _: &Cx<'_, H>, out: &mut Vec<GroupSpec>) {
        out.push(GroupSpec {
            id: GroupId(1),
            kind: GroupKind::Row { wrap: false },
            seat: Seat::First,
            reachable: AxisMask::BOTH,
            edge: [EdgeRule::Stop; 4],
            extent: Rect::new(384.0,BUTTON_Y,1152.0,64.0),
            len: self.buttons().len(),
            elem: ElemKind::Control,
        });
    }
    fn group_of(&self, e: &u32, _: &Cx<'_, H>) -> Option<GroupId> {
        self.buttons().contains(e).then_some(GroupId(1))
    }
    fn neighbour(&self, from: FocusKey<u32>, dir: Dir, _: &Cx<'_, H>) -> Step<u32> {
        let buttons=self.buttons();
        if let Some(index)=buttons.iter().position(|e|*e==from.elem) {
            let next=match dir {Dir::Left=>index.checked_sub(1),Dir::Right=>Some(index+1),_=>None};
            if let Some(elem)=next.and_then(|i|buttons.get(i)) {return Step::Move(FocusKey{entry:self.entry,elem:*elem});}
        }
        Step::Edge
    }
    fn place(&self, e: &u32, _: &Cx<'_, H>, _: At) -> Option<Placed> {
        let buttons=self.buttons();
        buttons.contains(e).then_some(Placed {
            rect: button_rect(*e,&buttons),
            rest_rect: button_rect(*e,&buttons),
            clip: Rect::new(0.0, 0.0, 1920.0, 1080.0),
            index: buttons.iter().position(|i|i==e).map(|i|i as u32),
        })
    }
    fn reconcile(&self, want: FocusKey<u32>, _: &Cx<'_, H>) -> FocusKey<u32> {
        if !self.buttons().contains(&want.elem){FocusKey{entry:self.entry,elem:WATCHLIST}}else{want}
    }
    fn seat(&self, _: GroupId, _: Placed, _: &Cx<'_, H>) -> FocusKey<u32> {
        FocusKey {
            entry: self.entry,
            elem: if self.has_local(){LIBRARY}else{WATCHLIST},
        }
    }
}
impl<H: AppLike> Screen<H> for CatalogDetailsScreen {
    fn name(&self) -> &'static str {
        "catalog_details"
    }
    fn state(&self) -> &dyn LogicalState {
        self
    }
    fn crumb(&self, _: &Cx<'_, H>) -> Option<std::borrow::Cow<'_, str>> {
        None
    }
    fn prepare(&mut self, _: &mut crate::ui::frame::Budget, _: &Cx<'_, H>) {}
    fn scrim(&self) -> Scrim {
        Scrim::dim(theme::underlay::DIM_PANEL)
    }
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, H>) {
        let r = PANEL;
        let p = f.painter.alpha(f.page_alpha);
        crate::ui::widgets::panel_ground(p, r, theme::ALERT_PANEL_RAD, f.underlay);
        let x = TEXT_X;
        let w = TEXT_W;
        if let Some((sid, thumb))=self.state.as_ref().and_then(|s|s.poster.as_ref()) {
            crate::ui::widgets::card(p, POSTER, crate::ui::widgets::Art::Thumb{sid:*sid,key:thumb,res:(250,375)}, theme::CARD_RING_RAD, false, 1.0, 0.0);
        }
        let text = |s: &str, y, size, ink, lines| {
            TextView::new(s, size, ink)
                .with_measure(f.measure)
                .max_lines(lines)
                .draw(p, Rect::new(x, r.y + y, w, 0.0));
        };
        text(
            "CATALOG",
            48.0,
            theme::size::CAPTION,
            theme::TEXT_TERTIARY,
            1,
        );
        text(
            self.state
                .as_ref()
                .map(|s| s.title.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(&self.arg.title),
            94.0,
            theme::size::HEADLINE,
            theme::TEXT_HEADING,
            2,
        );
        let year_value=self.state.as_ref().map_or(self.arg.year,|s|if s.year>0{s.year}else{self.arg.year});
        let year = if year_value > 0 {
            year_value.to_string()
        } else {
            "Year not supplied".into()
        };
        text(&year, 180.0, theme::size::CAPTION, theme::TEXT_SECONDARY, 1);
        let credit = [&self.arg.person,&self.arg.department,&self.arg.role].into_iter().filter(|s|!s.trim().is_empty()).map(String::as_str).collect::<Vec<_>>().join(" · ");
        if !credit.is_empty(){text(&credit, 220.0, theme::size::CAPTION, theme::TEXT_SECONDARY, 1);}
        if let Some(state)=&self.state {
            crate::ui::fmt::draw_ratings(p, &state.ratings, x, r.y+if credit.is_empty(){236.0}else{276.0}, x+w, f.measure);
        }
        let summary = self
            .state
            .as_ref()
            .map(|s| s.summary.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("Catalog information is unavailable.");
        text(summary, if credit.is_empty(){282.0}else{322.0}, theme::size::BODY, theme::TEXT_READING, if credit.is_empty(){4}else{3});
        let label = match self.state.as_ref() {
            Some(s) if s.busy => c"Loading…",
            Some(s) if s.failed => c"Retry Watchlist status",
            Some(s) if s.watchlisted == Some(true) => c"Remove from Watchlist",
            _ => c"Add to Watchlist",
        };
        use crate::ui::View;
        let buttons=self.buttons();
        for elem in buttons.iter().copied() {
        let label=match elem {LIBRARY=>c"Open in Library",TRAILER=>c"Watch Trailer",_=>label};
        let rect=button_rect(elem,&buttons);
        crate::ui::widgets::Button::new(label.as_ptr(), theme::size::BODY, rect)
            .focused(f.cx.focus.current.is_some_and(|key|key.entry==self.entry && key.elem==elem))
            .draw(&crate::ui::Env::inert(), p);
        f.stop(
            f.painter,
            crate::ui::screen::Stop {
                key: FocusKey {
                    entry: self.entry,
                    elem,
                },
                rect,
                rest_rect: rect,
                clip: Rect::new(0.0, 0.0, 1920.0, 1080.0),
                hover: crate::ui::screen::Hover::Focus,
                activate: crate::ui::screen::Activate::Immediate,
            },
        );
        }

        let hint = crate::ui::widgets::KeyHint::new(c"Press", c"BACK", c"to return");
        hint.draw(p, r.x+r.w-theme::alert::PAD-hint.width(f.measure), r.y+r.h-theme::space::MD-crate::ui::widgets::KeyHint::height()/2.0, f.measure);
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
