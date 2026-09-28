//! Profile- and server-scoped original downloads. Workers capture their authority at the click.
use plxnative_deck_core::downloads::{Identity, Store};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
pub(crate) use plxnative_deck_core::downloads::{Entry, State};

#[derive(Clone)]
pub(crate) enum Command { Play(Identity), Resume(Identity), Pause(Identity), Delete(Identity) }
static ACTIVE: Mutex<Option<(Identity, Arc<AtomicBool>)>> = Mutex::new(None);
static PLAY: Mutex<Option<Entry>> = Mutex::new(None);
static MESSAGE: Mutex<String> = Mutex::new(String::new());

pub(crate) fn profile() -> String {
    let session = crate::plex::session::peek();
    #[cfg(feature = "devtriggers")]
    if let Some(token) = crate::dev::read("token") {
        if token == "synthetic-token" { return plxnative_deck_core::downloads::profile_scope(&token, "fixture"); }
    }
    let mut profile = crate::plex::session::current_profile_key();
    if profile.is_empty() { profile = session.user.uuid.clone(); }
    plxnative_deck_core::downloads::profile_scope(&session.account_token, &profile)
}

pub(crate) fn snapshot() -> (Vec<Entry>, String) {
    let mut entries = store().and_then(|s| s.list(&profile())).unwrap_or_default();
    entries.sort_by_key(|e| e.identity.id());
    (entries, MESSAGE.lock().unwrap().clone())
}

pub(crate) fn take_play() -> Option<Entry> { PLAY.lock().unwrap().take().filter(|e| e.identity.profile == profile()) }

pub(crate) fn submit(command: Command) {
    let id = match &command { Command::Play(id) | Command::Resume(id) | Command::Pause(id) | Command::Delete(id) => id };
    if id.profile != profile() { return; }
    match command {
        Command::Play(id) => {
            if let Ok(s) = store() { if s.playable(&id).ok().flatten().is_some() { *PLAY.lock().unwrap() = s.get(&id).ok().flatten(); } }
        }
        Command::Pause(id) => {
            if let Some((active, cancel)) = &*ACTIVE.lock().unwrap() { if *active == id { cancel.store(true, Ordering::Relaxed); } }
        }
        Command::Delete(id) => {
            if ACTIVE.lock().unwrap().as_ref().is_some_and(|(active, _)| *active == id) {
                *MESSAGE.lock().unwrap() = "Pause the download before deleting it".into(); return;
            }
            if store().and_then(|s| s.remove(&id)).is_err() { *MESSAGE.lock().unwrap() = "Could not remove download".into(); }
        }
        Command::Resume(id) => {
            let sid = crate::plex::server_ids().find(|sid| crate::plex::client_for(*sid).is_some_and(|c| c.machine_id() == id.server));
            if let Some(sid) = sid { start(sid, id.rating_key, id.part); }
            else { *MESSAGE.lock().unwrap() = "Reconnect to this item's Plex server to resume".into(); }
        }
    }
}

pub(crate) fn start(sid: crate::plex::ServerId, rk: String, part: String) {
    let Some(client) = crate::plex::client_for(sid) else { return; };
    let id = Identity { profile: profile(), server: client.machine_id().into(), rating_key: rk, part };
    if id.profile.is_empty() { *MESSAGE.lock().unwrap() = "Sign in before downloading".into(); return; }
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut active = ACTIVE.lock().unwrap();
        if active.is_some() { *MESSAGE.lock().unwrap() = "A download is already running; pause it first".into(); return; }
        *active = Some((id.clone(), cancel.clone()));
    }
    *MESSAGE.lock().unwrap() = "Preparing original download…".into();
    if crate::task::spawn("deck download", move || {
        let result = (|| -> Result<(), &'static str> {
            let metadata = client.metadata(&id.rating_key).ok_or("Could not read metadata; reconnect and retry")?;
            let media = metadata.media.iter().find(|m| id.part.is_empty() || m.part.iter().any(|p| p.key == id.part))
                .ok_or("This version is no longer available")?;
            if media.part.len() != 1 { return Err("Multi-part downloads are not supported"); }
            let part = &media.part[0];
            let mut id = id.clone();
            id.part = part.key.clone();
            if id.server.is_empty() { id.server = client.machine_identity().ok_or("The server did not identify itself")?; }
            *ACTIVE.lock().unwrap() = Some((id.clone(), cancel.clone()));
            if part.size <= 0 { return Err("The server did not provide the original file size"); }
            let request = plxnative_deck_core::downloads::Request { identity: id.clone(), title: metadata.title,
                expected_bytes: part.size as u64, url: client.direct_play_url(&part.key, "").to_url().parse().map_err(|_| "Invalid media address")? };
            let store = store().map_err(|_| "Download storage is unavailable")?;
            plxnative_deck_core::downloads::download_blocking(&store, request, &cancel)
                .map_err(|_| "Download interrupted; select Resume to retry")?;
            Ok(())
        })();
        *MESSAGE.lock().unwrap() = if cancel.load(Ordering::Relaxed) { "Download paused".into() }
            else { result.map_or_else(str::to_string, |_| "Download complete".into()) };
        crate::log(&format!("download: {}", MESSAGE.lock().unwrap()));
        *ACTIVE.lock().unwrap() = None;
    }).is_none() { *ACTIVE.lock().unwrap() = None; *MESSAGE.lock().unwrap() = "Could not start download".into(); }
}

pub(crate) fn store() -> std::io::Result<Store> {
    let root = std::env::var_os("PLXNATIVE_DOWNLOAD_DIR").map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::paths::in_runtime_dir("downloads"));
    Store::open(root)
}

pub(crate) fn local_source(client: &crate::plex::Client, rk: &str, part: &str) -> Option<String> {
    let identity = Identity { profile: profile(), server: client.machine_id().into(), rating_key: rk.into(), part: part.into() };
    store().ok()?.playable(&identity).ok()??.to_str().map(str::to_string)
}
