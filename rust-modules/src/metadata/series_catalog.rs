//! Optional, off-frame companion catalog. Local details are usable before this arrives.
use super::{Detail, Episode, MetadataAdapter, MetadataState, Season};
use crate::plex::ServerId;
use serde::{Deserialize, Serialize};
use std::sync::{mpsc, Arc};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Row {
    pub season: i64,
    pub number: i64,
    pub title: String,
    pub summary: String,
    pub aired: String,
    pub state: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Catalog { pub episodes: Vec<Row> }
#[derive(Clone, Debug)]
pub(crate) enum Action { Read { now: u32 }, Request { season: i64, episode: Option<i64> } }

const READ_FAILURE: &str = "Season list unavailable · retrying shortly";
pub(crate) fn read_due(d: &Detail, now: u32) -> bool {
    d.is_show && !d.series_attempted && (d.series_retry_at==0 || now.wrapping_sub(d.series_retry_at)<0x8000_0000)
}
fn land_read(d: &mut Detail, answer: Option<serde_json::Value>, now: u32) {
    if let Some(catalog)=answer.and_then(|v|serde_json::from_value::<Catalog>(v).ok()).filter(valid_catalog) {
        d.series_catalog=Some(catalog); d.series_attempted=true; d.series_retry_at=0;
        if d.series_notice==READ_FAILURE { d.series_notice.clear(); }
        merge(d);
    } else {
        d.series_attempted=false; d.series_retry_at=now.wrapping_add(30_000);
        d.series_notice=READ_FAILURE.into();
    }
}
fn request_notice(answer: Option<&serde_json::Value>) -> &'static str {
    match answer.and_then(|v|v["request_state"].as_str()) {
        Some("Requested") => "Missing episode search requested",
        Some("Queued") => "Episodes already queued for download",
        Some("Already requested") => "Search already requested · please allow time for results",
        Some("Refresh library") => "Files are available · waiting for Plex library refresh",
        Some("Not aired") => "These episodes have not aired yet",
        Some("Nothing to request") => "No aired missing episodes to request",
        _ => "Request not confirmed — try again later",
    }
}

struct Pending {
    sid: ServerId, rk: String, generation: u32, profile: u32, action: Action,
    rx: mpsc::Receiver<Option<serde_json::Value>>,
}
#[derive(Default)]
pub(super) struct Controller { pending: Option<Pending> }

fn perform(sid: ServerId, rk: &str, action: &Action) -> Option<serde_json::Value> {
    if !crate::media_companion::valid_id(rk) { return None; }
    let client = crate::plex::client_for(sid)?;
    let p = crate::media_companion::pairing::read_pairing().filter(|p| p.machine_id == client.machine_id())?;
    let (verb, method, extra) = match action {
        Action::Read {..} => ("series", "GET", String::new()),
        Action::Request { season, episode } if (0..=999).contains(season) && episode.is_none_or(|n| (1..=10000).contains(&n)) =>
            ("request-episodes", "POST", format!("&season={season}{}", episode.map(|n|format!("&episode={n}")).unwrap_or_default())),
        _ => return None,
    };
    let response = crate::net::request_tls(
        &format!("https://{}:{}/v1/{verb}?rating_key={rk}&machine_id={}{}",p.host,p.port,p.machine_id,extra),
        &["Accept: application/json".into(),format!("Authorization: Bearer {}",p.token)], method,None,
        crate::net::Timeouts { connect_s:3,total_s:15,..crate::net::API },false,Some(2_000_000),crate::net::Tls::Pinned(&p.tls_pin))?;
    if response.status != 200 { return None; }
    let value: serde_json::Value = serde_json::from_slice(&response.body).ok()?;
    (value["rating_key"] == rk && value["machine_id"] == p.machine_id).then_some(value)
}

pub(super) fn request(state: &mut MetadataState, adapter: &Arc<MetadataAdapter>, sid: ServerId, rk: &str, action: Action) -> bool {
    let Some(d) = state.current.as_mut().filter(|d| d.is_show && d.sid==sid && d.rk==rk) else { return false; };
    let mut ctrl=adapter.series.lock().unwrap_or_else(|e|e.into_inner());
    if ctrl.pending.is_some() { return false; }
    if let Action::Read {now}=action { if !read_due(d,now) { return false; } }
    if let Action::Request { season, episode } = action {
        if !d.series_catalog.as_ref().is_some_and(|c| c.episodes.iter().any(|r|
            r.season==season && episode.is_none_or(|n|n==r.number) && r.state=="Missing")) { return false; }
    }
    let generation=super::detail_generation(adapter);
    let profile=crate::plex::session::current_gen();
    let (tx,rx)=mpsc::sync_channel(1);
    let key=rk.to_owned(); let work=action.clone();
    let spawned=crate::task::spawn("series-catalog",move || {
        let value=if profile==crate::plex::session::current_gen() { perform(sid,&key,&work) } else { None };
        let _=tx.send(value); crate::ui::idle::invalidate();
    }).is_some();
    if !spawned { return false; }
    d.series_attempted=true;
    if matches!(action,Action::Request {..}) { d.series_notice="Requesting missing episodes…".into(); }
    ctrl.pending=Some(Pending { sid,rk:rk.into(),generation,profile,action,rx });
    true
}

