//! Plex Discover identities are never PMS rating keys. All writes use the active profile.
use super::{account::AccountClient, models::Metadata};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::time::{Duration, Instant};

const DISCOVER: &str = "https://discover.provider.plex.tv";
const METADATA: &str = "https://metadata.provider.plex.tv";
const DISCOVERY_TITLES: usize = 24;
const DISCOVERY_PEOPLE: usize = 16;
const DISCOVERY_BYTES: usize = 2 * 1024 * 1024;
const DISCOVERY_DEADLINE: Duration = Duration::from_secs(20);

#[derive(Clone, Debug)]
pub(crate) struct Trailer {
    pub key: String,
    pub title: String,
}

/// Only progressive catalog extras from the fixed Plex metadata origin. No URLs,
/// traversal, query strings or caller-supplied authentication may enter this path.
pub(crate) fn valid_trailer_key(key: &str) -> bool {
    let p: Vec<_> = key.split('/').collect();
    p.len() == 8 && p[0].is_empty() && p[1] == "library" && p[2] == "metadata"
        && valid_id(p[3]) && p[4] == "extras" && valid_id(p[5]) && p[6] == "parts"
        && matches!(p[7], "mp4-5000" | "mp4-2500" | "mp4-750")
}

fn pick_catalog_trailer(id: &str, rows: &[Metadata]) -> Option<Trailer> {
    rows.iter().filter(|m| m.kind == "clip" && (m.extra_type == 1 || m.subtype == "trailer"))
        .find_map(|m| m.media.iter().filter(|media| media.video_codec == "h264" && media.audio_codec == "aac")
            .flat_map(|media| &media.part).find(|part| valid_trailer_key(&part.key)
                && part.key.starts_with(&format!("/library/metadata/{id}/extras/")))
            .map(|part| Trailer { key: part.key.clone(), title: m.title.clone() }))
}

