# Custom integration on upstream aa823c7 — 22 September 2026

This integration ports the recovered `5918b4b` attempt and the working custom baseline onto upstream `aa823c79277085796c5964da022a12a1a8a62490`. It retains the upstream owned-screen architecture, session persistence/fallback fixes, build identities and release guards. The uploaded tree was used as evidence, not overlaid onto the stable source.

## Preserved and integrated behavior

- Search combines local libraries and Plex catalog results. Exact nonempty Plex GUIDs prefer a known local playable result; a similar title is never sufficient. Cloud-only cards open catalog details with profile-scoped Watchlist observation and an explicit add/remove action with readback.
- Empty search loads bounded discovery feeds. Focused titles/people load debounced contextual shelves. Down enters related results; Back restores the prior page, focus and scroll. Requests and replies carry profile/serial identity. The trail and caches are bounded.
- The default keyboard opens with Search and offers A–Z, 1–0, Space and Backspace, without a footer or backing panel. In T9, 1 deletes and 0 cycles space/dash; ABC returns to letters. Physical digits use the same multitap logic with duplicate text-echo suppression. Blue on LG or F2 on desktop opens the optional SDL/LG keyboard. Top Results, Movies and TV Shows filters remain separate controls. Back closes the keyboard before leaving context or Search.
- Play Version exposes exact file/Part choices, audio tracks and subtitles (including explicit Off). Selections remain scoped to server, item and Part. Missing or ambiguous selections fail rather than switching to a sibling file/track.
- Get Ready, Repair and media status use the existing paired companion over pinned HTTPS. Get Ready keeps its confirmation; Repair submits on the first explicit click. Capability checks, single-flight submission and stale profile/Part rejection remain. Pairing and sign-in are not in the source package.
- Detail recommendations retain category headings. Actor pages prefer available Movies, then Shows, without stealing restored/user focus. Cloud credits use the catalog detail/Watchlist surface.
- Seek previews request existing PMS indexed stills, coalesce requests and retain a bounded resident cache. HUD presentation preserves the seek target through reopen, includes local/finish clocks and retains upstream subtitles and skip controls.
- Popcorn is the Get Ready icon, with thicker strokes. Audience ratings retain the Crowd icon.

## Experimental local audio — disabled by default

The integrated default is `LOCAL_AUDIO=0`. The recovered audio implementation is retained behind an opt-in feature; it is not validated for physical TV audio or synchronization. Build commands for this candidate always name `RELEASE=1 FLAVOR=stable`; do not enable the experimental feature in the stable package.

`LOCAL_AUDIO=1` enables the Rust `local-audio` feature, builds FFmpeg's native
DTS (`dca`), TrueHD, FLAC, Opus and Vorbis decoders plus libswresample, and stages
`libswresample-plx.so.7`. The build cache key and native objects distinguish this
mode. The default build retains subtitle-only FFmpeg and disabled audio stubs.

The architecture is compressed video → LG hardware decoder, compressed audio →
bundled FFmpeg → **48 kHz, 16-bit stereo PCM** → a queued SDL audio device.
AAC/AC-3/E-AC-3 keep the native path. Decoder parameters accept at most eight input
channels; surround inputs are downmixed to stereo. This is not bit-perfect output,
multichannel PCM, TrueHD Atmos, DTS:X rendering, or new passthrough support.

At live boot, capabilities are advertised only after both the exact bundled
FFmpeg decoder libraries and a real stereo PCM output open successfully. Dummy
and disk SDL drivers do not qualify in shipping code. This proves an output can
open, not that it will coexist correctly with Starfish on a particular television.

The decoder preserves packet side data and timestamps, uses a single decoder
thread, bounded decoded buffers and 20 ms PCM chunks, and flushes on seeks. The
main-thread sink limits its queue to 250 ms, pauses through playback holds,
flushes during seeks/teardown, and schedules PCM against reported video position.
SDL queue size does **not** include hardware output latency. LG's position reports
may be too coarse for smooth long-form A/V synchronization. This path must be
measured on the actual television and remains experimental even when boot succeeds.

A decoder/output failure disables software-audio capability until application
restart and reports an error. Retry/Choose Quality can then take the server's
conversion path; this is not a promise of seamless automatic recovery.

Unsupported video, forced subtitle burning and bandwidth limits can still cause
server conversion. No AV1/VP9 video path is added. A successful software-audio
route avoids an audio codec conversion on the server, not every possible transcode.

## Validation provenance

The supplied recovery archive reports C decoder/sanitizer/PCM checks. Those are historical claims from that archive, not tests performed on this integration. The integration's own compiler, test, simulator and package evidence is recorded separately in `docs/rebase-20260922.md` and the handoff logs. Desktop rendering cannot prove LG decoding, physical audio, frame rate or video-plane composition.

## Offline C test reproduction (Linux)

With a host FFmpeg 9 prefix containing the five decoders and libswresample, a C
compiler, system FFmpeg capable of encoding test fixtures, and SDL2 installed:

```sh
python3 ci/test-local-audio.py --ffmpeg-prefix /path/to/host-ffmpeg \
  --output /tmp/local-audio.json
python3 ci/test-local-audio.py --ffmpeg-prefix /path/to/host-ffmpeg \
  --sanitize --output /tmp/local-audio-sanitized.json
```

These commands test the production C boundary, not the Rust integration. A host
build of this crate with `--features local-audio` also needs the corresponding
headers through `PLX_FFMPEG_INCLUDE` or `vendor/ffmpeg-prefix-host/include`.

## Acceptance before enabling on a television

Compile and test both feature configurations. Validate native-only playback first.
Then exercise real DTS variants/DTS-HD and TrueHD movies, stereo/surround sources,
seeks in both directions, buffering, pause/resume, repeated track changes,
Original/Auto/fixed quality transitions, output-device changes, stop/reopen and
long-session drift. Inspect CPU usage and acoustic A/V sync; do not infer either
from queue size. Check captions, external subtitle Off, per-version track IDs,
actor Back restoration and all named recommendation shelves with remote/pointer.

Existing replay recordings require regeneration for the changed screen inventory.
No new replay execution or full on-device scenario evidence is claimed here.
