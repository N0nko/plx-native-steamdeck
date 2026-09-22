//! Bounded, profile-scoped discovery home feeds.
const DISCOVER: &str = "https://discover.provider.plex.tv";
pub(crate) const HOME_ROWS: usize = 64;
const HOME_ITEMS: usize = 60;
const HOME_HUBS: usize = 32;
const HOME_BYTES: usize = 2 * 1024 * 1024;
const HOME_INDEX: &str = "https://discover.provider.plex.tv/hubs/sections/home?count=60";
const HOME_DEADLINE: std::time::Duration = std::time::Duration::from_secs(20);

pub(crate) fn safe_home_key(key: &str) -> bool {
    key.len() <= 512
        && ["/hubs/sections/home/","/library/categories/","/library/awards/","/library/decades/"]
            .iter().find_map(|prefix|key.strip_prefix(prefix))
            .is_some_and(|tail| {
                tail.split('/').all(|part| {
                    !part.is_empty()
                        && !matches!(part, "." | "..")
                        && part
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
                })
            })
}

fn home_timeout(elapsed: std::time::Duration) -> Option<u64> {
    let seconds = HOME_DEADLINE.checked_sub(elapsed)?.as_secs().min(8);
    (seconds > 0).then_some(seconds)
}

fn discovery_home_with(index_url:&str, mut get: impl FnMut(&str) -> Option<Vec<u8>>) -> Option<Vec<DiscoveryHub>> {
    use serde_json::Value;
    let index = get(index_url)?;
    let initial = parse_discovery_home(&index)?;
    if initial.len() == HOME_ROWS {
        return Some(initial);
    }
    let mut wire: Value = serde_json::from_slice(&index).ok()?;
    let Some(hubs) = wire.get_mut("MediaContainer")?.get_mut("Hub") else {
        return Some(initial);
    };
    let hubs = hubs.as_array_mut()?;
    let mut seen = std::collections::HashSet::new();
    let mut fetched = 0;
    let mut failed = 0;
    for hub in hubs.iter_mut() {
        if fetched == HOME_HUBS {
            break;
        }
        if hub
            .get("Metadata").or_else(|| hub.get("Directory"))
            .and_then(Value::as_array)
            .is_some_and(|rows| !rows.is_empty())
        {
            continue;
        }
        if !matches!(
            hub.get("type").and_then(Value::as_str),
            Some("movie" | "show" | "mixed" | "clip" | "directory")
        ) {
            continue;
        }
        if !hub.get("title").and_then(Value::as_str).is_some_and(|s| {
            !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
        }) {
            continue;
        }
        let Some(key) = hub
            .get("key")
            .and_then(Value::as_str)
            .filter(|key| safe_home_key(key))
        else {
            continue;
        };
        if !seen.insert(key.to_owned()) {
            continue;
        }
        fetched += 1;
        let metadata = get(&format!(
            "{DISCOVER}{key}?count={HOME_ITEMS}&X-Plex-Container-Size={HOME_ITEMS}"
        ))
        .filter(|body| body.len() <= HOME_BYTES)
        .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
        .and_then(|page| {
            let container = page.get("MediaContainer")?.as_object()?;
            if let Some(rows) = container.get("Metadata").or_else(||container.get("Directory")) {
                return rows.as_array().cloned().map(|rows|(container.contains_key("Directory"),rows));
            }
            // A leaf may wrap its items in Hub; never recurse into another index.
            let nested = container.get("Hub")?.as_array()?;
            let arrays: Vec<_> = nested
                .iter()
                .filter_map(|h| h.get("Metadata")?.as_array())
                .collect();
            (!arrays.is_empty()).then(|| (false,arrays.into_iter().flatten().cloned().collect()))
        });
        if let Some((directory,metadata)) = metadata {
            hub[if directory {"Directory"}else{"Metadata"}] = Value::Array(metadata);
        } else {
            failed += 1;
        }
    }
    let rows = parse_discovery_home_value(&wire)?;
    crate::log(&format!(
        "discovery-home: hydrated fetched={fetched} failed={failed} rows={} items={}",
        rows.len(),
        rows.iter().map(|r| r.items.len()).sum::<usize>(),
    ));
    // Do not cache a failed hydration as a successful empty feed for fifteen minutes.
    if rows.is_empty() && failed > 0 {
        None
    } else {
        Some(rows)
    }
}