#[cfg(test)]
mod trailer_tests {
    use super::*;
    #[test]
    fn catalog_trailer_only_accepts_its_own_progressive_h264_aac_part() {
        let id="5d776cac47dd6e001f6ee059";
        let key=format!("/library/metadata/{id}/extras/620427be133b2900559ccaa1/parts/mp4-2500");
        let rows: Vec<Metadata> = serde_json::from_value(serde_json::json!([{
            "type":"clip","extraType":1,"title":"Trailer","ratingKey":"iva-949134",
            "Media":[{"videoCodec":"h264","audioCodec":"aac","Part":[{"key":key}]}]
        }])).unwrap();
        assert_eq!(pick_catalog_trailer(id,&rows).unwrap().key,key);
        assert!(pick_catalog_trailer("5eb8f22237ccbe0041723253",&rows).is_none());
        for bad in [format!("https://evil.test{key}"),format!("{key}?token=bad"),key.replace("/parts/", "/../parts/"),key.replace("mp4-2500","hls-2500")] {
            assert!(!valid_trailer_key(&bad));
        }
        let client=AccountClient::new("fixture",Some("private test"));
        let url=client.catalog_trailer_url(&key).unwrap();
        assert!(url.starts_with("https://metadata.provider.plex.tv:443/library/metadata/"));
        assert!(url.ends_with("X-Plex-Token=private%20test"));
        assert!(client.catalog_trailer_url("https://evil.test/").is_none());
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DiscoverySeed {
    /// The 24-hex component of an exact plex://movie|show/<id> GUID, never a PMS key.
    Title { id: String },
    /// Provider Role.id, confirmed by its /library/people/<id> key; not a numeric PMS tag.
    Person { id: String },
    /// A validated, provider-supplied browse key, confined to the Discover origin.
    Hub { key:String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiscoveryPerson {
    pub id: String,
    pub name: String,
    pub thumb: String,
}

#[derive(Default)]
pub(crate) struct DiscoveryPage {
    pub similar: Vec<Metadata>,
    pub people: Vec<DiscoveryPerson>,
}

impl std::fmt::Debug for DiscoveryPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscoveryPage")
            .field("similar_count", &self.similar.len())
            .field("people", &self.people)
            .finish()
    }
}

// Verified read-only against Plex providers, 2026-09-18:
// - Metadata Role has string id + key, not PMS Tag.id/tagKey.
// - Metadata Similar[].guid names explicit movie/show relationships. Hydrate those exact
//   GUIDs, not their display tags. Discover /related's similarTitles hub advertises free VOD
//   availability and returned unrelated titles for Interstellar; it is NOT a fallback here.
// - Person /credits returns CreditGroup[].Credit[].Metadata without guid. Hydrate the
//   bounded exact IDs in one metadata request; Container-Size is required for the batch.
// Fixtures below are public projections from Interstellar, Severance and their cast.
fn discovery_container(bytes: &[u8]) -> Option<Value> {
    if bytes.len() > DISCOVERY_BYTES {
        return None;
    }
    #[derive(Deserialize)]
    struct Wire {
        #[serde(rename = "MediaContainer")]
        container: Value,
    }
    let wire: Wire = serde_json::from_slice(bytes).ok()?;
    wire.container.is_object().then_some(wire.container)
}

fn catalog_identity(row: &Value) -> Option<(&str, &str)> {
    let kind = row.get("type")?.as_str()?;
    let id = row.get("ratingKey")?.as_str()?;
    (matches!(kind, "movie" | "show") && valid_id(id)).then_some((kind, id))
}

fn exact_catalog_identity(row: &Value) -> Option<(&str, &str)> {
    let (kind, id) = catalog_identity(row)?;
    (row.get("guid")?.as_str()? == format!("plex://{kind}/{id}")).then_some((kind, id))
}

fn title_metadata(bytes: &[u8], id: &str) -> Option<Value> {
    let c = discovery_container(bytes)?;
    let rows = c.get("Metadata")?.as_array()?;
    if rows.len() != 1 || exact_catalog_identity(&rows[0])?.1 != id {
        return None;
    }
    Some(rows[0].clone())
}

fn title_cast(bytes: &[u8], id: &str) -> Option<Vec<DiscoveryPerson>> {
    let row = title_metadata(bytes, id)?;
    let Some(roles) = row.get("Role") else {
        return Some(Vec::new());
    };
    let mut seen = HashSet::new();
    Some(
        roles
            .as_array()?
            .iter()
            .filter_map(|role| {
                let person = role.get("id")?.as_str()?;
                let name = role.get("tag")?.as_str()?;
                if !valid_id(person)
                    || role.get("type")?.as_str()? != "person"
                    || role.get("key")?.as_str()? != format!("/library/people/{person}")
                    || name.trim().is_empty()
                    || name.len() > 512
                    || name.chars().any(char::is_control)
                    || !seen.insert(person.to_owned())
                {
                    return None;
                }
                Some(DiscoveryPerson {
                    id: person.into(),
                    name: name.into(),
                    thumb: role
                        .get("thumb")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                })
            })
            .take(DISCOVERY_PEOPLE)
            .collect(),
    )
}

fn similar_ids(bytes: &[u8], id: &str) -> Option<Vec<(String, String)>> {
    let row = title_metadata(bytes, id)?;
    let Some(similar) = row.get("Similar") else {
        return Some(Vec::new());
    };
    let mut seen = HashSet::new();
    Some(
        similar
            .as_array()?
            .iter()
            .filter_map(|related| {
                let (kind, related_id) = related
                    .get("guid")?
                    .as_str()?
                    .strip_prefix("plex://")?
                    .split_once('/')?;
                (matches!(kind, "movie" | "show")
                    && valid_id(related_id)
                    && related_id != id
                    && seen.insert(related_id.to_owned()))
                .then(|| (kind.to_owned(), related_id.to_owned()))
            })
            .take(DISCOVERY_TITLES)
            .collect(),
    )
}

fn exact_person(bytes: &[u8], id: &str) -> Option<()> {
    let c = discovery_container(bytes)?;
    let rows = c.get("Metadata")?.as_array()?;
    if rows.len() != 1 {
        return None;
    }
    let row = &rows[0];
    (row.get("type")?.as_str()? == "person"
        && row.get("ratingKey")?.as_str()? == id
        && row.get("metadataId")?.as_str()? == id
        && row.get("key")?.as_str()? == format!("/library/people/{id}"))
    .then_some(())
}

fn credit_ids(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    let c = discovery_container(bytes)?;
    let Some(groups) = c.get("CreditGroup") else {
        return Some(Vec::new());
    };
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for group in groups.as_array()? {
        let Some(credits) = group.get("Credit") else {
            continue;
        };
        for credit in credits.as_array()? {
            let Some(row) = credit.get("Metadata") else {
                continue;
            };
            let Some((kind, id)) = catalog_identity(row) else {
                continue;
            };
            if row.get("key").and_then(Value::as_str)
                != Some(format!("/library/metadata/{id}").as_str())
                || !seen.insert(id.to_owned())
            {
                continue;
            }
            ids.push((kind.into(), id.into()));
            if ids.len() == DISCOVERY_TITLES {
                return Some(ids);
            }
        }
    }
    Some(ids)
}

fn discovery_titles(
    bytes: &[u8],
    seed: Option<&str>,
    credits: Option<&[(String, String)]>,
) -> Option<Vec<Metadata>> {
    let c = discovery_container(bytes)?;
    let Some(rows) = c.get("Metadata") else {
        return Some(Vec::new());
    };
    let mut seen = HashSet::new();
    let mut titles: Vec<Metadata> = rows
        .as_array()?
        .iter()
        .filter_map(|row| {
            let (kind, id) = exact_catalog_identity(row)?;
            if seed == Some(id)
                || credits.is_some_and(|list| !list.iter().any(|(k, i)| k == kind && i == id))
                || !seen.insert(id.to_owned())
            {
                return None;
            }
            let metadata: Metadata = serde_json::from_value(row.clone()).ok()?;
            (!metadata.title.trim().is_empty()).then_some(metadata)
        })
        .take(DISCOVERY_TITLES)
        .collect();
    if let Some(credits) = credits {
        titles.sort_by_key(|m| {
            credits
                .iter()
                .position(|(_, id)| *id == m.rating_key)
                .unwrap_or(usize::MAX)
        });
    }
    Some(titles)
}

fn hydrate_titles(
    ids: &[(String, String)],
    seed: Option<&str>,
    get: &mut impl FnMut(&str) -> Option<Vec<u8>>,
) -> Option<Vec<Metadata>> {
    if ids.is_empty() {
        return Some(Vec::new());
    }
    let batch = ids
        .iter()
        .map(|(_, id)| id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    discovery_titles(
        &get(&format!(
            "{METADATA}/library/metadata/{batch}?X-Plex-Container-Size={DISCOVERY_TITLES}"
        ))?,
        seed,
        Some(ids),
    )
}

fn discover_with(
    seed: &DiscoverySeed,
    mut get: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Option<DiscoveryPage> {
    match seed {
        DiscoverySeed::Hub{..}=>None, // Hub pages use discovery_home's indexed-feed protocol.
        DiscoverySeed::Title { id } => {
            if !valid_id(id) {
                return None;
            }
            let metadata = get(&format!("{METADATA}/library/metadata/{id}"))?;
            let people = title_cast(&metadata, id)?;
            let ids = similar_ids(&metadata, id)?;
            let similar = hydrate_titles(&ids, Some(id), &mut get)?;
            Some(DiscoveryPage { similar, people })
        }
        DiscoverySeed::Person { id } => {
            if !valid_id(id) {
                return None;
            }
            exact_person(&get(&format!("{DISCOVER}/library/people/{id}"))?, id)?;
            let credits = credit_ids(&get(&format!("{DISCOVER}/library/people/{id}/credits"))?)?;
            let similar = hydrate_titles(&credits, None, &mut get)?;
            Some(DiscoveryPage {
                similar,
                people: Vec::new(),
            })
        }
    }
}

fn active_token<'a>(
    profile: Option<&'a super::session::UserRef>,
    account_token: &'a str,
) -> Option<&'a str> {
    // None means detached, not the owner: never revive the persisted profile here.
    let user = profile?;
    let token = if !user.token.is_empty() {
        user.token.as_str()
    } else if user.id != 0 || !user.uuid.is_empty() {
        return None; // A selected Home profile must bring its own credential.
    } else {
        account_token
    };
    (!token.trim().is_empty()).then_some(token)
}

fn profile_authorized(
    headers: &[String],
    profile: Option<&super::session::UserRef>,
    account_token: &str,
) -> bool {
    active_token(profile, account_token).is_some_and(|token| {
        headers
            .iter()
            .filter_map(|h| h.strip_prefix("X-Plex-Token: "))
            .eq(std::iter::once(token))
    })
}

fn discovery_timeout(elapsed: Duration) -> Option<u64> {
    let seconds = DISCOVERY_DEADLINE.checked_sub(elapsed)?.as_secs().min(8);
    (seconds > 0).then_some(seconds)
}

pub(crate) fn valid_id(id: &str) -> bool {
    id.len() == 24 && id.bytes().all(|c| c.is_ascii_hexdigit())
}

pub(crate) fn active_client() -> Option<AccountClient> {
    let s = super::session::peek();
    let user = super::session::current();
    let token = active_token(user.as_ref(), &s.account_token)?;
    Some(AccountClient::new(&s.client_id, Some(token)))
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "MediaContainer")]
    container: Container,
}
#[derive(Deserialize)]
struct Container {
    #[serde(rename = "SearchResults", default)]
    groups: Vec<Group>,
    #[serde(rename = "Metadata", default)]
    metadata: Vec<Metadata>,
}
#[derive(Deserialize)]
struct Group {
    #[serde(default)]
    id: String,
    #[serde(rename = "SearchResult", default)]
    results: Vec<ResultItem>,
}
#[derive(Deserialize)]
struct ResultItem {
    #[serde(rename = "Metadata")]
    metadata: Metadata,
}

