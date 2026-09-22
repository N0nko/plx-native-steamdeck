//! Opt-in software audio. Decoder on the demux worker; PCM device on the app thread.
//! Native AAC/AC3/EAC3 stay unchanged. Stereo PCM only, not an immersive-audio renderer.
//! Runtime capability requires both the bundled decoder and a real SDL output device.
use std::ffi::{c_char, c_int, c_void, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

extern "C" {
    fn plx_audio_init(util: *const c_char, codec: *const c_char, resample: *const c_char) -> u32;
    fn plx_audio_decoder_open(parameters: *const c_void) -> *mut c_void;
    fn plx_audio_send_packet(
        decoder: *mut c_void,
        packet: *const c_void,
        pts: i64,
        duration: i64,
    ) -> c_int;
    fn plx_audio_send(decoder: *mut c_void, data: *const u8, bytes: c_int, pts: i64) -> c_int;
    fn plx_audio_receive(
        decoder: *mut c_void,
        data: *mut *const u8,
        bytes: *mut c_int,
        pts: *mut i64,
    ) -> c_int;
    fn plx_audio_decoder_flush(decoder: *mut c_void);
    fn plx_audio_decoder_close(decoder: *mut c_void);
    fn plx_pcm_open() -> *mut c_void;
    fn plx_pcm_close(sink: *mut c_void);
    fn plx_pcm_pause(sink: *mut c_void, paused: c_int);
    fn plx_pcm_flush(sink: *mut c_void);
    fn plx_pcm_queued(sink: *mut c_void) -> u32;
    fn plx_pcm_feed(
        sink: *mut c_void,
        data: *const u8,
        bytes: c_int,
        pts: i64,
        video: i64,
        prime: c_int,
    ) -> c_int;
}

static AVAILABLE: AtomicU32 = AtomicU32::new(0);
// Diagnostic publication only; never authorizes a codec or owns the output device.
static OUTPUT_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) fn output_active() -> bool {
    OUTPUT_ACTIVE.load(Ordering::Acquire)
}
const CODECS: &[(&str, u32)] = &[
    ("dts", 1),
    ("dca", 1),
    ("truehd", 2),
    ("flac", 4),
    ("opus", 8),
    ("vorbis", 16),
];

pub(crate) fn software_codec(codec: &str) -> bool {
    CODECS.iter().any(|(name, _)| *name == codec)
}
pub(crate) fn supported(codec: &str) -> bool {
    let mask = AVAILABLE.load(Ordering::Acquire);
    CODECS
        .iter()
        .any(|(name, bit)| *name == codec && mask & bit != 0)
}
pub(crate) fn advertised() -> String {
    CODECS
        .iter()
        .filter(|(name, _)| supported(name))
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(",")
}

/// Live-resource boot only, after SDL/FFmpeg and before playback. Never called by replay fixtures.
pub(crate) fn boot() {
    if !cfg!(feature = "local-audio") {
        return;
    }
    let names = if cfg!(target_os = "macos") {
        [
            "libavutil-plx.61.dylib",
            "libavcodec-plx.63.dylib",
            "libswresample-plx.7.dylib",
        ]
    } else {
        [
            "libavutil-plx.so.61",
            "libavcodec-plx.so.63",
            "libswresample-plx.so.7",
        ]
    };
    let paths =
        names.map(|name| CString::new(crate::paths::in_app_dir(name).to_string_lossy().as_bytes()));
    let [Ok(util), Ok(codec), Ok(swr)] = paths else {
        return;
    };
    let mask = unsafe { plx_audio_init(util.as_ptr(), codec.as_ptr(), swr.as_ptr()) };
    if mask == 0 {
        crate::log("local audio: bundled decoders unavailable; using server fallback");
        return;
    }
    let sink = unsafe { plx_pcm_open() };
    if sink.is_null() {
        crate::log("local audio: no usable stereo PCM output; using server fallback");
        return;
    }
    unsafe { plx_pcm_close(sink) };
    AVAILABLE.store(mask, Ordering::Release);
    crate::log(
        "local audio: experimental stereo PCM enabled; hardware latency and TV A/V sync unverified",
    );
}

/// Disallow the software route for subsequent resolves until the next application launch.
/// Existing playback fails visibly; Retry/Choose Quality can then request server conversion.
pub(crate) fn failed() {
    AVAILABLE.store(0, Ordering::Release);
    crate::player::SHARED
        .local_audio_failed
        .store(true, Ordering::Release);
    crate::player::log(
        "local audio: decoder/output failed; software capability disabled for this run",
    );
}

pub(crate) fn failed_for_queue(q: *mut crate::aq::AuQueue) {
    let live = crate::aq::aq_if_not_aborted(q, || {
        AVAILABLE.store(0, Ordering::Release);
        crate::player::SHARED
            .local_audio_failed
            .store(true, Ordering::Release);
    })
    .is_some();
    if live {
        crate::player::log("local audio: decoder failed; server fallback is available on Retry");
    }
}