pub(crate) struct DiscoveryHub {
    pub title: String,
    pub kind: String,
    pub items: Vec<super::Metadata>,
    pub directories: Vec<DiscoveryDirectory>,
}
pub(crate) struct DiscoveryDirectory {pub title:String,pub key:String,pub thumb:String}

fn title_metadata(row:&serde_json::Value)->Option<super::Metadata>{
    let mut row=row.clone();
    if row.get("type")?.as_str()?=="clip" {
        // Provider trailers explicitly identify their parent title; never use the clip's ID
        // as a movie identity. The rail opens that title's catalog information.
        let guid=row.get("primaryGuid")?.as_str()?.to_owned();
        let (kind,id)=guid.strip_prefix("plex://")?.split_once('/')?;
        if !matches!(kind,"movie"|"show") || !super::catalog::valid_id(id){return None;}
        row["type"]=kind.into();row["ratingKey"]=id.into();row["guid"]=guid.clone().into();
    }
    let item:super::Metadata=serde_json::from_value(row).ok()?;
    (matches!(item.kind.as_str(),"movie"|"show") && super::catalog::valid_id(&item.rating_key)
        && item.guid==format!("plex://{}/{}",item.kind,item.rating_key) && !item.title.trim().is_empty()).then_some(item)
}

fn parse_discovery_home(bytes: &[u8]) -> Option<Vec<DiscoveryHub>> {
    use serde_json::Value;
    if bytes.len() > HOME_BYTES {
        return None;
    }
    let wire: Value = serde_json::from_slice(bytes).ok()?;
    parse_discovery_home_value(&wire)
}
fn parse_discovery_home_value(wire:&serde_json::Value)->Option<Vec<DiscoveryHub>>{
    use serde_json::Value;
    let container = wire.get("MediaContainer")?.as_object()?;
    let Some(hubs) = container.get("Hub") else {
        return Some(Vec::new());
    };
    let hubs = hubs.as_array()?;
    let mut rows = Vec::new();
    let mut seen_hubs = std::collections::HashSet::new();
    for hub in hubs {
        let Some(title) = hub.get("title").and_then(Value::as_str) else {
            continue;
        };
        if title.trim().is_empty() || title.len() > 256 || title.chars().any(char::is_control) {
            continue;
        }
        let id = hub
            .get("hubIdentifier")
            .and_then(Value::as_str)
            .unwrap_or(title);
        if !seen_hubs.insert(id) {
            continue;
        }
        if let Some(dirs)=hub.get("Directory").and_then(Value::as_array){
            let mut seen=std::collections::HashSet::new();
            let directories=dirs.iter().filter_map(|d|{
                let key=d.get("key")?.as_str()?;let title=d.get("title")?.as_str()?;
                if !safe_home_key(key) || title.trim().is_empty() || title.len()>256 || title.chars().any(char::is_control) || !seen.insert(key){return None;}
                Some(DiscoveryDirectory{title:title.into(),key:key.into(),thumb:d.get("thumb").and_then(Value::as_str).unwrap_or("").into()})
            }).take(HOME_ITEMS).collect::<Vec<_>>();
            if !directories.is_empty(){rows.push(DiscoveryHub{title:title.into(),kind:"directory".into(),items:Vec::new(),directories});}
            if rows.len()>=HOME_ROWS{return Some(rows);}
            continue;
        }
        let Some(metadata)=hub.get("Metadata").and_then(Value::as_array) else {continue;};
        let (movies,shows):(Vec<_>,Vec<_>)=metadata.iter().filter_map(title_metadata).partition(|m|m.kind=="movie");
        let mixed = !movies.is_empty() && !shows.is_empty();
        for (kind, label, metadata) in [("movie", "Movies",movies), ("show", "TV Shows",shows)] {
            let mut seen = std::collections::HashSet::new();
            let items: Vec<super::Metadata> = metadata
                .into_iter()
                .filter(|m|seen.insert(m.rating_key.clone()))
                .take(HOME_ITEMS)
                .collect();
            if items.is_empty() {
                continue;
            }
            rows.push(DiscoveryHub {
                title: if mixed {
                    format!("{title} - {label}")
                } else {
                    title.into()
                },
                kind: kind.into(),
                items,
                directories:Vec::new(),
            });
            if rows.len() == HOME_ROWS {
                return Some(rows);
            }
        }
    }
    Some(rows)
}

