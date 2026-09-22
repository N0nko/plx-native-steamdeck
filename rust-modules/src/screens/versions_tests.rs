//! No network or global metadata fixture: exercise the same owned surface as production.
use super::*;
use crate::metadata::{Detail, MetadataView};
use crate::screens::registry::{AppArg, MetadataLike};
use crate::stores::metadata::MetadataStore;
use crate::ui::machine::{FocusRead, Host, InputOwner, PressRead, Stamped, Tick};

struct Fixture;
impl Host for Fixture {
    type Arg = AppArg;
    type Fx = AppFx;
    type Msg = AppMsg;
    type Elem = u32;
    type Views<'a> = MetadataView<'a>;
    type Init = AppArg;
    type Memory = PageMemory;
}
impl MetadataLike for Fixture {
    fn metadata<'a>(cx: &Cx<'a, Self>) -> MetadataView<'a> {
        cx.views
    }
}
fn sid() -> ServerId {
    ServerId::from_raw(1)
}
fn version(id: i64, index: i64) -> MediaVersion {
    MediaVersion {
        source: crate::media_version::Candidate {
            position: crate::media_version::Position {
                media_index: index,
                part_index: 0,
            },
            part_id: id,
            part_key: format!("/library/parts/{id}/stamp/file.mkv"),
        },
        part_count: 1,
        vcodec: "hevc".into(),
        acodec: "ac3".into(),
        video_resolution: "4k".into(),
        width: 3840,
        height: 2160,
        bitrate: 45_000,
        size: 2_147_483_648,
        file: "/movies/example.mkv".into(),
        container: "mkv".into(),
        hdr: true,
        ..Default::default()
    }
}
fn owner(versions: Vec<MediaVersion>) -> MetadataStore {
    let mut owner = MetadataStore::default();
    let part = versions
        .first()
        .map(|v| v.source.part_key.clone())
        .unwrap_or_default();
    crate::metadata::set_current_for_test(
        owner.state_mut(),
        Some(Detail {
            sid: sid(),
            rk: "42".into(),
            part,
            versions,
            ..Default::default()
        }),
    );
    owner
}
fn panel(owner: &MetadataStore) -> VersionsScreen {
    VersionsScreen::new(
        EntryId(7),
        VersionsArg {
            host: InstanceId(9),
            sid: sid(),
            rk: "42".into(),
            anchor: [160.0f32, 700.0, 150.0, 44.0].map(f32::to_bits),
        },
        owner.view(),
    )
}
fn run(
    p: &mut VersionsScreen,
    owner: &MetadataStore,
    event: ScreenEvent<Fixture>,
) -> Vec<Stamped<Fixture>> {
    let cx = Cx::<Fixture> {
        views: owner.view(),
        tick: Tick::default(),
        measure: &crate::ui::fixture::FixtureMeasure,
        focus: FocusRead {
            current: Some(FocusKey {
                entry: p.entry,
                elem: p.table.sel.max(0) as u32,
            }),
            ..Default::default()
        },
        press: PressRead::default(),
        owner: InputOwner::Entry(p.entry),
    };
    let mut out = Vec::new();
    let mut present = crate::ui::present::Present::default();
    let mut effects = Effects::new(&mut out, MachineId::Instance(InstanceId(7)), &mut present);
    Machine::step(p, &event, &cx, &mut effects);
    out
}
fn capabilities(p: &VersionsScreen) -> CompanionSnapshot {
    CompanionSnapshot {
        key: p.target().unwrap(),
        label: "Streaming".into(),
        configured: true,
        busy: false,
        can_repair: true,
        can_prepare: true,
    }
}