pub(crate) fn pump(state: &mut MetadataState, adapter: &MetadataAdapter, gate: &crate::ui::landgate::Gate) -> bool {
    if super::season_loading(adapter) { return false; }
    let result=crate::stores::take_landing(gate,crate::stores::StoreId::Metadata,|| {
        let mut ctrl=adapter.series.lock().unwrap_or_else(|e|e.into_inner());
        let answer=match ctrl.pending.as_ref()?.rx.try_recv() {
            Ok(value)=>value, Err(mpsc::TryRecvError::Empty)=>return None, Err(_)=>None,
        };
        Some((ctrl.pending.take()?,answer))
    });
    let Some((pending,answer))=result else { return false; };
    if pending.profile!=crate::plex::session::current_gen() || pending.generation!=super::detail_generation(adapter) { return false; }
    let Some(d)=state.current.as_mut().filter(|d|d.sid==pending.sid && d.rk==pending.rk) else { return false; };
    match pending.action {
        Action::Read {now} => land_read(d,answer,now),
        Action::Request { season,episode } => {
            let confirmed=answer.as_ref().is_some_and(|v| v["season"].as_i64()==Some(season)
                && v["episode"].as_i64()==episode && v["request_state"]=="Requested");
            d.series_notice=request_notice(answer.as_ref()).into();
            // Refresh ownership/queue labels even when an older card produced no new request.
            d.series_attempted=false; d.series_retry_at=0;
            if confirmed { if let Some(c)=&mut d.series_catalog { for r in &mut c.episodes {
                if r.season==season && episode.is_none_or(|n|n==r.number) && r.state=="Missing"
                    && answer.as_ref().and_then(|v|v["episode_numbers"].as_array()).is_some_and(|ids|ids.iter().any(|id|id.as_i64()==Some(r.number))) { r.state="Requested".into(); }
            } } merge_episodes(d); }
        }
    }
    true
}