// Diagnostics project only fixed shape labels and counts, never wire strings or headers.
fn discovery_home_diagnostic(
    status: u16,
    body: &[u8],
    parsed: Option<&[DiscoveryHub]>,
    stale: bool,
) -> String {
    use serde_json::Value;
    let wire = (body.len() <= HOME_BYTES)
        .then(|| serde_json::from_slice::<Value>(body).ok())
        .flatten();
    let hubs = wire
        .as_ref()
        .and_then(|v| v.get("MediaContainer"))
        .and_then(|c| c.get("Hub"))
        .and_then(Value::as_array);
    let direct = wire
        .as_ref()
        .and_then(|v| v.get("MediaContainer"))
        .and_then(|c| c.get("Metadata"))
        .and_then(Value::as_array);
    let shape = match wire.as_ref() {
        None => "invalid-json-or-oversize",
        Some(v) => match v.get("MediaContainer") {
            None => "missing-container",
            Some(c) if !c.is_object() => "invalid-container",
            Some(c) => match c.get("Hub") {
                None if direct.is_some() => "metadata",
                None => "missing-hubs",
                Some(h) if !h.is_array() => "invalid-hubs",
                Some(_) => "hubs",
            },
        },
    };
    let mut wire_items = 0;
    let mut guid_items = 0;
    if let Some(items) = direct {
        wire_items += items.len();
        guid_items += items
            .iter()
            .filter(|m| m.get("guid").and_then(Value::as_str).is_some())
            .count();
    }
    if let Some(hubs) = hubs {
        for hub in hubs {
            if let Some(items) = hub.get("Metadata").and_then(Value::as_array) {
                wire_items += items.len();
                guid_items += items
                    .iter()
                    .filter(|m| m.get("guid").and_then(Value::as_str).is_some())
                    .count();
            }
        }
    }
    format!(
        "discovery-home: response auth_missing=false http={status} bytes={} shape={shape} wire_hubs={} wire_items={wire_items} guid_items={guid_items} parsed={} rows={} items={} stale={stale}",
        body.len(), hubs.map_or(0, Vec::len), parsed.is_some(),
        parsed.map_or(0, <[DiscoveryHub]>::len),
        parsed.map_or(0, |rows| rows.iter().map(|h| h.items.len()).sum::<usize>()),
    )
}

fn discovery_home_index_diagnostics(body: &[u8]) -> Vec<String> {
    use serde_json::Value;
    if body.len() > HOME_BYTES {
        return Vec::new();
    }
    let Ok(wire) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let Some(hubs) = wire
        .get("MediaContainer")
        .and_then(|c| c.get("Hub"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let label = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 192
                    && s.bytes().all(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_' | b':')
                    })
            })
            .unwrap_or("unavailable")
            .to_owned()
    };
    hubs.iter()
        .enumerate()
        .filter_map(|(index, hub)| {
            let key = hub.get("key")?.as_str().filter(|key| safe_home_key(key))?;
            Some(format!(
                "discovery-home: index slot={index} hubIdentifier={} type={} key={key}",
                label(hub.get("hubIdentifier")),
                label(hub.get("type"))
            ))
        })
        .take(24)
        .collect()
}