fn parse_search(bytes: &[u8]) -> Option<Vec<Metadata>> {
    let response: Envelope = serde_json::from_slice(bytes).ok()?;
    Some(
        response
            .container
            .groups
            .into_iter()
            .filter(|g| g.id == "external")
            .flat_map(|g| g.results)
            .map(|r| r.metadata)
            .filter(|m| {
                matches!(m.kind.as_str(), "movie" | "show")
                    && valid_id(&m.rating_key)
                    && m.guid == format!("plex://{}/{}", m.kind, m.rating_key)
            })
            .take(60)
            .collect(),
    )
}

impl AccountClient {
    /// Contextual provider discovery, read-only and blocking. Run on a worker.
    /// At most three GETs, 2 MiB each, with one 20-second deadline. No redirects,
    /// credential switching, or retries; a profile change invalidates the whole result.
    /// The active owner may use the account token; a selected Home profile never does.
    /// Titles use the metadata provider's explicit Similar GUIDs, never its free VOD hub.
    pub(crate) fn discover_related(&self, seed: &DiscoverySeed) -> Option<DiscoveryPage> {
        let generation = super::session::current_gen();
        let headers = self.headers();
        if !profile_authorized(
            &headers,
            super::session::current().as_ref(),
            &super::session::peek().account_token,
        ) {
            return None;
        }
        let start = Instant::now();
        let page = discover_with(seed, |url| {
            if generation != super::session::current_gen() {
                return None;
            }
            let remaining = discovery_timeout(start.elapsed())?;
            let response = crate::net::request(
                url,
                &headers,
                "GET",
                None,
                crate::net::Timeouts {
                    connect_s: remaining.min(4) as _,
                    total_s: remaining as _,
                    ..crate::net::API
                },
                false,
                Some(DISCOVERY_BYTES),
                None,
            )?;
            (response.status == 200 && generation == super::session::current_gen())
                .then_some(response.body)
        })?;
        (generation == super::session::current_gen() && start.elapsed() < DISCOVERY_DEADLINE)
            .then_some(page)
    }