#[test]
fn labels_distinguish_actual_files_without_exposing_server_directories() {
    let v = version(11, 0);
    let row = version_row(sid(), "42", &v.source.part_key, &v);
    assert!(row.label.contains("HEVC") && row.label.contains("HDR"));
    assert!(row.detail.contains("45.0 Mbps") && row.detail.contains("2.0 GiB"));
    assert!(row.detail.contains("example.mkv") && !row.detail.contains("/movies/"));
    assert!(row.checked && row.playable);
}
#[test]
fn unavailable_multipart_and_out_of_range_choices_never_play() {
    let mut v = version(11, 0);
    v.part_count = 2;
    let rows = vec![version_row(sid(), "42", "", &v)];
    assert_eq!(action_at(&rows, 0), Action::None);
    assert_eq!(action_at(&rows, -1), Action::None);
    assert_eq!(action_at(&rows, 5), Action::None);
}
#[test]
fn single_version_still_exposes_the_companion_section() {
    let _guard = crate::testlock::serial();
    let owner = owner(vec![version(11, 0)]);
    let p = panel(&owner);
    assert_eq!(p.rows.len(), 1);
    assert_eq!(p.row_count(), 5);
    assert_eq!(p.table.sections.len(), 3);
    assert!(p.companion.is_none());
}
#[test]
fn activating_a_version_reports_the_exact_host_server_item_and_part_once() {
    let _guard = crate::testlock::serial();
    let owner = owner(vec![version(11, 0), version(22, 1)]);
    let mut p = panel(&owner);
    run(&mut p, &owner, ScreenEvent::Mount);
    let events = run(&mut p, &owner, ScreenEvent::Activate(1));
    assert!(events.iter().any(|event| matches!(&event.fx,
        Fx::Deliver(MachineId::Instance(InstanceId(9)),
            Delivery::Screen(ScreenEvent::App(AppMsg::VersionSelected { sid: source, rk, part })))
            if *source == sid() && rk == "42" && part == "/library/parts/22/stamp/file.mkv")));
    assert!(events
        .iter()
        .any(|event| matches!(&event.fx, Fx::Nav(NavOp::Dismiss(EntryId(7))))));
    assert!(
        run(&mut p, &owner, ScreenEvent::Activate(1)).is_empty(),
        "dismissal retires further commits"
    );
}
#[test]
fn metadata_reordering_retains_the_highlighted_part_not_its_index() {
    let _guard = crate::testlock::serial();
    let mut owner = owner(vec![version(11, 0), version(22, 1)]);
    let mut p = panel(&owner);
    p.focus_row(1);
    crate::metadata::set_current_for_test(
        owner.state_mut(),
        Some(Detail {
            sid: sid(),
            rk: "42".into(),
            part: version(11, 1).source.part_key,
            versions: vec![version(22, 0), version(11, 1)],
            ..Default::default()
        }),
    );
    assert!(p.refresh(owner.view()));
    assert_eq!(p.table.sel, 0);
    assert_eq!(p.target().unwrap().part, "22");
}
#[test]
fn stale_companion_replies_never_authorize_another_file_or_closed_screen() {
    let _guard = crate::testlock::serial();
    let owner = owner(vec![version(11, 0), version(22, 1)]);
    let mut p = panel(&owner);
    run(&mut p, &owner, ScreenEvent::Mount);
    let old = capabilities(&p);
    p.focus_row(1);
    run(&mut p, &owner, ScreenEvent::App(AppMsg::Companion(old)));
    assert!(p.companion.is_none());
    let current = capabilities(&p);
    run(&mut p, &owner, ScreenEvent::Suspend);
    run(&mut p, &owner, ScreenEvent::App(AppMsg::Companion(current)));
    assert!(p.companion.is_none());
}
#[test]
fn prepare_requires_two_presses_and_sends_only_the_highlighted_part() {
    let _guard = crate::testlock::serial();
    let owner = owner(vec![version(11, 0), version(22, 1)]);
    let mut p = panel(&owner);
    run(&mut p, &owner, ScreenEvent::Mount);
    p.focus_row(1);
    let ready = capabilities(&p);
    run(&mut p, &owner, ScreenEvent::App(AppMsg::Companion(ready)));
    let prepare_row = p.companion_offset() as u32 + 1;
    let first = run(&mut p, &owner, ScreenEvent::Activate(prepare_row));
    assert!(!first
        .iter()
        .any(|event| matches!(&event.fx, Fx::App(AppFx::Companion(_)))));
    assert_eq!(p.confirm, Some(CompanionAction::Prepare));
    let second = run(&mut p, &owner, ScreenEvent::Activate(prepare_row));
    assert_eq!(
        second
            .iter()
            .filter(|event| matches!(&event.fx,
        Fx::App(AppFx::Companion(CompanionRequest { key, action: CompanionAction::Prepare }))
            if key.part == "22" && key.rk == "42" && key.sid == sid()))
            .count(),
        1
    );
    assert!(p.companion.as_ref().unwrap().busy);
    assert!(!run(&mut p, &owner, ScreenEvent::Activate(prepare_row))
        .iter()
        .any(|event| matches!(&event.fx, Fx::App(AppFx::Companion(_)))));
}
#[test]
fn unknown_capabilities_cannot_send_mutating_requests() {
    let _guard = crate::testlock::serial();
    let owner = owner(vec![version(11, 0)]);
    let mut p = panel(&owner);
    run(&mut p, &owner, ScreenEvent::Mount);
    let first = p.companion_offset() as u32;
    for action_row in [first, first + 1, first, first + 1] {
        assert!(run(&mut p, &owner, ScreenEvent::Activate(action_row)).is_empty());
    }
    assert!(p.confirm.is_none());
}
#[test]
fn changing_the_highlight_clears_any_old_mutation_confirmation() {
    let _guard = crate::testlock::serial();
    let owner = owner(vec![version(11, 0), version(22, 1)]);
    let mut p = panel(&owner);
    p.confirm = Some(CompanionAction::Repair);
    p.companion = Some(capabilities(&p));
    p.focus_row(1);
    assert!(p.confirm.is_none() && p.companion.is_none());
}
#[test]
fn panel_bounds_remain_on_screen_even_for_long_version_lists() {
    for anchor in [
        Rect::new(0.0, 0.0, 120.0, 44.0),
        Rect::new(1900.0, 1040.0, 120.0, 44.0),
    ] {
        for height in [0.0, 500.0, 5000.0] {
            let r = panel_at(anchor, height);
            assert!(r.x >= EDGE_X && r.x + r.w <= SCR_W - EDGE_X);
            assert!(r.y >= EDGE && r.y + r.h <= SCR_H - EDGE);
        }
    }
}

