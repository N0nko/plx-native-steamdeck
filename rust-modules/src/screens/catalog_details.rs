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
    pub(crate) inline: Option<[i32; 2]>,
    pub(crate) host_glass: bool,
    pub(crate) parent: Option<crate::ui::machine::InstanceId>,
    pub(crate) poster: Option<(crate::plex::ServerId, String)>,
}
impl CatalogDetailsArg {
    pub(crate) const INLINE_W: f32 = 1040.0;
    pub(crate) const INLINE_H: f32 = crate::ui::consts::CARD_H;
}
impl LogicalState for CatalogDetailsArg {
    fn write(&self, c: &mut Canon) {
        c.bool(self.host_glass);
        c.option(self.parent,|c,p|{c.u32(p.0);});
        c.option(self.inline, |c, a| { c.u32(a[0] as u32).u32(a[1] as u32); });
        c.option(self.poster.as_ref(), |c,(sid,key)| { c.u32(sid.raw() as u32).str(key); });
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
pub(crate) const SHAPE: &str = "CatalogDetails{inline_v2:host_glass:bool,cast_row:motion,preview:{dwell:f32,requested:bool,promoted:bool,chrome:f32},credits:[{tag,role,thumb,guid}],opt<[i32;2]>,poster:opt<(sid,key)>,full:bool,extended:{kind,facts,genres,cast,backdrop},watchlist_v4:profile_status_summary_local_sid_rk_poster_sid_thumb,ratings:[{art:u32,value:f64,critic:bool}],trailer:opt<{key:str,title:str}>,catalog_id:str,title:str,person:str,department:str,role:str,year:i32}";
pub(crate) struct CatalogDetailsScreen {
    entry: EntryId,
    arg: CatalogDetailsArg,
    state: Option<crate::catalog_action::Snapshot>,
    profile: u32,
    full: bool,
    preview_dwell: f32,
    preview_requested: bool,
    preview_promoted: bool,
    chrome: f32,
    trailer_ctl: crate::ui::trailer::Transport,
    cast_row: crate::ui::card_row::CardRow,
}
const WATCHLIST: u32 = 1;
const LIBRARY: u32 = 2;
const TRAILER: u32 = 3;
const INFO: u32 = 4;
const COMPACT_PAD: f32 = 18.0;
const CAST: u32 = 100;
impl CatalogDetailsScreen {
    pub(crate) fn new(entry: EntryId, arg: CatalogDetailsArg) -> Self {
        let full = arg.inline.is_none();
        Self {
            full,
            entry,
            arg,
            state: None,
            profile: 0,
            preview_dwell: 0.0, preview_requested: false, preview_promoted: false, chrome: 1.0,
            trailer_ctl: crate::ui::trailer::Transport::IDLE,
            cast_row: crate::ui::card_row::CardRow::new(),
        }
    }
    fn has_local(&self)->bool {self.state.as_ref().is_some_and(|s|s.local.is_some())}
    fn buttons(&self) -> Vec<u32> {
        let mut buttons=Vec::new();
        if self.has_local(){buttons.push(LIBRARY);}
        if !self.full && self.state.as_ref().is_some_and(|s|s.trailer.is_some()){buttons.push(TRAILER);}
        buttons.push(WATCHLIST);
        buttons
    }
    fn panel(&self) -> Rect {
        if self.full { Rect::FULL } else {
            let [x,y]=self.arg.inline.unwrap_or([410,420]);
            Rect::new(x as f32,y as f32,CatalogDetailsArg::INLINE_W,CatalogDetailsArg::INLINE_H)
        }
    }
    fn info_rect(&self) -> Rect {
        let r=self.panel();
        if self.full { Rect::new(96.0,150.0,1728.0,400.0) }
        else { Rect::new(r.x+12.0,r.y+12.0,r.w-120.0,r.h-24.0) }
    }
    fn button_rect(&self, elem:u32) -> Rect {
        let buttons=self.buttons();
        let index=buttons.iter().position(|e|*e==elem).unwrap_or(0) as f32;
        if self.full {
            Rect::new(442.0+index*200.0,590.0,64.0,64.0)
        } else {
            let r=self.panel();
            let height=buttons.len() as f32*80.0-16.0;
            Rect::new(r.x+r.w-88.0,r.y+(r.h-height)/2.0+index*80.0,64.0,64.0)
        }
    }
    fn select_info<H:AppLike>(&mut self, fx:&mut Effects<'_,H>) {
        if self.full {return;}
        self.full=true;
        self.preview_dwell=0.0; self.preview_requested=false;
        fx.push(Fx::Deliver(fx.from(), crate::ui::machine::Delivery::Screen(ScreenEvent::Enter(
            Enter::Fresh {focus:FocusTarget::Elem(FocusKey{entry:self.entry,elem:self.buttons()[0]})}))));
        fx.invalidate(crate::ui::present::Provenance::Input);
    }
    fn back<H:AppLike>(&mut self, fx:&mut Effects<'_,H>) {
        self.stop_preview(fx);
        if self.full && self.arg.inline.is_some() {
            self.full=false;
            fx.push(Fx::Deliver(fx.from(), crate::ui::machine::Delivery::Screen(ScreenEvent::Enter(
                Enter::Fresh {focus:FocusTarget::Elem(FocusKey{entry:self.entry,elem:INFO})}))));
            fx.invalidate(crate::ui::present::Provenance::Input);
        } else { fx.push(Fx::Nav(NavOp::Dismiss(self.entry))); }
    }
    fn activate<H:AppLike>(&mut self, elem:u32, fx:&mut Effects<'_,H>) {
        if self.preview_promoted || self.profile!=crate::plex::session::current_gen(){return;}
        if elem==INFO { self.select_info(fx); return; }
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
        } else if elem>=CAST {
            if let Some(person)=state.credits.get((elem-CAST) as usize) {
                fx.push(Fx::App(AppFx::Content(crate::screens::registry::ContentReq::Present(
                    crate::screens::registry::ContentArg::Person {
                        sid:crate::plex::server_ids().next().unwrap_or(crate::plex::ServerId::UNSET),
                        key:String::new(),guid:person.tag_key.clone(),name:person.tag.clone(),thumb:person.thumb.clone(),
                    }))));
            }
        } else if elem==WATCHLIST {
            let request=crate::catalog_action::Request{id:self.arg.catalog_id.clone(),profile:self.profile,desired:state.watchlisted.map(|v|!v)};
            if let Some(state)=&mut self.state {state.busy=true;}
            fx.push(Fx::App(AppFx::Catalog(request)));
            fx.invalidate(crate::ui::present::Provenance::Input);
        }
    }
    fn cast_len(&self)->usize {if self.full {self.state.as_ref().map_or(0,|s|s.credits.len())} else {0}}
    fn cast_rect(&self, i:usize)->Rect {
        let sty=crate::ui::card_row::RowStyle::CAST;
        Rect::new(sty.margin_x+i as f32*(sty.w+sty.gap)-self.cast_row.scroll_x(),748.0,sty.w,sty.h)
    }
    fn stop_preview<H:AppLike>(&mut self,fx:&mut Effects<'_,H>) {
        if self.preview_requested {
            fx.push(Fx::App(AppFx::Content(crate::screens::registry::ContentReq::PreviewStop)));
        }
        self.preview_promoted=false; self.preview_dwell=0.0;
        self.trailer_ctl.dismiss();
    }
    fn preview_tick<H:AppLike>(&mut self,t:crate::ui::machine::Tick,cx:&Cx<'_,H>,fx:&mut Effects<'_,H>) {
        use crate::screens::registry::ContentReq;
        if cx.owner!=crate::ui::machine::InputOwner::Entry(self.entry) {return;}
        let focus=cx.focus.current.filter(|k|k.entry==self.entry).map(|k|k.elem);
        let (_,cast_motion)=crate::ui::idle::scoped_motion(|| self.cast_row.update(self.cast_len(),focus.filter(|e|*e>=CAST).map(|e|(e-CAST) as usize),&crate::ui::card_row::RowStyle::CAST,t.dt()));
        let hero=self.full && focus.is_some_and(|e|e<CAST);
        if !hero && self.preview_requested && crate::player::preview::occupies() {self.stop_preview(fx);}
        if hero && !self.preview_requested && crate::player::preview::enabled() {
            self.preview_dwell+=t.dt();
            if self.preview_dwell>=crate::player::preview::DWELL_S {
                if let Some((state,trailer))=self.state.as_ref().and_then(|s|s.trailer.as_ref().map(|t|(s,t))) {
                    self.preview_requested=true;
                    fx.push(Fx::App(AppFx::Content(ContentReq::PreviewStart {
                        sid:crate::plex::server_ids().next().unwrap_or(crate::plex::ServerId::UNSET),
                        rk:String::new(),part:trailer.key.clone(),vcodec:"h264".into(),acodec:"aac".into(),title:state.title.clone(),
                    })));
                }
            }
        }
        let picture=self.preview_requested && crate::player::preview::view().picture;
        self.preview_promoted &= picture;
        if self.preview_requested {crate::player::preview::set_promoted(self.preview_promoted);}
        let target=if self.preview_promoted {0.0} else {1.0};
        self.chrome+=(target-self.chrome).clamp(-t.dt()/0.25,t.dt()/0.25);
        for target in [self.trailer_ctl.step_scrub_hold(t.ms,crate::player::duration_ns()),self.trailer_ctl.step_tap_commit(t.ms)].into_iter().flatten() {
            fx.push(Fx::App(AppFx::Content(ContentReq::PreviewSeek(target))));
        }
        let moving=self.trailer_ctl.update(t.ms,t.dt(),self.preview_promoted,crate::player::preview::paused(),picture && hero && !self.preview_promoted);
        if self.full && (moving || cast_motion || self.chrome!=target) {fx.note(crate::ui::present::PresentEvent::Motion);}
    }
    fn preview_key<H:AppLike>(&mut self,key:Key,edge:Edge,sym:u32,wcode:u32,now:u32,fx:&mut Effects<'_,H>)->bool {
        use crate::ui::trailer::{trailer_key,TrailerKey};
        use crate::screens::registry::ContentReq;
        if !self.preview_promoted {return false;}
        let Some(action)=trailer_key(key,sym,wcode) else{return false;};
        if let TrailerKey::Scrub(fwd)=action {
            let target=match edge {
                Edge::Down=>{self.trailer_ctl.scrub_fresh(fwd,now,crate::player::duration_ns(),crate::player::playpos_ns());None},
                Edge::Repeat=>{self.trailer_ctl.scrub_repeat(now);None},
                Edge::Up=>self.trailer_ctl.scrub_release(now),
            };
            if let Some(target)=target {fx.push(Fx::App(AppFx::Content(ContentReq::PreviewSeek(target))));}
        } else if edge==Edge::Down {
            match action {
                TrailerKey::Collapse=>{self.preview_promoted=false; self.trailer_ctl.dismiss(); if crate::player::preview::paused(){fx.push(Fx::App(AppFx::Content(ContentReq::PreviewTransport(Some(true)))));}},
                TrailerKey::Toggle=>fx.push(Fx::App(AppFx::Content(ContentReq::PreviewTransport(None)))),
                TrailerKey::Play=>fx.push(Fx::App(AppFx::Content(ContentReq::PreviewTransport(Some(true))))),
                TrailerKey::Pause=>fx.push(Fx::App(AppFx::Content(ContentReq::PreviewTransport(Some(false))))),
                _=>{},
            }
        }
        if self.preview_promoted {self.trailer_ctl.reveal();}
        fx.invalidate(crate::ui::present::Provenance::Input); true
    }

}
impl LogicalState for CatalogDetailsScreen {
    fn write(&self, c: &mut Canon) {
        self.arg.write(c);
        c.u32(self.profile).bool(self.full).f32(self.preview_dwell).bool(self.preview_requested).bool(self.preview_promoted).f32(self.chrome);
        self.cast_row.write_motion(c);
        c.option(self.state.as_ref(), |c, s| {
            c.bool(s.busy).bool(s.failed).str(&s.summary).str(&s.title);
            c.seq(s.credits.len()); for p in &s.credits {c.str(&p.tag).str(&p.role).str(&p.thumb).str(&p.tag_key);}
            c.str(&s.kind).str(&s.facts).str(&s.genres).str(&s.cast);
            c.option(s.backdrop.as_ref(),|c,(sid,key)|{c.u32(sid.raw() as u32).str(key);});
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
        if let ScreenEvent::Input(input)=ev {
            if let InputKind::Key {key,edge,sym,wcode,..}=input.kind {
                if self.preview_key(key,edge,sym,wcode,input.at.ms,fx){return Handled::Yes;}
                if edge==Edge::Down && !self.full && self.arg.inline.is_some() {
                    let elem=cx.focus.current.filter(|k|k.entry==self.entry).map(|k|k.elem);
                    let dir=match (key,elem) {
                        (Key::Left,Some(INFO))=>Some(Dir::Left),
                        (Key::Right,Some(e)) if self.buttons().contains(&e)=>Some(Dir::Right),
                        _=>None,
                    };
                    if let Some(dir)=dir {
                        if let Some(parent)=self.arg.parent {
                            fx.push(Fx::Deliver(crate::ui::machine::MachineId::Instance(parent),
                                crate::ui::machine::Delivery::Screen(ScreenEvent::App(AppMsg::CatalogNavigate(dir)))));
                        }
                        self.stop_preview(fx); fx.push(Fx::Nav(NavOp::Dismiss(self.entry)));
                        return Handled::Yes;
                    }
                }
                if self.full && self.preview_requested && crate::player::preview::view().picture && key==Key::Up && edge==Edge::Down
                    && cx.focus.current.is_some_and(|k|k.entry==self.entry && k.elem<CAST) {
                    self.preview_promoted=true; self.trailer_ctl.reveal(); fx.invalidate(crate::ui::present::Provenance::Input); return Handled::Yes;
                }
            }
        }
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
                    self.full=crate::dev::flag("catalog-full-fixture");
                    state.kind="movie".into(); state.facts="2021 · 155 min · PG-13 · Warner Bros.".into();
                    state.genres="Science Fiction · Adventure · Drama".into();
                    state.cast="Timothée Chalamet · Rebecca Ferguson · Oscar Isaac · Zendaya · Jason Momoa".into();
                    state.credits=state.cast.split(" · ").map(|name|crate::catalog_action::Credit {tag:name.into(),role:"Cast".into(),thumb:"/library/metadata/1/thumb/1".into(),..Default::default()}).collect();
                    state.title="Dune".into(); state.year=2021; state.busy=false; state.watchlisted=Some(false);
                    state.summary="A young heir travels to the desert planet Arrakis, where rival houses compete for a resource that determines the future of the galaxy.".into();
                    state.local=Some((crate::plex::ServerId::from_raw(1),"42".into()));
                    state.poster=crate::plex::server_ids().next().map(|sid|(sid,"/library/metadata/1/thumb/1".into()));
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
                            elem: INFO,
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
            ScreenEvent::Tick(t)=>self.preview_tick(*t,cx,fx),
            ScreenEvent::Cover | ScreenEvent::Suspend | ScreenEvent::Unmount | ScreenEvent::WillLeave(_)=>self.stop_preview(fx),
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
                self.back(fx);
            }
            _ => return Handled::No,
        }
        Handled::Yes
    }
}
impl<H: AppLike> Focusable<H> for CatalogDetailsScreen {
    fn groups(&self, _: &Cx<'_, H>, out: &mut Vec<GroupSpec>) {
        out.push(GroupSpec {id:GroupId(1),kind:GroupKind::Free,seat:Seat::First,
            reachable:AxisMask::BOTH,edge:[EdgeRule::Stop;4],extent:self.panel(),
            len:self.buttons().len()+1+self.cast_len(),elem:ElemKind::Control});
    }
    fn group_of(&self,e:&u32,_:&Cx<'_,H>)->Option<GroupId> {
        (*e==INFO || self.buttons().contains(e) || (*e>=CAST && (*e-CAST) < self.cast_len() as u32)).then_some(GroupId(1))
    }
    fn neighbour(&self,from:FocusKey<u32>,dir:Dir,_:&Cx<'_,H>)->Step<u32> {
        let buttons=self.buttons();
        let next=if from.elem>=CAST {
            let i=(from.elem-CAST) as usize;
            match dir {Dir::Up=>Some(buttons[0]),Dir::Left if i>0=>Some(from.elem-1),Dir::Right if i+1<self.cast_len()=>Some(from.elem+1),_=>None}
        } else if from.elem==INFO {
            matches!(dir,Dir::Right|Dir::Down).then_some(buttons[0])
        } else if let Some(i)=buttons.iter().position(|e|*e==from.elem) {
            if (!self.full && dir==Dir::Left) || (self.full && dir==Dir::Up) {Some(INFO)}
            else { match dir {
                Dir::Up if !self.full => i.checked_sub(1).map(|j|buttons[j]),
                Dir::Down if !self.full => buttons.get(i+1).copied(),
                Dir::Left if self.full => i.checked_sub(1).map(|j|buttons[j]),
                Dir::Right if self.full => buttons.get(i+1).copied(),
                Dir::Down if self.full && self.cast_len()>0 => Some(CAST),
                _=>None,
            }}
        } else {None};
        next.map_or(Step::Edge,|elem|Step::Move(FocusKey{entry:self.entry,elem}))
    }
    fn place(&self,e:&u32,cx:&Cx<'_,H>,_:At)->Option<Placed> {
        if self.group_of(e,cx).is_none() {return None;}
        let rect=if *e==INFO {self.info_rect()} else if *e>=CAST {self.cast_rect((*e-CAST) as usize)} else {self.button_rect(*e)};
        Some(Placed{rect,rest_rect:rect,clip:Rect::FULL,index:if *e==INFO {None}
            else {self.buttons().iter().position(|i|i==e).map(|i|i as u32)}})
    }
    fn reconcile(&self,want:FocusKey<u32>,_:&Cx<'_,H>)->FocusKey<u32> {
        if want.elem==INFO || self.buttons().contains(&want.elem) || (want.elem>=CAST && (want.elem-CAST)<self.cast_len() as u32) {want}
        else {FocusKey{entry:self.entry,elem:INFO}}
    }
    fn seat(&self,_:GroupId,_:Placed,_:&Cx<'_,H>)->FocusKey<u32> {FocusKey{entry:self.entry,elem:INFO}}
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
    fn scrim(&self) -> Scrim { Scrim::NONE }
    fn draw(&mut self, f: &mut DrawFrame<'_, '_, H>) {
        use crate::ui::View;
        let r=self.panel();
        let preview=if self.full && self.preview_requested {crate::player::preview::view()} else {crate::player::preview::View::STILL};
        if preview.picture && !f.painter.is_recording() {crate::gfx::frame_clear_through();}
        let base=f.painter.alpha(f.page_alpha);
        let p=base.alpha(if self.full {self.chrome} else {1.0});
        if self.full {
            if !preview.picture {base.rect(Rect::FULL,0.0,theme::CINEMA_GROUND,theme::CINEMA_GROUND,0.0);}
            if let Some((sid,key))=self.state.as_ref().and_then(|s|s.backdrop.as_ref()) {
                crate::ui::widgets::card(base.alpha(preview.art),Rect::FULL,crate::ui::widgets::Art::Thumb{sid:*sid,key,res:(1280,720)},0.0,false,1.0,0.0);
            }
            let shade=theme::scrim_black(if preview.picture {0.58}else{0.84});
            p.rect(Rect::FULL,0.0,shade,shade,0.0);
        } else if !self.arg.host_glass {crate::ui::widgets::panel_ground(p,r,theme::ALERT_PANEL_RAD,f.underlay);}
        let poster=if self.full {Rect::new(96.0,160.0,240.0,360.0)}
            else {Rect::new(r.x+COMPACT_PAD,r.y+COMPACT_PAD,226.0,339.0)};
        if let Some((sid,key))=self.state.as_ref().and_then(|s|s.poster.as_ref()).or(self.arg.poster.as_ref()) {
            crate::ui::widgets::card(p,poster,crate::ui::widgets::Art::Thumb{sid:*sid,key,res:(300,450)},theme::CARD_RING_RAD,false,1.0,0.0);
        }
        else { crate::ui::widgets::card(p,poster,crate::ui::widgets::Art::Poster(None),theme::CARD_RING_RAD,false,1.0,0.0); }
        let x=if self.full {384.0} else {poster.x+poster.w+24.0};
        let w=if self.full {1440.0} else {r.x+r.w-120.0-x};
        let top=if self.full {160.0} else {r.y+32.0};
        let text=|s:&str,y,size,ink,lines| {
            TextView::new(s,size,ink).with_measure(f.measure).max_lines(lines)
                .draw(p,Rect::new(x,y,w,0.0));
        };
        let state=self.state.as_ref();
        let title=state.map(|s|s.title.as_str()).filter(|s|!s.is_empty()).unwrap_or(&self.arg.title);
        text(title,top,if self.full {theme::size::DISPLAY} else {theme::size::HEADLINE},theme::TEXT_HEADING,if self.full {2}else{1});
        let year=self.arg.year.to_string();
        let facts=state.map(|s|s.facts.as_str()).filter(|s|!s.is_empty()).unwrap_or(if self.arg.year>0{&year}else{""});
        let title_h=TextView::new(title,theme::size::DISPLAY,theme::TEXT_HEADING).with_measure(f.measure).max_lines(2).measure_h(w);
        let facts_y=top+if self.full {title_h+20.0} else {56.0};
        text(facts,facts_y,theme::size::CAPTION,theme::TEXT_SECONDARY,1);
        if let Some(state)=state {
            if self.full {
                crate::ui::fmt::draw_ratings(p,&state.ratings,x,facts_y+50.0,x+w,f.measure);
            } else {
                crate::ui::fmt::draw_compact_ratings(p,&state.ratings,x,r.y+246.0,x+w,f.measure);
            }
            text(&state.summary,if self.full{facts_y+100.0}else{top+104.0},if self.full{theme::size::BODY}else{theme::size::CAPTION},theme::TEXT_READING,if self.full{4}else{3});
            if self.full {
                if !state.genres.is_empty(){text(&state.genres,540.0,theme::size::CAPTION,theme::TEXT_SECONDARY,1);}

            }
        }
        let current=f.cx.focus.current.filter(|k|k.entry==self.entry).map(|k|k.elem);
        if current==Some(INFO) {
            p.rring(if self.full {poster} else {r},if self.full {theme::CARD_RING_RAD}else{theme::ALERT_PANEL_RAD},2.0,theme::TEXT_SECONDARY);
        }
        let watch=match state {Some(s) if s.busy=>c"Loading…",Some(s) if s.failed=>c"Retry Watchlist",Some(s) if s.watchlisted==Some(true)=>c"Remove Watchlist",_=>c"Watchlist"};
        if !self.full {
            let hint=match current {Some(WATCHLIST)=>watch.to_str().unwrap_or_default(),Some(LIBRARY)=>"Open in Library",Some(TRAILER)=>"Preview trailer",_=>"Details  ›"};
            text(hint,r.y+r.h-48.0,theme::size::CAPTION,theme::TEXT_SECONDARY,1);
        }
        for elem in self.buttons().into_iter().chain(std::iter::once(INFO)) {
            let rect=if elem==INFO {self.info_rect()} else {self.button_rect(elem)};
            if elem!=INFO {
                let icon=match elem {LIBRARY=>crate::ui::icons::Icon::Show,TRAILER=>crate::ui::icons::Icon::Trailer,_=>watchlist_icon(state.and_then(|s|s.watchlisted))};
                crate::ui::widgets::CircleButton::new(c"".as_ptr()).icon(icon).frame(rect)
                    .focused(current==Some(elem)).draw(&crate::ui::Env::inert(),p);
                if self.full && current==Some(elem) {
                    let label=match elem {LIBRARY=>c"Open in Library",TRAILER=>c"Trailer",_ if state.is_some_and(|s|s.watchlisted==Some(true))=>c"Watchlisted",_=>watch};
                    TextView::new(label.to_str().unwrap_or_default(),theme::size::CAPTION,theme::TEXT_SECONDARY).with_measure(f.measure).h(crate::ui::label::HAlign::Center).max_lines(1).draw(p,Rect::new(rect.x+rect.w/2.0-90.0,rect.y+rect.h+12.0,180.0,0.0));
                }
            }
            if !self.preview_promoted {f.stop(f.painter,crate::ui::screen::Stop{key:FocusKey{entry:self.entry,elem},rect,rest_rect:rect,
                clip:Rect::FULL,hover:crate::ui::screen::Hover::Focus,activate:crate::ui::screen::Activate::Immediate});}
        }
        if self.full {
            if self.cast_len()>0 {
                TextView::new("Cast",theme::size::HEADLINE,theme::TEXT_HEADING).with_measure(f.measure).draw(p,Rect::new(96.0,700.0,1000.0,0.0));
                let sid=crate::plex::server_ids().next().unwrap_or(crate::plex::ServerId::UNSET);
                if let Some(state)=state {for (i,person) in state.credits.iter().enumerate() {
                    let rect=self.cast_rect(i); let clip=Rect::new(80.0,734.0,1760.0,290.0);
                    if rect.x+rect.w<clip.x || rect.x>clip.x+clip.w {continue;}
                    let cp=p.clipped(clip);
                    crate::ui::widgets::card(cp,rect,crate::ui::widgets::Art::Person{sid,key:&person.thumb,res:(240,240)},rect.w/2.0,current==Some(CAST+i as u32),self.cast_row.scale(i),0.0);
                    TextView::new(&person.tag,theme::size::CAPTION,theme::TEXT_PRIMARY).with_measure(f.measure).max_lines(1).draw(cp,Rect::new(rect.x,rect.y+rect.h+14.0,rect.w,0.0));
                    TextView::new(&person.role,theme::size::MICRO,theme::TEXT_SECONDARY).with_measure(f.measure).max_lines(1).draw(cp,Rect::new(rect.x,rect.y+rect.h+46.0,rect.w,0.0));
                    if !self.preview_promoted {f.stop(f.painter,crate::ui::screen::Stop {key:FocusKey{entry:self.entry,elem:CAST+i as u32},rect,rest_rect:rect,clip,hover:crate::ui::screen::Hover::Focus,activate:crate::ui::screen::Activate::Immediate});}
                }}
            }
            self.trailer_ctl.draw(base,title,state.and_then(|s|s.trailer.as_ref()).map_or("",|t|t.title.as_str()),crate::player::preview::paused(),crate::player::playpos_ns(),crate::player::duration_ns(),f.measure);
            self.trailer_ctl.draw_hint(p,80.0,f.measure);
            let hint=crate::ui::widgets::KeyHint::new(c"Press",c"BACK",c"to return");
            hint.draw(p,1824.0-hint.width(f.measure),1030.0,f.measure);
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

fn watchlist_icon(selected:Option<bool>)->crate::ui::icons::Icon {
    if selected==Some(true) {crate::ui::icons::Icon::WatchlistFill} else {crate::ui::icons::Icon::Watchlist}
}