fn discovery_home_container_diagnostic(body: &[u8]) -> String {
    use serde_json::Value;
    let wire = (body.len() <= HOME_BYTES)
        .then(|| serde_json::from_slice::<Value>(body).ok())
        .flatten();
    let container = wire.as_ref().and_then(|v| v.get("MediaContainer"));
    let shape = |value: Option<&Value>| match value {
        None => "absent",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
        Some(Value::Null) => "null",
        Some(_) => "scalar",
    };
    let top_metadata = wire.as_ref().and_then(|v| v.get("Metadata"));
    let metadata = container.and_then(|c| c.get("Metadata"));
    let count = |field: &str| {
        container
            .and_then(|c| c.get(field))
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().filter(|s| s.len() <= 20)?.parse::<u64>().ok())
            })
            .map_or_else(|| "unknown".to_owned(), |n| n.to_string())
    };
    // Only fixed schema field names and counts, never arbitrary keys or their string values.
    let fields = [
        "size",
        "totalSize",
        "offset",
        "Metadata",
        "Hub",
        "Directory",
        "Video",
        "SearchResults",
    ]
    .into_iter()
    .filter(|field| container.is_some_and(|c| c.get(field).is_some()))
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "discovery-home: container shape={} fields=[{fields}] size={} totalSize={} offset={} Metadata={} metadata_items={} top_Metadata={} top_metadata_items={}",
        shape(container), count("size"), count("totalSize"), count("offset"), shape(metadata),
        metadata.and_then(Value::as_array).map_or(0, Vec::len), shape(top_metadata),
        top_metadata.and_then(Value::as_array).map_or(0, Vec::len),
    )
}