fn valid_catalog(c: &Catalog) -> bool {
    let mut keys=std::collections::BTreeSet::new();
    c.episodes.len()<=5000 && c.episodes.iter().all(|r| (0..=999).contains(&r.season) && (1..=10000).contains(&r.number)
        && r.title.len()<=1024 && r.summary.len()<=16384 && keys.insert((r.season,r.number))
        && matches!(r.state.as_str(),"Library"|"Refresh library"|"Missing"|"Not aired"|"Requested"|"Queued"))
}
pub(crate) fn first_local_episode(d: &Detail) -> Option<&Episode> {
    d.episodes.iter().find(|episode|!missing(episode))
}
pub(crate) fn missing(ep: &Episode) -> bool { ep.rk.starts_with("missing:") }
pub(crate) fn synthetic_season(rk: &str) -> bool { rk.starts_with("missing-season:") }
pub(crate) fn requestable(d: &Detail, season: i64, episode: Option<i64>) -> bool {
    d.series_catalog.as_ref().is_some_and(|c|c.episodes.iter().any(|r|r.season==season
        && episode.is_none_or(|n|n==r.number) && r.state=="Missing"))
}
pub(crate) fn status<'a>(d: &'a Detail, ep: &Episode) -> &'a str {
    d.series_catalog.as_ref().and_then(|c|c.episodes.iter().find(|r|r.season==ep.season&&r.number==ep.index))
        .map(|r|if r.state=="Missing" {"Request episode"} else {r.state.as_str()}).unwrap_or("Unavailable")
}
fn merge(d: &mut Detail) {
    let selected=d.seasons.get(d.cur_season).map(|s|s.index);
    if let Some(c)=&d.series_catalog { for row in &c.episodes {
        if !d.seasons.iter().any(|s|s.index==row.season) {
            d.seasons.push(Season { rk:format!("missing-season:{}",row.season),index:row.season,
                title:if row.season==0 {"Specials".into()}else{format!("Season {}",row.season)},leaf_count:0,viewed_leaf_count:0 });
        }
    } }
    d.seasons.sort_by_key(|s|s.index);
    if let Some(i)=selected.and_then(|n|d.seasons.iter().position(|s|s.index==n)) { d.cur_season=i; }
    merge_episodes(d);
}
pub(super) fn merge_episodes(d: &mut Detail) {
    let Some(season)=d.seasons.get(d.cur_season).map(|s|s.index) else {return;};
    d.episodes.retain(|e|!missing(e));
    if let Some(c)=&d.series_catalog { for row in c.episodes.iter().filter(|r|r.season==season) {
        if !d.episodes.iter().any(|e|e.season==season && e.index==row.number) {
            d.episodes.push(Episode { rk:format!("missing:{season}:{}",row.number),season,index:row.number,
                title:row.title.clone(),summary:row.summary.clone(),aired:row.aired.clone(),thumb:d.art.clone(),..Default::default() });
        }
    } }
    d.episodes.sort_by_key(|e|e.index);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_catalog_retries_with_backoff_and_preserves_local_details() {
        let mut d=Detail {is_show:true,series_attempted:true,episodes:vec![Episode {rk:"42".into(),..Default::default()}],..Default::default()};
        land_read(&mut d,None,1000);
        assert_eq!(d.episodes[0].rk,"42");
        assert!(!read_due(&d,30_999)); assert!(read_due(&d,31_000));
        land_read(&mut d,Some(serde_json::json!({"episodes":[]})),31_000);
        assert!(!read_due(&d,99_000)); assert!(d.series_notice.is_empty());
        d.series_attempted=false;
        land_read(&mut d,None,u32::MAX-1000);
        assert!(!read_due(&d,20_000)); assert!(read_due(&d,30_000));
    }
    #[test]
    fn catalog_accepts_queue_states_and_request_reply_explains_deduplication() {
        for state in ["Requested","Queued"] {
            let c=Catalog {episodes:vec![Row {season:1,number:1,title:"Episode".into(),summary:String::new(),aired:String::new(),state:state.into()}]};
            assert!(valid_catalog(&c));
            let d=Detail {series_catalog:Some(c),..Default::default()};
            assert!(!requestable(&d,1,Some(1)));
        }
        assert!(request_notice(Some(&serde_json::json!({"request_state":"Already requested"}))).contains("already requested"));
        assert!(request_notice(Some(&serde_json::json!({"request_state":"Queued"}))).contains("queued"));
    }
    #[test]
    fn missing_season_zero_is_named_specials() {
        let mut d=Detail {series_catalog:Some(Catalog {episodes:vec![Row {season:0,number:1,title:"Special".into(),summary:String::new(),aired:String::new(),state:"Missing".into()}]}),..Default::default()};
        merge(&mut d);
        assert_eq!(d.seasons[0].title,"Specials");
    }
    #[test]
    fn series_play_skips_missing_placeholders_without_changing_local_fallbacks() {
        let mut d=Detail {episodes:vec![Episode {rk:"missing:1:1".into(),..Default::default()},Episode {rk:"42".into(),..Default::default()}],..Default::default()};
        assert_eq!(first_local_episode(&d).unwrap().rk,"42");
        assert_eq!(super::super::version_episode(&d).unwrap().rk,"42");
        d.episodes.pop(); assert!(first_local_episode(&d).is_none());
        assert!(super::super::version_episode(&d).is_none());
    }
    #[test]
    fn merge_preserves_selected_season_and_local_playback_identity() {
        let mut d=Detail { is_show:true,seasons:vec![Season {rk:"s2".into(),index:2,title:"Season 2".into(),leaf_count:1,viewed_leaf_count:0}],
            episodes:vec![Episode {rk:"42".into(),part:"/library/parts/1/file".into(),season:2,index:1,..Default::default()}],..Default::default() };
        d.series_catalog=Some(Catalog {episodes:vec![1,2,3].into_iter().map(|n|Row {season:if n==3 {1}else{2},number:n,title:"Episode".into(),summary:String::new(),aired:String::new(),state:"Missing".into()}).collect()});
        merge(&mut d); assert_eq!(d.cur_season,1); assert_eq!(d.episodes.len(),2);
        assert_eq!(d.episodes[0].rk,"42"); assert_eq!(d.episodes[0].part,"/library/parts/1/file");
        assert!(missing(&d.episodes[1])); assert!(d.episodes[1].part.is_empty());
        merge(&mut d); assert_eq!(d.episodes.len(),2);
    }
}
