//! SteamOS entry point; the application core and controls are shared with PlxNative.
use std::{ffi::CString, path::PathBuf};
fn main() {
    let home = std::env::var_os("HOME").map(PathBuf::from).expect("HOME is required");
    let root = home.join(".local/share/plxnative-steamdeck");
    for (key, path) in [("PLXNATIVE_APP_DIR",root.join("assets")), ("PLXNATIVE_RUNTIME_DIR",root.join("state")), ("PLXNATIVE_DOWNLOAD_DIR",root.join("downloads"))] {
        if std::env::var_os(key).is_none() { std::env::set_var(key, path); }
    }
    if std::env::args().any(|s| s == "--downloads") { std::env::set_var("PLXNATIVE_START_DOWNLOADS", "1"); }
    let runtime = plxnative_modules::sim_runtime_dir();
    std::fs::create_dir_all(&runtime).expect("Cannot create app state directory");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).expect("Cannot protect app state directory");
    let _ = std::fs::write(plxnative_modules::sim_events_log(), "PlxNative Steam Deck: native UI, original downloads, Vulkan video\n");
    let host = CString::new("").unwrap();
    let rc = plxnative_modules::plex_run(host.as_ptr(), 32400);
    // The existing app owns detached curl workers; process shutdown must not race OpenSSL teardown.
    unsafe { libc::_exit(rc) }
}