#[test]
fn preplay_audio_choice_is_a_store_request_and_does_not_start_the_movie() {
    let _guard = crate::testlock::serial();
    let mut v = version(11, 0);
    v.audio = vec![crate::metadata::Stream {
        id: 110,
        codec: "truehd".into(),
        lang: "English".into(),
        ..Default::default()
    }];
    let owner = owner(vec![v]);
    let mut p = panel(&owner);
    run(&mut p, &owner, ScreenEvent::Mount);
    let events = run(&mut p, &owner, ScreenEvent::Activate(2)); // version, Auto audio, chosen audio
    assert!(events.iter().any(|event| matches!(&event.fx,
        Fx::App(AppFx::Store(crate::stores::StoreId::Metadata,
            crate::stores::StoreCmd::Metadata(crate::stores::metadata::MetadataCmd::SetPreplay { sid: s, rk, part, choice })))
            if *s == sid() && rk == "42" && part == "/library/parts/11/stamp/file.mkv" && choice.audio == Some(110))));
    assert!(!events
        .iter()
        .any(|event| matches!(&event.fx, Fx::Nav(NavOp::Dismiss(_)))));
    assert!(p.active);
}
#[test]
fn subtitle_off_is_not_the_automatic_entry() {
    let _guard = crate::testlock::serial();
    let owner = owner(vec![version(11, 0)]);
    let mut p = panel(&owner);
    run(&mut p, &owner, ScreenEvent::Mount);
    run(&mut p, &owner, ScreenEvent::Activate(2)); // version, Auto subtitles, Off
    assert_eq!(p.choice.subtitle, Some(0));
    run(&mut p, &owner, ScreenEvent::Activate(1));
    assert_eq!(p.choice.subtitle, None);
}