    fn catalog_request(&self, url: &str, method: &str) -> Option<crate::net::Resp> {
        crate::net::request(
            url,
            &self.headers(),
            method,
            None,
            crate::net::Timeouts {
                connect_s: 4,
                total_s: 10,
                ..crate::net::API
            },
            false,
            Some(2 * 1024 * 1024),
            None,
        )
        .filter(|r| (200..300).contains(&r.status))
    }
    pub(crate) fn catalog_search(&self, query: &str) -> Option<Vec<Metadata>> {
        let q = crate::pms::urlenc_str(query);
        let r = self.catalog_request(&format!("{DISCOVER}/library/search?query={q}&limit=60&searchTypes=movies%2Ctv&searchProviders=discover&includeMetadata=1"), "GET")?;
        parse_search(&r.body)
    }
    pub(crate) fn watchlisted(&self, id: &str) -> Option<bool> {
        if !valid_id(id) {
            return None;
        }
        let r = self.catalog_request(
            &format!("{METADATA}/library/metadata/{id}/userState"),
            "GET",
        )?;
        watchlist_state(&r.body)
    }
    pub(crate) fn catalog_detail(&self, id: &str) -> Option<Metadata> {
        if !valid_id(id) {
            return None;
        }
        let r = self.catalog_request(&format!("{METADATA}/library/metadata/{id}"), "GET")?;
        let response: Envelope = serde_json::from_slice(&r.body).ok()?;
        response.container.metadata.into_iter().find(|m| {
            m.rating_key == id
                && matches!(m.kind.as_str(), "movie" | "show")
                && m.guid == format!("plex://{}/{}", m.kind, id)
        })
    }
    pub(crate) fn catalog_trailer(&self, id: &str) -> Option<Trailer> {
        if !valid_id(id) { return None; }
        let r = self.catalog_request(&format!("{METADATA}/library/metadata/{id}/extras"), "GET")?;
        let response: Envelope = serde_json::from_slice(&r.body).ok()?;
        pick_catalog_trailer(id, &response.container.metadata)
    }
    pub(crate) fn set_watchlisted(&self, id: &str, desired: bool) -> Option<bool> {
        if !valid_id(id) {
            return None;
        }
        if self.watchlisted(id)? == desired {
            return Some(desired);
        }
        let verb = if desired {
            "addToWatchlist"
        } else {
            "removeFromWatchlist"
        };
        self.catalog_request(&format!("{DISCOVER}/actions/{verb}?ratingKey={id}"), "PUT")?;
        // Success is server readback, not a hopeful button toggle. Never auto-retry a write.
        self.watchlisted(id).filter(|v| *v == desired)
    }
}

