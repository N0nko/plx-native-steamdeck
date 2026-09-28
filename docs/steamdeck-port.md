# PlxNative Steam Deck port (alpha)

Independent fork: https://github.com/N0nko/plx-native-steamdeck
Branch: `steamdeck`, based on the custom v0.7.0 build (`c4b4cb42`).

## Build and launch

On Debian/WSL with Rust stable, SDL2, SDL2_ttf and OpenGL development libraries:

```sh
sh steamdeck/build.sh
```

Copy the contents of `steamdeck/dist/` to the Deck and run `sh install.sh` there.
The installer uses the user Flatpak installation of `io.mpv.Mpv`; it does not modify
SteamOS's read-only image. Add `~/.local/share/plxnative-steamdeck/launch.sh` as a
non-Steam game. Use Gaming Mode on a Steam Deck OLED for HDR10. The desktop entry
also has an Offline downloads action (`launch.sh --downloads`).

Sign in using PlxNative's existing Plex flow. Hold OK/A on a movie or episode,
choose **Download original**, and open **Downloads** from the profile menu.
Downloads provides progress, pause/resume, deletion, and local playback. One
original downloads at a time; a busy message asks you to pause it before starting
another. A/B, the D-pad, keyboard and pointer input use the existing native UI.

## Architecture

The additive `steamdeck` Cargo feature enables the desktop SDL/OpenGL UI and a
separate playback adapter. The `plxnative-deck` binary is the shipping entry point;
`plxnative-sim` with `steamdeck,devtriggers` is used only for fixture testing.
Default webOS builds keep their Starfish/ACB backend.

The Deck canvas is 1920x1200 logical units, uniformly scaled to the full 1280x800
display. Shared screen bounds, clipping, bottom controls and pointer coordinates use
that canvas; media retains its original aspect ratio. Downloads remains in the
hold-A/OK item menu, as requested.

mpv's gpu-next renderer owns decoding, colour conversion, and HDR presentation.
Its OSC and default controls are disabled. PlxNative renders its actual native HUD
into a premultiplied BGRA plane which mpv composites as SDR graphics over HDR video.
The video never passes through PlxNative's 8-bit UI framebuffer. HUD transfers use
private tmpfs files and acknowledgement before reuse. IPC carries input, transport
commands, position, duration and track selection. The sandbox dies with its parent;
Back sends Quit and reaps the child.

The video presenter runs independently of the UI. Both presentation gates let the
HUD settle, controller sensor/axis events do not cause redraws, and the covered SDL
window is not swapped while mpv presents. HUD buffers are reused and read as BGRA
instead of allocating and converting two full-screen buffers for every update.

Gamescope's Wayland socket and host Vulkan WSI layer allow `x11vk` to negotiate a
10-bit PQ surface. This matches the host-layer approach in Moonlight's Flatpak.
The direct `waylandvk` route on the tested SteamOS image lacks `wp_viewporter`.

Downloads retain original file bytes. An atomic manifest identifies the account
scope, profile, server, rating key, and exact Part. URLs and access tokens are never
persisted in these manifests or passed in playback process arguments. Strong ETags,
If-Range and exact Content-Range checks protect resumed files; a server ignoring
Range restarts from byte zero. Completed files require the expected length and an
atomic rename. Partial files cannot be played. Private file modes, file locks,
symlink refusal and free-space checks cover storage operations.

## Device evidence, 2026-09-28

Test target: Steam Deck OLED (Galileo), SteamOS 3.9.2, 1280x800 at 90 Hz.
mpv 0.41.0, libplacebo 7.360.1, FFmpeg 9.0.1 from Flathub.

The synthetic HEVC Main10 fixture played through the integrated native UI with:

- VAAPI hardware decoding;
- `VK_FORMAT_A2B10G10R10_UNORM_PACK32` and `VK_COLOR_SPACE_HDR10_ST2084_EXT`;
- HDR mastering metadata passed to Gamescope;
- PlxNative's native controls composited over video, with OK pausing playback;
- an original downloaded from the native item menu, verified byte-for-byte, then
  played from the native Downloads screen after the fixture server was stopped;
- pause and seek through the native controls, a cold launch into Downloads with
  the server still stopped, and deletion through the native Downloads screen;
- inspected 1280x800 captures of Home, detail, library, search, settings/legal and
  Downloads, with no UI letterbox and bottom content using the taller canvas.

The final synthetic HDR playback sample measured 5.0% of one CPU core in the UI
process and 2.3 HUD uploads/second, with zero decoder or video-output dropped
frames. This includes developer status logging and loops the eight-second fixture;
it is not a whole-device wattage or battery-life measurement. The earlier redraw
path uploaded roughly 55 full frames/second. `steamdeck/test-performance.py`
explicitly dismisses the test harness's pinned HUD and guards against continuous
hidden-HUD uploads.

`steamdeck/test-device.py` checks the download engine and backend directly.
`steamdeck/test-native.py` checks integrated streaming and controls.
`steamdeck/test-downloads-ui.py` checks the native download and offline flow.
All use synthetic media and a loopback mock server, never household watch state.
Core tests cover validated resume, an ignored Range, bad ranges, redirect refusal,
low storage, cancellation, profile isolation, deletion, and credential redaction.

Host verification uses `make check`, the nightly no-default-feature library check,
and Deck-feature tests for full-height geometry/pointer mapping, presentation gates
and controller event filtering. On a root WSL runner the permission tests require
dropping DAC override/search capabilities (`setpriv --bounding-set=-dac_override,-dac_read_search`).
Shell scripts and the source-audit allowlists use LF endings on Windows checkouts.

Manual FFI review: SDL controller handles remain confined to the UI thread;
desktop GL BGRA readback uses a reusable buffer of exactly width*height*4 bytes,
and the in-place row flip uses disjoint slices. No webOS FFI signature changes.
The documentation claims are scoped to the Deck feature and the observed HDR10
path; host tests do not establish ARM/webOS device parity.

## Alpha limits

This is an original-file player, without the television's adaptive transcoding,
background previews or automatic next-episode pipeline. Multi-part downloads are
explicitly refused. Downloads preserve all embedded tracks; external subtitle files
are not downloaded. The offline library currently keeps the title and file state,
not a complete offline copy of Plex's metadata and artwork.

Plex watch-history synchronization, suspend/resume, physical controller ergonomics,
audio/subtitle selection across a broad codec library, external HDR monitors and
Dolby Vision still need acceptance testing. HDR10 output negotiation is verified;
panel luminance accuracy and colour calibration are not claimed. Device tests use
a stopped server as the offline condition; they do not change the Deck's Wi-Fi.

## References

- [Moonlight Vulkan renderer](https://github.com/moonlight-stream/moonlight-qt/blob/master/app/streaming/video/ffmpeg-renderers/plvk.cpp)
- [Moonlight Gamescope integration](https://github.com/flathub/com.moonlight_stream.Moonlight/blob/master/com.moonlight_stream.Moonlight.json)
- [mpv colourspace signalling](https://mpv.io/manual/stable/#options-target-colorspace-hint)
