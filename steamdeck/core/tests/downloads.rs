use plxnative_deck_core::downloads::{Identity, Request, State, Store};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;

#[test]
fn account_and_profile_scopes_are_separate_and_do_not_store_credentials() {
    use plxnative_deck_core::downloads::profile_scope;
    let owner = profile_scope("account-secret", "");
    assert_eq!(owner.len(), 64);
    assert!(!owner.contains("secret"));
    assert_ne!(owner, profile_scope("another-account", ""));
    assert_ne!(owner, profile_scope("account-secret", "child"));
    assert_eq!(profile_scope("", "child"), "");
}

fn identity(profile: &str) -> Identity {
    Identity { profile: profile.into(), server: "fixture-server".into(), rating_key: "1".into(),
        part: "/library/parts/1/1/test.mkv".into() }
}
fn serve(reply: &'static [u8]) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/library/parts/1/1/test.mkv?X-Plex-Token=TEST_SECRET", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).unwrap(); request.push(byte[0]);
        }
        socket.write_all(reply).unwrap();
        String::from_utf8(request).unwrap()
    });
    (url, thread)
}
fn request(url: &str) -> Request {
    Request { identity: identity("one"), title: "Fixture".into(), expected_bytes: 8, url: url.parse().unwrap() }
}
fn run(store: &Store, url: &str) -> std::io::Result<plxnative_deck_core::downloads::Entry> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
        .block_on(store.download(request(url), &AtomicBool::new(false), || Ok(u64::MAX)))
}

#[test]
fn original_bytes_are_atomic_and_profile_scoped() {
    let tmp = tempfile::tempdir().unwrap(); let store = Store::open(tmp.path()).unwrap();
    let (url, server) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nETag: \"v1\"\r\n\r\nHDRBYTES");
    let entry = run(&store, &url).unwrap(); server.join().unwrap();
    assert_eq!(entry.state, State::Complete);
    assert_eq!(std::fs::read(store.playable(&identity("one")).unwrap().unwrap()).unwrap(), b"HDRBYTES");
    assert!(store.list("two").unwrap().is_empty());
    assert!(store.playable(&identity("two")).unwrap().is_none());
    let manifest = std::fs::read_to_string(tmp.path().join(format!("{}.json", identity("one").id()))).unwrap();
    assert!(!manifest.contains("TEST_SECRET")); assert!(!manifest.contains("http://"));
    store.remove(&identity("one")).unwrap();
    assert!(store.playable(&identity("one")).unwrap().is_none());
}

#[test]
fn interrupted_file_resumes_only_the_matching_range() {
    let tmp = tempfile::tempdir().unwrap(); let store = Store::open(tmp.path()).unwrap();
    let (url, first) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nETag: \"v1\"\r\n\r\nHDR");
    assert!(run(&store, &url).is_err()); first.join().unwrap();
    assert!(store.playable(&identity("one")).unwrap().is_none());
    let (url, second) = serve(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 3-7/8\r\nETag: \"v1\"\r\n\r\nBYTES");
    assert_eq!(run(&store, &url).unwrap().state, State::Complete);
    let sent = second.join().unwrap().to_lowercase();
    assert!(sent.contains("range: bytes=3-")); assert!(sent.contains("if-range: \"v1\""));
    assert_eq!(std::fs::read(store.playable(&identity("one")).unwrap().unwrap()).unwrap(), b"HDRBYTES");
}

#[test]
fn ignored_range_replaces_partial_instead_of_appending() {
    let tmp = tempfile::tempdir().unwrap(); let store = Store::open(tmp.path()).unwrap();
    let (url, first) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nETag: \"v1\"\r\n\r\nOLD");
    assert!(run(&store, &url).is_err()); first.join().unwrap();
    let (url, second) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nETag: \"v2\"\r\n\r\nNEWBYTES");
    assert!(run(&store, &url).is_ok()); second.join().unwrap();
    assert_eq!(std::fs::read(store.playable(&identity("one")).unwrap().unwrap()).unwrap(), b"NEWBYTES");
}

#[test]
fn invalid_range_never_becomes_playable() {
    let tmp = tempfile::tempdir().unwrap(); let store = Store::open(tmp.path()).unwrap();
    let (url, server) = serve(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 8\r\nContent-Range: bytes 1-8/9\r\n\r\nBADBYTES");
    assert!(run(&store, &url).is_err()); server.join().unwrap();
    assert!(store.playable(&identity("one")).unwrap().is_none());
}

#[test]
fn redirect_is_refused_without_leaking_credentials() {
    let tmp = tempfile::tempdir().unwrap(); let store = Store::open(tmp.path()).unwrap();
    let (url, server) = serve(b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\nLocation: http://127.0.0.1:1/steal\r\n\r\n");
    let error = run(&store, &url).unwrap_err(); server.join().unwrap();
    assert!(!error.to_string().contains("TEST_SECRET"));
    assert!(store.playable(&identity("one")).unwrap().is_none());
}

#[test]
fn low_storage_and_cancellation_never_publish_complete() {
    let tmp = tempfile::tempdir().unwrap(); let store = Store::open(tmp.path()).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let url = "http://127.0.0.1:1/library/parts/1/1/test.mkv";
    assert!(rt.block_on(store.download(request(url), &AtomicBool::new(false), || Ok(0))).is_err());
    let entry = rt.block_on(store.download(request(url), &AtomicBool::new(true), || Ok(u64::MAX))).unwrap();
    assert_eq!(entry.state, State::Paused);
    assert!(store.playable(&identity("one")).unwrap().is_none());
}
