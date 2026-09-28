//! Original-file downloads. No transcoding and no credentials in persisted records.
//!
//! A caller supplies a freshly authorised URL for every run. A partial file is resumed only
//! with a strong ETag and an exact Content-Range. The immutable part identity is scoped to
//! both the Plex profile and the server. Playback uses only verified, completed records.
use reqwest::{header, Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MANIFEST_VERSION: u32 = 1;
const MIN_FREE: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub profile: String,
    pub server: String,
    pub rating_key: String,
    pub part: String,
}

impl Identity {
    pub fn id(&self) -> String {
        let mut hash = Sha256::new();
        for value in [&self.profile, &self.server, &self.rating_key, &self.part] {
            hash.update((value.len() as u64).to_be_bytes());
            hash.update(value.as_bytes());
        }
        format!("{:x}", hash.finalize())
    }
    fn valid(&self) -> bool {
        !self.profile.is_empty() && !self.server.is_empty() && !self.rating_key.is_empty()
            && self.part.starts_with("/library/parts/") && !self.part.contains('?')
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State { Paused, Downloading, Complete, Failed }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub schema: u32,
    pub identity: Identity,
    pub title: String,
    pub expected_bytes: u64,
    pub received_bytes: u64,
    pub state: State,
    pub etag: Option<String>,
    pub error: Option<String>,
}

/// Deliberately has no Debug or Serialize implementation: the URL can contain a credential.
pub struct Request {
    pub identity: Identity,
    pub title: String,
    pub expected_bytes: u64,
    pub url: Url,
}

#[derive(Clone)]
pub struct Store { root: PathBuf }

impl Store {
    pub fn open(root: impl AsRef<Path>) -> io::Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.as_ref(), fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self { root: fs::canonicalize(root)? })
    }

    fn path(&self, identity: &Identity, suffix: &str) -> PathBuf {
        self.root.join(format!("{}.{}", identity.id(), suffix))
    }

    pub fn get(&self, identity: &Identity) -> io::Result<Option<Entry>> {
        let path = self.path(identity, "json");
        let bytes = match fs::read(path) {
            Ok(v) => v,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let entry: Entry = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if entry.schema != MANIFEST_VERSION || entry.identity != *identity {
            return Err(io::Error::other("Invalid download manifest"));
        }
        Ok(Some(entry))
    }

    pub fn list(&self, profile: &str) -> io::Result<Vec<Entry>> {
        let mut entries = Vec::new();
        for file in fs::read_dir(&self.root)? {
            let file = file?;
            if file.path().extension().and_then(|s| s.to_str()) != Some("json") { continue; }
            let Ok(bytes) = fs::read(file.path()) else { continue };
            let Ok(mut entry) = serde_json::from_slice::<Entry>(&bytes) else { continue };
            if entry.schema != MANIFEST_VERSION || entry.identity.profile != profile
                || file.path() != self.path(&entry.identity, "json") { continue; }
            // A process can die while its last saved record says Downloading. No lease means
            // it is resumable, never a permanently running job. The worker owns a file lock.
            if entry.state == State::Downloading && self.try_lock(&entry.identity).is_ok() {
                entry.state = State::Paused;
            }
            entries.push(entry);
        }
        entries.sort_by(|a, b| a.title.cmp(&b.title).then(a.identity.id().cmp(&b.identity.id())));
        Ok(entries)
    }

    pub fn playable(&self, identity: &Identity) -> io::Result<Option<PathBuf>> {
        let Some(entry) = self.get(identity)? else { return Ok(None) };
        if entry.state != State::Complete || entry.received_bytes != entry.expected_bytes {
            return Ok(None);
        }
        let path = self.path(identity, "media");
        Ok(fs::symlink_metadata(&path).ok()
            .filter(|m| m.is_file() && m.len() == entry.expected_bytes).map(|_| path))
    }

    pub fn remove(&self, identity: &Identity) -> io::Result<()> {
        let _lock = self.try_lock(identity)?;
        for suffix in ["json", "part", "media", "json.tmp"] {
            match fs::remove_file(self.path(identity, suffix)) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                Err(e) => return Err(e),
            }
        }
        self.sync_dir()
    }

    fn save(&self, entry: &Entry) -> io::Result<()> {
        let tmp = self.path(&entry.identity, "json.tmp");
        let mut file = private_file(&tmp, true)?;
        serde_json::to_writer(&mut file, entry).map_err(io::Error::other)?;
        file.sync_all()?;
        fs::rename(tmp, self.path(&entry.identity, "json"))?;
        self.sync_dir()
    }

    fn sync_dir(&self) -> io::Result<()> {
        #[cfg(unix)] { File::open(&self.root)?.sync_all()?; }
        Ok(())
    }

    fn try_lock(&self, identity: &Identity) -> io::Result<File> {
        let file = private_file(&self.path(identity, "lock"), false)?;
        file.try_lock().map_err(|_| io::Error::other("Download is already in use"))?;
        Ok(file)
    }

    /// Run on a worker runtime, never the UI thread. Cancellation preserves a resumable partial.
    /// `available_bytes` is supplied by the platform storage adapter and is rechecked per chunk.
    pub async fn download(
        &self, request: Request, cancel: &AtomicBool,
        available_bytes: impl Fn() -> io::Result<u64>,
    ) -> io::Result<Entry> {
        if !request.identity.valid() || request.expected_bytes == 0
            || !matches!(request.url.scheme(), "http" | "https")
            || request.url.host_str().is_none() {
            return Err(io::Error::other("Invalid original-file download request"));
        }
        let _lock = self.try_lock(&request.identity)?;
        if let Some(entry) = self.get(&request.identity)? {
            if entry.expected_bytes == request.expected_bytes
                && self.playable(&request.identity)?.is_some() { return Ok(entry); }
        }
        let mut entry = self.get(&request.identity)?.filter(|e|
            e.expected_bytes == request.expected_bytes).unwrap_or(Entry {
                schema: MANIFEST_VERSION, identity: request.identity.clone(),
                title: request.title.clone(), expected_bytes: request.expected_bytes,
                received_bytes: 0, state: State::Paused, etag: None, error: None,
            });
        entry.state = State::Downloading;
        entry.error = None;
        self.save(&entry)?;
        let result = self.transfer(&request, &mut entry, cancel, &available_bytes).await;
        if result.is_err() {
            entry.state = State::Failed;
            // The transport's error can carry its complete credential-bearing URL.
            entry.error = Some("Download interrupted; retry when the server and storage are available".into());
        }
        self.save(&entry)?;
        result.map(|()| entry)
    }

    async fn transfer(&self, request: &Request, entry: &mut Entry, cancel: &AtomicBool,
        available_bytes: &impl Fn() -> io::Result<u64>) -> io::Result<()> {
        let path = self.path(&request.identity, "part");
        let mut file = private_file(&path, false)?;
        let size = file.metadata()?.len();
        let mut offset = if size < request.expected_bytes && entry.etag.as_ref()
            .is_some_and(|v| strong_etag(v)) { size } else { 0 };
        if offset == 0 { file.set_len(0)?; }
        if available_bytes()? < request.expected_bytes.saturating_sub(offset).saturating_add(MIN_FREE) {
            return Err(io::Error::other("Not enough storage space"));
        }
        if cancel.load(Ordering::Relaxed) {
            entry.state = State::Paused; entry.received_bytes = offset; return Ok(());
        }
        let client = Client::builder().redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10)).read_timeout(Duration::from_secs(20))
            .build().map_err(|_| io::Error::other("Cannot initialise download transport"))?;
        let mut get = client.get(request.url.clone()).header(header::ACCEPT_ENCODING, "identity");
        if offset > 0 {
            get = get.header(header::RANGE, format!("bytes={offset}-"))
                .header(header::IF_RANGE, entry.etag.as_deref().unwrap());
        }
        let mut response = tokio::select! {
            response = get.send() => response.map_err(|_| io::Error::other("Cannot reach media server"))?,
            _ = cancelled(cancel) => { entry.state = State::Paused; return Ok(()) }
        };
        if response.headers().get(header::CONTENT_ENCODING).is_some_and(|v| v != "identity") {
            return Err(io::Error::other("Unexpected content encoding"));
        }
        let etag = response.headers().get(header::ETAG).and_then(|v| v.to_str().ok())
            .filter(|v| strong_etag(v)).map(str::to_owned);
        if response.status() == StatusCode::OK {
            // A server ignoring Range, or an If-Range validator mismatch, starts a fresh file.
            offset = 0; file.set_len(0)?;
        } else if response.status() == StatusCode::PARTIAL_CONTENT {
            let range = response.headers().get(header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok()).and_then(parse_range);
            if range != Some((offset, request.expected_bytes - 1, request.expected_bytes))
                || (offset > 0 && etag != entry.etag) {
                return Err(io::Error::other("Server returned an inconsistent byte range"));
            }
        } else {
            // In particular: do not forward credential-bearing requests through redirects.
            return Err(io::Error::other("Server refused the original media download"));
        }
        if response.content_length().is_some_and(|n| n != request.expected_bytes - offset) {
            return Err(io::Error::other("Media length changed; refresh its metadata"));
        }
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(offset))?;
        entry.etag = etag;
        entry.received_bytes = offset;
        self.save(entry)?;
        let mut saved = Instant::now();
        loop {
            let chunk = tokio::select! {
                chunk = response.chunk() => chunk.map_err(|_| io::Error::other("Transfer interrupted"))?,
                _ = cancelled(cancel) => {
                    file.sync_all()?; entry.state = State::Paused; return Ok(())
                }
            };
            let Some(chunk) = chunk else { break };
            if chunk.len() as u64 > request.expected_bytes.saturating_sub(entry.received_bytes) {
                return Err(io::Error::other("Media exceeds its declared length"));
            }
            if available_bytes()? < (chunk.len() as u64).saturating_add(MIN_FREE) {
                return Err(io::Error::other("Storage filled during download"));
            }
            file.write_all(&chunk)?;
            entry.received_bytes += chunk.len() as u64;
            if saved.elapsed() > Duration::from_secs(1) { self.save(entry)?; saved = Instant::now(); }
        }
        if entry.received_bytes != request.expected_bytes {
            return Err(io::Error::other("Incomplete media file"));
        }
        file.sync_all()?;
        drop(file);
        fs::rename(path, self.path(&entry.identity, "media"))?;
        self.sync_dir()?;
        entry.state = State::Complete;
        Ok(())
    }
}

