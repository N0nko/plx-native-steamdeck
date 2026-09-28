//! Steam Deck adapter: the existing native HUD controls a private HDR video process.
use super::{adapter::PlayerAdapter, PlaybackState, SHARED, TX};
use std::sync::atomic::Ordering::Relaxed;

pub(crate) fn start(ps: &crate::route::PlaybackSession, pa: &mut PlayerAdapter) -> bool {
    let resume = take_start_position();
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

fn take_start_position() -> f64 {
    // resume_at/arm_seek publishes the first-load offset here. TX is only the
    // live HUD seek queue. Consume before reset_session clears the startup state.
    SHARED.seek_to_ns.swap(-1, std::sync::atomic::Ordering::Acquire).max(0) as f64 / 1e9
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_consumes_the_resume_armed_for_the_first_load() {
        let _serial = crate::testlock::serial();
        SHARED.reset_session();
        TX.seek_to_ns.store(-1, Relaxed);
        super::super::engine::arm_seek(20_250_000_000);
        let resume = take_start_position();
        let consumed = SHARED.seek_to_ns.load(Relaxed);
        SHARED.reset_session();
        TX.seek_to_ns.store(-1, Relaxed);
        assert_eq!(resume, 20.25, "Resume must consume the startup seek, not the live seek queue");
        assert_eq!(consumed, -1, "A later Play from beginning must not reuse this offset");
        assert_eq!(take_start_position(), 0.);
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
    let Some(session) = pa.deck.as_mut() else { return; };
    if !session.accepts_overlay() {
        // Preserve the final fade/seek frame when IPC still owns the previous buffer.
        if session.ready && !session.failed { crate::ui::idle::invalidate(); }
        return;
    }
    if w <= 0 || h <= 0 { return; }
    unsafe extern "C" { fn glReadPixels(x: i32, y: i32, w: i32, h: i32, format: u32, kind: u32, data: *mut std::ffi::c_void); }
    let bgra = session.overlay_buffer(w, h);
    // SAFETY: current desktop GL context; exactly w*h*4 writable bytes. GL_BGRA is
    // supported by desktop OpenGL and avoids a per-pixel CPU channel conversion.
    glReadPixels(x, y, w, h, 0x80E1, 0x1401, bgra.as_mut_ptr().cast());
    let stride = w as usize * 4;
    for row in 0..h as usize / 2 {
        let (top, bottom) = bgra.split_at_mut((h as usize - row - 1) * stride);
        top[row * stride..(row + 1) * stride].swap_with_slice(&mut bottom[..stride]);
    }
    session.overlay(x, y, w, h, (crate::surface::LOGICAL_W as f64, crate::surface::LOGICAL_H as f64));
}
