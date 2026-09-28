//! Steam Deck adapter: the existing native HUD controls a private HDR video process.
use super::{adapter::PlayerAdapter, PlaybackState, SHARED, TX};
use std::sync::atomic::Ordering::Relaxed;

pub(crate) fn start(ps: &crate::route::PlaybackSession, pa: &mut PlayerAdapter) -> bool {
    let resume = TX.seek_to_ns.swap(-1, Relaxed).max(0) as f64 / 1e9;
    stop(pa);
    let source = crate::route::url(ps).to_string();
    if source.is_empty() { return false; }
    // HUD transfers live in tmpfs; continuous redraws must not write to the SSD.
    let root = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir).join(format!("plxnative-video-{}", std::process::id()));
    SHARED.reset_session();
    super::reset_audio_track();
    super::reset_subtitle();
    match plxnative_deck_core::playback::Session::launch(&root, source, resume) {
        Ok(session) => {
            pa.deck = Some(session);
            TX.started.store(true, Relaxed);
            SHARED.pb_state.store(PlaybackState::Connecting as u8, Relaxed);
            true
        }
        Err(_) => { SHARED.pb_state.store(PlaybackState::Error as u8, Relaxed); false }
    }
}

pub(crate) fn stop(pa: &mut PlayerAdapter) {
    pa.deck = None;
    TX.started.store(false, Relaxed);
    TX.seek_to_ns.store(-1, Relaxed);
    TX.commit_paused(false);
    SHARED.seen_frame.store(false, Relaxed);
    SHARED.ended.store(false, Relaxed);
    SHARED.seeking.store(false, Relaxed);
    SHARED.pb_state.store(PlaybackState::Idle as u8, Relaxed);
}

pub(crate) fn pump(pa: &mut PlayerAdapter) {
    let Some(session) = pa.deck.as_mut() else { return; };
    session.tick();
    session.select_tracks(SHARED.desired_audio_idx.load(Relaxed), SHARED.desired_sub_idx.load(Relaxed));
    let seek = TX.seek_to_ns.load(Relaxed);
    if seek >= 0 && session.ready && session.command(serde_json::json!(["seek", seek as f64 / 1e9, "absolute+exact"])) {
        TX.seek_to_ns.store(-1, Relaxed);
        session.seeking = true;
    }
    SHARED.playpos_ns.store((session.position * 1e9) as i64, Relaxed);
    SHARED.duration_ns.store((session.duration * 1e9) as i64, Relaxed);
    SHARED.seen_frame.store(session.ready, Relaxed);
    SHARED.frames.store(i32::from(session.ready), Relaxed);
    SHARED.ended.store(session.ended, Relaxed);
    SHARED.seeking.store(session.seeking, Relaxed);
    TX.commit_paused(session.paused);
    let state = if session.failed { PlaybackState::Error } else if session.seeking { PlaybackState::Seeking } else if session.ready { PlaybackState::Playing } else { PlaybackState::Connecting };
    SHARED.pb_state.store(state as u8, Relaxed);
    #[cfg(feature = "devtriggers")]
    if crate::dev::flag("deck-status") {
        let status = serde_json::json!({"position": session.position, "duration": session.duration, "ready": session.ready, "paused": session.paused, "failed": session.failed, "hdr": session.hdr, "hardware": session.hardware, "overlays": session.overlays});
        let _ = std::fs::write(crate::paths::in_runtime_dir("deck-status.json"), status.to_string());
    }
}

/// Read back only the native UI plane. mpv's premultiplied BGRA overlay is composited as SDR
/// graphics in gpu-next; HDR video never passes through this 8-bit framebuffer.
pub(crate) unsafe fn present(pa: &mut PlayerAdapter, x: i32, y: i32, w: i32, h: i32) {
    let Some(session) = pa.deck.as_mut().filter(|s| s.accepts_overlay()) else { return; };
    if w <= 0 || h <= 0 { return; }
    unsafe extern "C" { fn glReadPixels(x: i32, y: i32, w: i32, h: i32, format: u32, kind: u32, data: *mut std::ffi::c_void); }
    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    // SAFETY: current main-thread GL context; exactly w*h*4 writable bytes, RGBA unsigned byte.
    glReadPixels(x, y, w, h, 0x1908, 0x1401, rgba.as_mut_ptr().cast());
    let stride = w as usize * 4;
    let mut bgra = vec![0u8; rgba.len()];
    for row in 0..h as usize {
        for col in 0..w as usize {
            let src = (h as usize - row - 1) * stride + col * 4;
            let dst = row * stride + col * 4;
            bgra[dst..dst+4].copy_from_slice(&[rgba[src+2], rgba[src+1], rgba[src], rgba[src+3]]);
        }
    }
    session.overlay(x, y, w, h, &bgra);
}