async fn cancelled(cancel: &AtomicBool) {
    while !cancel.load(Ordering::Relaxed) { tokio::time::sleep(Duration::from_millis(50)).await; }
}

fn strong_etag(value: &str) -> bool {
    value.len() >= 2 && value.starts_with('"') && value.ends_with('"')
        && !value.contains(['\r', '\n'])
}

fn parse_range(value: &str) -> Option<(u64, u64, u64)> {
    let (span, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (first, last) = span.split_once('-')?;
    let (first, last, total) = (first.parse().ok()?, last.parse().ok()?, total.parse().ok()?);
    (first <= last && last < total).then_some((first, last, total))
}

fn private_file(path: &Path, truncate: bool) -> io::Result<File> {
    // Refuse links even inside our private root. Files are never named from a media title.
    if fs::symlink_metadata(path).is_ok_and(|m| !m.is_file()) {
        return Err(io::Error::other("Unexpected file type in download storage"));
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(truncate);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

/// Runs on a download worker, never on the UI thread.
pub fn download_blocking(store: &Store, request: Request, cancel: &AtomicBool) -> io::Result<Entry> {
    use std::os::unix::ffi::OsStrExt;
    let root = std::ffi::CString::new(store.root.as_os_str().as_bytes()).map_err(io::Error::other)?;
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(
        store.download(request, cancel, || {
            let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
            // SAFETY: NUL-terminated path, writable statvfs; initialized only on success.
            if unsafe { libc::statvfs(root.as_ptr(), stat.as_mut_ptr()) } != 0 { return Err(io::Error::last_os_error()); }
            let stat = unsafe { stat.assume_init() };
            Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
        })
    )
}

pub fn profile_scope(account: &str, profile: &str) -> String {
    if account.is_empty() { return String::new(); }
    let mut hash = Sha256::new();
    hash.update((account.len() as u64).to_be_bytes()); hash.update(account.as_bytes());
    hash.update(profile.as_bytes());
    format!("{:x}", hash.finalize())
}