fn watchlist_state(bytes: &[u8]) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let c = v.get("MediaContainer")?;
    let state = c
        .get("UserState")
        .and_then(|u| u.as_array().and_then(|a| a.first()).or(Some(u)))
        .or_else(|| c.get("Metadata")?.as_array()?.first())?;
    if !state.is_object() {
        return None;
    }
    Some(state.get("watchlistedAt").is_some_and(|x| {
        x.as_i64()
            .unwrap_or_else(|| x.as_str().and_then(|s| s.parse().ok()).unwrap_or(0))
            > 0
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const MOVIE: &str = "5d7769cefb0d55001f530acd";
    const PERSON: &str = "5d7768287e9a3c0020c6adeb";
    const CREDIT: &str = "6526e65f8ea21bb8cbbbb6ae";

    // Minimal projections of authenticated GET responses observed 2026-09-18.
    // Public catalog facts only; no account fields, token, or response headers.
    fn live_title() -> Value {
        json!({"MediaContainer":{"Metadata":[{
            "type":"movie","ratingKey":MOVIE,"guid":format!("plex://movie/{MOVIE}"),"title":"Interstellar",
            "Similar":[
                {"guid":"plex://movie/5d77682a3c3c2a001fbcbff4","tag":"Solaris"},
                {"guid":"plex://movie/5e40c3e46d9025003f6cd4ed","tag":"The Creator"}],
            "Role":[{"id":PERSON,"order":1,"tag":"Matthew McConaughey","role":"Cooper",
                "thumb":"https://metadata-static.plex.tv/b/people/b604d520903d1d1f445458008ab9d1f3.jpg",
                "key":format!("/library/people/{PERSON}"),"type":"person"}]
        }]}})
    }
    fn live_hubs() -> Value {
        json!({"MediaContainer":{"identifier":"com.plexapp.plugins.library","size":4,"Hub":[
            {"title":"Extras","type":"clip","placeholder":true,"hubIdentifier":"extras",
                "key":format!("/library/metadata/{MOVIE}/extras")},
            {"title":"Titles You Might Like on Plex","type":"mixed","placeholder":true,
                "hubIdentifier":"similarTitles","source":"provider://tv.plex.provider.vod",
                "key":format!("/library/metadata/{MOVIE}/similarTitles")}
        ]}})
    }
    fn live_recommendations() -> Value {
        json!({"MediaContainer":{"Metadata":[
            {"type":"movie","ratingKey":"5d776d1a594b2b001e7015fc","guid":"plex://movie/5d776d1a594b2b001e7015fc","title":"Labyrinth"},
            {"type":"movie","ratingKey":"5d7768328a7581001f12d0db","guid":"plex://movie/5d7768328a7581001f12d0db","title":"The Gods Must Be Crazy"}
        ]}})
    }
    fn live_person() -> Value {
        json!({"MediaContainer":{"Metadata":[{"type":"person","title":"Matthew McConaughey",
            "ratingKey":PERSON,"metadataId":PERSON,"key":format!("/library/people/{PERSON}")}]}})
    }
    fn live_credits() -> Value {
        json!({"MediaContainer":{"CreditGroup":[{"type":"actor","title":"Actor","Credit":[
            {"order":1,"role":"Buster Moon (voice)","Metadata":{"type":"movie","ratingKey":CREDIT,
                "key":format!("/library/metadata/{CREDIT}"),"title":"Sing 3","year":2027,
                "thumb":"https://image.tmdb.org/t/p/original/xnR5dSaH7FpfHRxKL7rLU3HBzpM.jpg"}}
        ]}]}})
    }
    fn live_hydrated_credit() -> Value {
        json!({"MediaContainer":{"Metadata":[{"type":"movie","ratingKey":CREDIT,
            "guid":format!("plex://movie/{CREDIT}"),"title":"Sing 3"}]}})
    }
    fn bytes(value: &Value) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }

    #[test]
    fn contextual_title_uses_exact_similar_guids_not_the_free_vod_hub() {
        let hydrated = json!({"MediaContainer":{"Metadata":[
            {"type":"movie","ratingKey":"5d77682a3c3c2a001fbcbff4",
                "guid":"plex://movie/5d77682a3c3c2a001fbcbff4","title":"Solaris"},
            {"type":"movie","ratingKey":"5e40c3e46d9025003f6cd4ed",
                "guid":"plex://movie/5e40c3e46d9025003f6cd4ed","title":"The Creator"}]}});
        let mut calls = Vec::new();
        let page = discover_with(&DiscoverySeed::Title { id: MOVIE.into() }, |url| {
            calls.push(url.to_owned());
            if url == format!("{METADATA}/library/metadata/{MOVIE}") {
                Some(bytes(&live_title()))
            } else if url == format!("{METADATA}/library/metadata/5d77682a3c3c2a001fbcbff4,5e40c3e46d9025003f6cd4ed?X-Plex-Container-Size=24") {
                Some(bytes(&hydrated))
            } else if url == format!("{DISCOVER}/library/metadata/{MOVIE}/related?count=24") {
                Some(bytes(&live_hubs()))
            } else if url == format!("https://vod.provider.plex.tv/library/metadata/{MOVIE}/similarTitles?count=24") {
                Some(bytes(&live_recommendations()))
            } else {
                panic!("unexpected request");
            }
        }).unwrap();
        assert_eq!(
            page.similar
                .iter()
                .map(|m| m.title.as_str())
                .collect::<Vec<_>>(),
            ["Solaris", "The Creator"]
        );
        assert_eq!(page.people[0].id, PERSON);
        assert_eq!(calls.len(), 2, "no VOD fallback or recommendation guessing");
    }

    #[test]
    fn missing_similar_relationships_returns_cast_without_vod_fallback() {
        let mut title = live_title();
        title["MediaContainer"]["Metadata"][0]
            .as_object_mut()
            .unwrap()
            .remove("Similar");
        let mut calls = 0;
        let page = discover_with(&DiscoverySeed::Title { id: MOVIE.into() }, |_| {
            calls += 1;
            (calls == 1).then(|| bytes(&title))
        })
        .unwrap();
        assert!(page.similar.is_empty());
        assert_eq!(page.people[0].id, PERSON);
        assert_eq!(calls, 1);
    }

    #[test]
    fn contextual_person_hydrates_credit_ids_before_returning_portable_titles() {
        let mut calls = Vec::new();
        let page = discover_with(&DiscoverySeed::Person { id: PERSON.into() }, |url| {
            calls.push(url.to_owned());
            Some(bytes(&match calls.len() {
                1 => live_person(),
                2 => live_credits(),
                3 => live_hydrated_credit(),
                _ => panic!("unbounded requests"),
            }))
        })
        .unwrap();
        assert_eq!(
            calls,
            [
                format!("{DISCOVER}/library/people/{PERSON}"),
                format!("{DISCOVER}/library/people/{PERSON}/credits"),
                format!("{METADATA}/library/metadata/{CREDIT}?X-Plex-Container-Size=24"),
            ]
        );
        assert!(page.people.is_empty());
        assert_eq!(page.similar.len(), 1);
        assert_eq!(page.similar[0].guid, format!("plex://movie/{CREDIT}"));
    }
    #[test]
    fn discovery_rejects_local_keys_injection_and_ambiguous_seed_metadata() {
        for id in [
            "42",
            "../people",
            "5d7769cefb0d55001f530acd?x=1",
            "plex://movie/5d7769cefb0d55001f530acd",
        ] {
            for seed in [
                DiscoverySeed::Title { id: id.into() },
                DiscoverySeed::Person { id: id.into() },
            ] {
                assert!(discover_with(&seed, |_| panic!("invalid seed reached network")).is_none());
            }
        }
        let mut title = live_title();
        title["MediaContainer"]["Metadata"][0]["guid"] = json!(format!("plex://show/{MOVIE}"));
        assert!(title_cast(&bytes(&title), MOVIE).is_none());
        let mut title = live_title();
        let row = title["MediaContainer"]["Metadata"][0].clone();
        title["MediaContainer"]["Metadata"]
            .as_array_mut()
            .unwrap()
            .push(row);
        assert!(title_cast(&bytes(&title), MOVIE).is_none());
        assert!(exact_person(&bytes(&live_person()), MOVIE).is_none());
    }
    #[test]
    fn cast_requires_provider_id_and_matching_key_not_numeric_tag_or_slug() {
        let mut title = live_title();
        for value in [json!(465), json!("matthew-mcconaughey"), json!(MOVIE)] {
            title["MediaContainer"]["Metadata"][0]["Role"][0]["id"] = value;
            assert!(title_cast(&bytes(&title), MOVIE).unwrap().is_empty());
        }
        let mut title = live_title();
        let role = title["MediaContainer"]["Metadata"][0]["Role"][0].clone();
        title["MediaContainer"]["Metadata"][0]["Role"]
            .as_array_mut()
            .unwrap()
            .push(role);
        assert_eq!(title_cast(&bytes(&title), MOVIE).unwrap().len(), 1);
    }
    #[test]
    fn similar_relationships_require_exact_guids_and_never_follow_tags_or_urls() {
        let mut title = live_title();
        title["MediaContainer"]["Metadata"][0]["Similar"] = json!([
            {"guid":format!("https://example.invalid/{CREDIT}")},
            {"guid":format!("plex://movie/{CREDIT}?X-Plex-Token=bad")},
            {"guid":format!("plex://person/{PERSON}")},
            {"guid":format!("plex://movie/{MOVIE}")},
            {"guid":"plex://movie/42","tag":"Inception"},
            {"tag":"The Martian"},
            {"guid":format!("plex://movie/{CREDIT}")},
            {"guid":format!("plex://movie/{CREDIT}")}
        ]);
        assert_eq!(
            similar_ids(&bytes(&title), MOVIE).unwrap(),
            vec![("movie".into(), CREDIT.into())]
        );
        assert!(similar_ids(&bytes(&title), CREDIT).is_none());
    }
    #[test]
    fn discovery_deduplicates_exact_ids_excludes_seed_and_rejects_uncredited_hydration() {
        let mut v = live_recommendations();
        let rows = v["MediaContainer"]["Metadata"].as_array_mut().unwrap();
        rows.push(rows[0].clone());
        rows.push(live_title()["MediaContainer"]["Metadata"][0].clone());
        rows.push(
            json!({"type":"movie","ratingKey":"42","guid":"plex://movie/42","title":"Local"}),
        );
        assert_eq!(
            discovery_titles(&bytes(&v), Some(MOVIE), None)
                .unwrap()
                .len(),
            2
        );
        let credits = credit_ids(&bytes(&live_credits())).unwrap();
        assert!(discovery_titles(&bytes(&v), None, Some(&credits))
            .unwrap()
            .is_empty());
        let mut wrong = live_hydrated_credit();
        wrong["MediaContainer"]["Metadata"][0]["type"] = json!("show");
        wrong["MediaContainer"]["Metadata"][0]["guid"] = json!(format!("plex://show/{CREDIT}"));
        assert!(discovery_titles(&bytes(&wrong), None, Some(&credits))
            .unwrap()
            .is_empty());
    }
    #[test]
    fn discovery_is_bounded_and_empty_is_distinct_from_failure() {
        let rows: Vec<_> = (0..100).map(|i| { let id = format!("{i:024x}");
            json!({"type":"movie","ratingKey":id,"guid":format!("plex://movie/{id}"),"title":"Fixture"}) }).collect();
        assert_eq!(
            discovery_titles(
                &bytes(&json!({"MediaContainer":{"Metadata":rows}})),
                None,
                None
            )
            .unwrap()
            .len(),
            DISCOVERY_TITLES
        );
        assert!(discovery_container(&vec![b' '; DISCOVERY_BYTES + 1]).is_none());
        assert!(discovery_container(b"{}").is_none());
        assert!(discovery_container(br#"{"MediaContainer":null}"#).is_none());
        assert!(credit_ids(br#"{"MediaContainer":{"CreditGroup":[]}}"#)
            .unwrap()
            .is_empty());
        let mut calls = 0;
        assert!(
            discover_with(&DiscoverySeed::Title { id: MOVIE.into() }, |_| {
                calls += 1;
                if calls == 1 {
                    Some(bytes(&live_title()))
                } else {
                    None
                }
            })
            .is_none()
        );
        assert_eq!(calls, 2, "no alternate credential or endpoint retry");
    }
    #[test]
    fn discovery_cast_and_credits_have_independent_hard_limits() {
        let mut title = live_title();
        title["MediaContainer"]["Metadata"][0]["Role"] = json!((0..100).map(|i| {
            let id = format!("{i:024x}");
            json!({"id":id,"type":"person","key":format!("/library/people/{id}"),"tag":"Fixture"})
        }).collect::<Vec<_>>());
        assert_eq!(
            title_cast(&bytes(&title), MOVIE).unwrap().len(),
            DISCOVERY_PEOPLE
        );
        let row = live_credits()["MediaContainer"]["CreditGroup"][0]["Credit"][0].clone();
        let mut credits = live_credits();
        credits["MediaContainer"]["CreditGroup"][0]["Credit"] = json!((0..100)
            .map(|i| {
                let id = format!("{i:024x}");
                let mut next = row.clone();
                next["Metadata"]["ratingKey"] = json!(id);
                next["Metadata"]["key"] = json!(format!("/library/metadata/{id}"));
                next
            })
            .collect::<Vec<_>>());
        assert_eq!(
            credit_ids(&bytes(&credits)).unwrap().len(),
            DISCOVERY_TITLES
        );
        assert_eq!(discovery_timeout(Duration::ZERO), Some(8));
        assert_eq!(discovery_timeout(Duration::from_secs(16)), Some(4));
        assert_eq!(discovery_timeout(Duration::from_secs(20)), None);
        assert_eq!(discovery_timeout(Duration::from_secs(21)), None);
    }
    #[test]
    fn verified_show_cast_uses_the_same_global_person_contract() {
        let show = json!({"MediaContainer":{"Metadata":[{"type":"show","ratingKey":"5e1622d34f2292003e91314a",
            "guid":"plex://show/5e1622d34f2292003e91314a","title":"Severance","Role":[{
                "id":"5d77682beb5d26001f1de8cb","type":"person","key":"/library/people/5d77682beb5d26001f1de8cb",
                "tag":"Adam Scott","role":"Mark Scout"}]}]}});
        let people = title_cast(&bytes(&show), "5e1622d34f2292003e91314a").unwrap();
        assert_eq!(people[0].name, "Adam Scott");
        assert_eq!(people[0].id, "5d77682beb5d26001f1de8cb");
    }
    #[test]
    fn discovery_authorizes_the_active_owner_account_client() {
        let _guard = crate::testlock::serial();
        let _session = super::super::session::TempSession::new(
            "catalog-discovery_authorizes_the_active_owner_account_client",
        );
        super::super::session::save(&super::super::session::Session {
            client_id: "catalog-test".into(),
            account_token: "owner".into(),
            ..Default::default()
        });
        super::super::session::publish_profile_for_test(
            Some(super::super::session::UserRef::default()),
            super::super::session::current_gen().wrapping_add(1),
        );
        let authorized = active_client().is_some_and(|client| {
            profile_authorized(
                &client.headers(),
                super::super::session::current().as_ref(),
                &super::super::session::peek().account_token,
            )
        });
        assert!(
            authorized,
            "the active owner's account client must pass discovery authorization"
        );
    }

    #[test]
    fn detached_profile_cannot_create_a_client_from_the_persisted_session() {
        let _guard = crate::testlock::serial();
        let _session = super::super::session::TempSession::new(
            "catalog-detached_profile_cannot_create_a_client_from_the_persisted_session",
        );
        let mut accepted = Vec::new();
        for user in [
            super::super::session::UserRef::default(),
            super::super::session::UserRef {
                id: 7,
                uuid: "previous-profile".into(),
                token: "previous-profile-token".into(),
                ..Default::default()
            },
        ] {
            super::super::session::save(&super::super::session::Session {
                client_id: "catalog-test".into(),
                account_token: "owner".into(),
                user,
                ..Default::default()
            });
            super::super::session::publish_profile_for_test(
                None,
                super::super::session::current_gen().wrapping_add(1),
            );
            accepted.push(active_client().is_some());
        }
        assert_eq!(
            accepted,
            [false, false],
            "detachment must not revive persisted credentials"
        );
    }

    #[test]
    fn partial_hydration_keeps_only_returned_exact_relationships() {
        // Provider batches can return fewer rows than the requested IDs (observed 2026-09-19).
        let mut calls = 0;
        let page = discover_with(&DiscoverySeed::Title { id: MOVIE.into() }, |_| {
            calls += 1;
            Some(bytes(&match calls {
                1 => live_title(),
                2 => json!({"MediaContainer":{"size":1,"Metadata":[{
                    "type":"movie","ratingKey":"5d77682a3c3c2a001fbcbff4",
                    "guid":"plex://movie/5d77682a3c3c2a001fbcbff4","title":"Solaris"}]}}),
                _ => panic!("no retries for omitted titles"),
            }))
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(page.similar.len(), 1);
        assert_eq!(page.similar[0].title, "Solaris");
        assert_eq!(page.people[0].id, PERSON);
    }

    #[test]
    fn refused_credits_do_not_retry_or_become_an_empty_filmography() {
        let mut calls = 0;
        let page = discover_with(&DiscoverySeed::Person { id: PERSON.into() }, |_| {
            calls += 1;
            match calls {
                1 => Some(bytes(&live_person())),
                2 => None, // HTTP 401 remains a transport failure, not an empty result.
                _ => panic!("no credential or endpoint fallback"),
            }
        });
        assert!(page.is_none());
        assert_eq!(calls, 2);
    }

    #[test]
    fn discovery_requires_the_active_profiles_credential_without_fallback() {
        let user = super::super::session::UserRef {
            token: "profile".into(),
            ..Default::default()
        };
        assert!(profile_authorized(
            &["X-Plex-Token: profile".into()],
            Some(&user),
            "owner",
        ));
        assert!(!profile_authorized(
            &["X-Plex-Token: owner".into()],
            Some(&user),
            "owner",
        ));
        assert!(!profile_authorized(&[], Some(&user), "owner"));
        assert!(!profile_authorized(
            &["X-Plex-Token: profile".into()],
            None,
            "owner"
        ));
        assert!(!profile_authorized(
            &["X-Plex-Token: profile".into(), "X-Plex-Token: owner".into()],
            Some(&user),
            "owner",
        ));
        assert!(!profile_authorized(
            &["X-Plex-Token: owner".into()],
            Some(&super::super::session::UserRef::default()),
            "",
        ));
    }

    #[test]
    fn owner_credential_is_only_for_an_active_unselected_profile() {
        use super::super::session::UserRef;
        let owner = UserRef::default();
        assert_eq!(active_token(Some(&owner), "owner"), Some("owner"));
        assert_eq!(active_token(None, "owner"), None);
        for account in ["", " ", "\t"] {
            assert_eq!(active_token(Some(&owner), account), None);
        }
        for user in [
            UserRef {
                id: 7,
                ..Default::default()
            },
            UserRef {
                id: -1,
                ..Default::default()
            },
            UserRef {
                uuid: "managed".into(),
                ..Default::default()
            },
            UserRef {
                token: " ".into(),
                ..Default::default()
            },
        ] {
            assert_eq!(active_token(Some(&user), "owner"), None);
            assert!(!profile_authorized(
                &["X-Plex-Token: owner".into()],
                Some(&user),
                "owner"
            ));
        }
        let managed = UserRef {
            id: 7,
            uuid: "managed".into(),
            token: "managed-token".into(),
            ..Default::default()
        };
        assert_eq!(active_token(Some(&managed), "owner"), Some("managed-token"));
        assert!(profile_authorized(
            &["X-Plex-Token: managed-token".into()],
            Some(&managed),
            "owner"
        ));
        for headers in [
            vec!["X-Plex-Token: owner".into()],
            vec!["X-Plex-Token: previous-profile".into()],
            vec![
                "X-Plex-Token: managed-token".into(),
                "X-Plex-Token: managed-token".into(),
            ],
        ] {
            assert!(!profile_authorized(&headers, Some(&managed), "owner"));
        }
        assert!(!profile_authorized(
            &["X-Plex-Token: previous-owner".into()],
            Some(&owner),
            "owner"
        ));
    }
    #[test]
    fn catalog_never_accepts_local_ids_or_people() {
        let b = br#"{"MediaContainer":{"SearchResults":[{"id":"external","SearchResult":[{"Metadata":{"type":"show","ratingKey":"5e1622d34f2292003e91314a","guid":"plex://show/5e1622d34f2292003e91314a","title":"Severance"}},{"Metadata":{"type":"movie","ratingKey":"123","guid":"plex://movie/123"}},{"Metadata":{"type":"person","ratingKey":"5e1622d34f2292003e91314a"}}]}]}}"#;
        let r = parse_search(b).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Severance");
        assert!(!valid_id("../actions/add"));
    }
    #[test]
    fn missing_response_is_not_an_empty_watchlist() {
        assert_eq!(watchlist_state(b"{}"), None);
        assert_eq!(
            watchlist_state(br#"{"MediaContainer":{"UserState":{"watchlistedAt":null}}}"#),
            Some(false)
        );
        assert_eq!(
            watchlist_state(br#"{"MediaContainer":{"UserState":[{"watchlistedAt":"123"}]}}"#),
            Some(true)
        );
    }
}
