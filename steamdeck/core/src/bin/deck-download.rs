//! Device smoke-test entry point. The application will call the same Store API.
//! A request arrives on stdin so signed media URLs are absent from process arguments.
use plxnative_deck_core::downloads::{Identity, Request, Store};
use serde::Deserialize;
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[derive(Deserialize)]
struct Input { identity: Identity, title: String, expected_bytes: u64, url: String }

fn available(path: &Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("Invalid storage path"))?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path is NUL-terminated, stat points to a complete writable statvfs object.
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let stat = unsafe { stat.assume_init() };
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os().nth(1).ok_or("Pass an absolute download directory")?;
    let root = Path::new(&root);
    if !root.is_absolute() { return Err("Download directory must be absolute".into()); }
    let mut json = String::new();
    io::stdin().take(64 * 1024).read_to_string(&mut json)?;
    let input: Input = serde_json::from_str(&json)?;
    let store = Store::open(root)?;
    let request = Request { identity: input.identity, title: input.title,
        expected_bytes: input.expected_bytes, url: input.url.parse()? };
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let entry = runtime.block_on(store.download(request, &AtomicBool::new(false), || available(root)))?;
    println!("{}", serde_json::to_string(&entry)?);
    if let Some(path) = store.playable(&entry.identity)? { println!("{}", path.display()); }
    Ok(())
}

fn main() {
    if run().is_err() {
        // Parsing and HTTP errors may carry input fields. The manifest holds a safe error.
        eprintln!("Download failed; check request, connection and available storage");
        std::process::exit(1);
    }
}