pub(crate) struct Decoder(*mut c_void);
impl Decoder {
    /// `par` belongs to the same FFmpeg 9 instance; C copies it before this returns.
    pub(crate) unsafe fn open(par: *const c_void) -> Option<Self> {
        let p = plx_audio_decoder_open(par);
        (!p.is_null()).then_some(Self(p))
    }
    pub(crate) fn flush(&mut self) {
        unsafe { plx_audio_decoder_flush(self.0) }
    }
    fn receive(
        &mut self,
        q: *mut crate::aq::AuQueue,
        drain: &mut impl FnMut() -> bool,
    ) -> Result<i32, ()> {
        // One packet may produce multiple frames; bound the total work on malformed input.
        for _ in 0..256 {
            let mut data = std::ptr::null();
            let mut bytes = 0;
            let mut pts = 0;
            let result = unsafe { plx_audio_receive(self.0, &mut data, &mut bytes, &mut pts) };
            if result == 0 || result == 2 {
                return Ok(result);
            }
            if result != 1 || data.is_null() || bytes <= 0 || bytes > 65536 * 4 || bytes % 4 != 0 {
                return Err(());
            }
            // 20 ms pieces keep queue admission and clock correction bounded even for FLAC frames.
            let pcm = unsafe { std::slice::from_raw_parts(data, bytes as usize) };
            let mut frames = 0i64;
            for chunk in pcm.chunks(3840) {
                let stamp = pts.checked_add(frames * 1_000_000_000 / 48000).ok_or(())?;
                if crate::aq::aq_push_with_drain(
                    q,
                    chunk.as_ptr(),
                    chunk.len() as c_int,
                    stamp,
                    1,
                    2,
                    &mut *drain,
                ) != 0
                {
                    return Err(());
                }
                frames += (chunk.len() / 4) as i64;
                crate::player::SHARED.hls_audio_tail_ns.store(
                    pts.saturating_add(frames * 1_000_000_000 / 48000),
                    Ordering::Release,
                );
            }
        }
        Err(())
    }
    pub(crate) unsafe fn packet(
        &mut self,
        packet: *const c_void,
        pts: i64,
        duration: i64,
        q: *mut crate::aq::AuQueue,
        mut drain: impl FnMut() -> bool,
    ) -> Result<(), ()> {
        let mut r = plx_audio_send_packet(self.0, packet, pts, duration.max(0));
        if r == 0 {
            self.receive(q, &mut drain)?;
            r = plx_audio_send_packet(self.0, packet, pts, duration.max(0));
        }
        if r != 1 {
            return Err(());
        }
        self.receive(q, &mut drain).map(|_| ())
    }
    pub(crate) fn finish(
        &mut self,
        q: *mut crate::aq::AuQueue,
        mut drain: impl FnMut() -> bool,
    ) -> Result<(), ()> {
        let mut r = unsafe { plx_audio_send(self.0, std::ptr::null(), 0, 0) };
        if r == 0 {
            self.receive(q, &mut drain)?;
            r = unsafe { plx_audio_send(self.0, std::ptr::null(), 0, 0) };
        }
        if r != 1 {
            return Err(());
        }
        match self.receive(q, &mut drain)? {
            2 => Ok(()),
            _ => Err(()),
        }
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { plx_audio_decoder_close(self.0) }
    }
}

/// Engine-owned. Raw pointer deliberately makes this !Send/!Sync.
pub(crate) struct Pcm(*mut c_void);
impl Pcm {
    pub(crate) fn open(codec: &str) -> Option<Self> {
        if !supported(codec) {
            return None;
        }
        let p = unsafe { plx_pcm_open() };
        if p.is_null() {
            return None;
        }
        OUTPUT_ACTIVE.store(true, Ordering::Release);
        Some(Self(p))
    }
    pub(crate) fn pause(&mut self, paused: bool) {
        unsafe { plx_pcm_pause(self.0, paused as c_int) }
    }
    pub(crate) fn flush(&mut self) {
        unsafe { plx_pcm_flush(self.0) }
    }
    pub(crate) fn queued(&self) -> u32 {
        unsafe { plx_pcm_queued(self.0) }
    }
    pub(crate) unsafe fn feed(
        &mut self,
        data: *const u8,
        bytes: c_int,
        pts: i64,
        video: i64,
        prime: bool,
    ) -> i32 {
        if bytes <= 0 {
            return -1;
        }
        plx_pcm_feed(self.0, data, bytes, pts, video, prime as c_int)
    }
}
impl Drop for Pcm {
    fn drop(&mut self) {
        OUTPUT_ACTIVE.store(false, Ordering::Release);
        unsafe { plx_pcm_close(self.0) }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_and_unknown_codecs_are_never_software_claims() {
        for name in ["aac", "ac3", "eac3", "", "unknown"] {
            assert!(!super::software_codec(name));
        }
        for name in ["dts", "dca", "truehd", "flac", "opus", "vorbis"] {
            assert!(super::software_codec(name));
        }
    }
}
