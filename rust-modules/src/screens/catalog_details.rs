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
pub(crate) const SHAPE: &str = "CatalogDetails{watchlist_v1:profile_status_summary,catalog_id:str,title:str,person:str,department:str,role:str,year:i32}";
pub(crate) struct CatalogDetailsScreen {
    entry: EntryId,
    arg: CatalogDetailsArg,
    state: Option<crate::catalog_action::Snapshot>,
    profile: u32,
}
const WATCHLIST: u32 = 1;
fn button_rect() -> Rect {
    Rect::new(456.0, 736.0, 1008.0, 64.0)
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
}
impl LogicalState for CatalogDetailsScreen {
    fn write(&self, c: &mut Canon) {
        self.arg.write(c);
        c.u32(self.profile);
        c.option(self.state.as_ref(), |c, s| {
            c.bool(s.busy).bool(s.failed).str(&s.summary).str(&s.title);
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
    fn step(&mut self, ev: &Self::Ev, _cx: &Cx<'_, H>, fx: &mut Effects<'_, H>) -> Handled {
        match ev {
            ScreenEvent::Mount => {
                self.profile = crate::plex::session::current_gen();
                let request = crate::catalog_action::Request {
                    id: self.arg.catalog_id.clone(),
                    profile: self.profile,
                    desired: None,
                };
                self.state = Some(crate::catalog_action::Snapshot::pending(&request));
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
            ScreenEvent::Activate(WATCHLIST) | ScreenEvent::PressCommit(_) => {
                if let Some(state) = self.state.as_ref().filter(|s| !s.busy) {
                    let desired = state.watchlisted.map(|v| !v);
                    let request = crate::catalog_action::Request {
                        id: self.arg.catalog_id.clone(),
                        profile: self.profile,
                        desired,
                    };
                    self.state = Some(crate::catalog_action::Snapshot::pending(&request));
                    fx.push(Fx::App(AppFx::Catalog(request)));
                    fx.invalidate(crate::ui::present::Provenance::Input);
                }
            }
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
            extent: button_rect(),
            len: 1,
            elem: ElemKind::Control,
        });
    }
    fn group_of(&self, e: &u32, _: &Cx<'_, H>) -> Option<GroupId> {
        (*e == WATCHLIST).then_some(GroupId(1))
    }
    fn neighbour(&self, _: FocusKey<u32>, _: Dir, _: &Cx<'_, H>) -> Step<u32> {
        Step::Edge
    }
    fn place(&self, e: &u32, _: &Cx<'_, H>, _: At) -> Option<Placed> {
        (*e == WATCHLIST).then_some(Placed {
            rect: button_rect(),
            rest_rect: button_rect(),
            clip: Rect::new(0.0, 0.0, 1920.0, 1080.0),
            index: Some(0),
        })
    }
    fn reconcile(&self, want: FocusKey<u32>, _: &Cx<'_, H>) -> FocusKey<u32> {
        want
    }
    fn seat(&self, _: GroupId, _: Placed, _: &Cx<'_, H>) -> FocusKey<u32> {
        FocusKey {
            entry: self.entry,
            elem: WATCHLIST,
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
        let r = Rect::new(
            (crate::ui::consts::SCR_W - 1120.0) * 0.5,
            (crate::ui::consts::SCR_H - 600.0) * 0.5,
            1120.0,
            600.0,
        );
        let p = f.painter.alpha(f.page_alpha);
        crate::ui::widgets::panel_ground(p, r, theme::ALERT_PANEL_RAD, f.underlay);
        let x = r.x + theme::alert::PAD;
        let w = r.w - 2.0 * theme::alert::PAD;
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
        let year = if self.arg.year > 0 {
            self.arg.year.to_string()
        } else {
            "Year not supplied".into()
        };
        text(&year, 205.0, theme::size::BODY, theme::TEXT_READING, 1);
        let credit = if self.arg.role.is_empty() {
            format!("{} · {}", self.arg.person, self.arg.department)
        } else {
            format!(
                "{} · {} · {}",
                self.arg.person, self.arg.department, self.arg.role
            )
        };
        text(&credit, 252.0, theme::size::BODY, theme::TEXT_READING, 1);
        let summary = self
            .state
            .as_ref()
            .map(|s| s.summary.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("This title is not available in your library.");
        text(summary, 310.0, theme::size::BODY, theme::TEXT_READING, 4);
        let label = match self.state.as_ref() {
            Some(s) if s.busy => c"Loading…",
            Some(s) if s.failed => c"Retry Watchlist status",
            Some(s) if s.watchlisted == Some(true) => c"Remove from Watchlist",
            _ => c"Add to Watchlist",
        };
        use crate::ui::View;
        crate::ui::widgets::Button::new(label.as_ptr(), theme::size::BODY, button_rect())
            .focused(true)
            .draw(&crate::ui::Env::inert(), p);
        f.stop(
            f.painter,
            crate::ui::screen::Stop {
                key: FocusKey {
                    entry: self.entry,
                    elem: WATCHLIST,
                },
                rect: button_rect(),
                rest_rect: button_rect(),
                clip: Rect::new(0.0, 0.0, 1920.0, 1080.0),
                hover: crate::ui::screen::Hover::Focus,
                activate: crate::ui::screen::Activate::Immediate,
            },
        );

        let hint = crate::ui::widgets::KeyHint::new(c"Press", c"BACK", c"to return");
        hint.draw(p, x + w - hint.width(f.measure), r.y + 572.0, f.measure);
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