/// Index plus at most 32 advertised hub GETs, 2 MiB each and one 20-second deadline.
/// One active-profile credential, fixed provider, no redirects, retries or recursive hydration.
pub(crate) fn discovery_at(generation:u32,key:Option<&str>)->Option<Vec<DiscoveryHub>>{
    let url=match key {Some(key) if safe_home_key(key)=>format!("{DISCOVER}{key}?count={HOME_ITEMS}&X-Plex-Container-Size={HOME_ITEMS}"),Some(_)=>return None,None=>HOME_INDEX.into()};
    if generation != super::session::current_gen() {
        crate::log("discovery-home: cancelled stage=before-auth stale=true");
        return None;
    }
    let Some(account) = super::catalog::active_client() else {
        crate::log("discovery-home: skipped auth_missing=true");
        return None;
    };
    let headers = account.headers();
    let start = std::time::Instant::now();
    let rows = discovery_home_with(&url,|url| {
        if generation != super::session::current_gen() {
            crate::log("discovery-home: cancelled stage=before-request stale=true");
            return None;
        }
        let Some(remaining) = home_timeout(start.elapsed()) else {
            crate::log("discovery-home: deadline-expired");
            return None;
        };
        crate::log("discovery-home: request auth_missing=false");
        if let Some(key) = url
            .strip_prefix(DISCOVER)
            .and_then(|path| path.split('?').next())
            .filter(|key| safe_home_key(key))
        {
            crate::log(&format!("discovery-home: hub-request key={key}"));
        }
        let Some(response) = crate::net::request(
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
            Some(HOME_BYTES),
            None,
        ) else {
            crate::log("discovery-home: transport-failed auth_missing=false http=none bytes=0");
            return None;
        };
        let stale = generation != super::session::current_gen();
        let parsed = (response.status == 200 && !stale)
            .then(|| parse_discovery_home(&response.body))
            .flatten();
        crate::log(&discovery_home_diagnostic(
            response.status,
            &response.body,
            parsed.as_deref(),
            stale,
        ));
        crate::log(&discovery_home_container_diagnostic(&response.body));
        if url == HOME_INDEX && response.status == 200 && !stale {
            for line in discovery_home_index_diagnostics(&response.body) {
                crate::log(&line);
            }
        }
        (response.status == 200 && !stale).then_some(response.body)
    })?;
    (generation == super::session::current_gen()).then_some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn provider_order_and_more_than_twelve_items_are_preserved(){
        let items=(0..40).map(|i|json!({"type":"movie","ratingKey":format!("{i:024x}"),"guid":format!("plex://movie/{i:024x}"),"title":format!("Title {i}")})).collect::<Vec<_>>();
        let wire=json!({"MediaContainer":{"Hub":[{"title":"New live seasonal shelf","Metadata":items},{"title":"Most Watchlisted","Metadata":[{"type":"show","ratingKey":"0123456789abcdef01234567","guid":"plex://show/0123456789abcdef01234567","title":"Show"}]}]}});
        let rows=parse_discovery_home(&serde_json::to_vec(&wire).unwrap()).unwrap();
        assert_eq!(rows[0].title,"New live seasonal shelf");assert_eq!(rows[0].items.len(),40);assert_eq!(rows.len(),2);
    }
    #[test]
    fn dynamic_index_hydrates_directories_and_trailer_parent_identity(){
        let rows=discovery_home_with(HOME_INDEX,|url|Some(serde_json::to_vec(&if url==HOME_INDEX {
            json!({"MediaContainer":{"Hub":[{"type":"directory","title":"Browse by Genre","key":"/hubs/sections/home/genres"},{"type":"clip","title":"New trailers","key":"/hubs/sections/home/new-trailers"}]}})
        }else if url.contains("/genres?") {
            json!({"MediaContainer":{"Directory":[{"title":"Action","key":"/library/categories/action"},{"title":"Bad","key":"https://evil.test"}]}})
        }else {
            json!({"MediaContainer":{"Metadata":[{"type":"clip","title":"Title","ratingKey":"clip","primaryGuid":"plex://movie/0123456789abcdef01234567"}]}})
        }).unwrap())).unwrap();
        assert_eq!(rows[0].directories.len(),1);assert_eq!(rows[1].items[0].rating_key,"0123456789abcdef01234567");
    }
    #[test]
    fn provider_keys_cannot_escape_discovery_origin_or_read_arbitrary_routes(){
        for key in ["https://evil.test/x","//evil.test/x","/library/metadata/42","/library/categories/../x","/library/categories/x?token=a","/library/categories/%2e%2e/x"]{assert!(!safe_home_key(key),"{key}");}
        for key in ["/library/categories/action/trending-action","/library/awards/academy-awards/award_winners","/library/decades/2020"]{assert!(safe_home_key(key));}
    }
    #[test]
    fn hydration_is_bounded_and_retains_success_before_a_failed_leaf(){
        let mut calls=0;
        let rows=discovery_home_with(HOME_INDEX,|url|{
            if url==HOME_INDEX{return Some(serde_json::to_vec(&json!({"MediaContainer":{"Hub":(0..40).map(|i|json!({"title":format!("Feed {i}"),"type":"movie","key":format!("/hubs/sections/home/feed-{i}")})).collect::<Vec<_>>()}})).unwrap());}
            calls+=1;
            if calls>1{return None;}
            Some(serde_json::to_vec(&json!({"MediaContainer":{"Metadata":(0..75).map(|i|json!({"title":format!("Film {i}"),"type":"movie","ratingKey":format!("{i:024x}"),"guid":format!("plex://movie/{i:024x}")})).collect::<Vec<_>>()}})).unwrap())
        }).unwrap();
        assert_eq!(calls,32);assert_eq!(rows.len(),1);assert_eq!(rows[0].items.len(),60);
    }
}
