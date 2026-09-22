# Live discovery and playback regression map

Empty Search reads the active profile's HTTPS discovery index at
`https://discover.provider.plex.tv/hubs/sections/home`, preserving the provider's order, names and
advertised keys. It hydrates up to 32 feeds within a 20-second deadline, bounds each response at
2 MiB, each media row at 60 titles, and the page at 64 rows. Mixed feeds split into Movies and
TV Shows. Empty or unsupported feeds are omitted. Successful feeds survive other feeds failing.
These are bounded pages, not infinite scrolling. Search queries also request up to 60 catalog hits.

Genre, Award and Decade entries open provider-advertised category pages. Their keys are restricted
to approved paths on the same provider; account credentials never go to an advertised foreign URL.
Browse entries use the shared glass controls; no placeholder posters are invented for categories
without art. Trailer rails open their explicitly identified parent movie/show catalog details;
direct trailer playback from those rails is not implemented. Contextual title/cast exploration,
profile generation checks, cache and Back trail remain in place.

The bridge retains a last-successful discovery page for the active profile. Fresh cached feeds
are reused for 15 minutes; stale feeds remain visible during refresh and on provider failure.
The cache is in memory, is discarded across profile generations, and is unavailable to recording
or replay. Local library loading is independent of this provider request.

Media status now belongs beside the Direct Play/Direct Stream facts. Play Version uses the same
icon-at-rest, expanding-label-on-focus control as the other secondary actions. The existing finish
time display remains unchanged. Existing series heroes, resume actions, seasons and landscape
episode cards remain the implementation under test.

One Back from Home content returns focus to Home in the top navigation. One Back from Movies/TV
content returns focus to that section's top tab. Back again retains the existing root/home action.
An in-flight library transition still consumes Back to cancel that transition first.

## Device suite

The maintained source paths and generated output paths are:

1. `1-home-movies-tv`
2. `2-search-discover`
3. `3-playback-details-watchlist`

`tests/tv/coverage.json` lists 66 current feature checks, including cases needing special media or
a controlled account. Add a case and its plan whenever a feature changes. Step `covers` lists those
case IDs. `python3 tools/tv-suite-report.py --output CAPTURE_ROOT` generates one gallery and coverage
ledger per path plus `UI-GALLERY.html`. Missing captures remain explicitly untested. Superseded
runs belong in `archive/previous-sweeps`, outside the active galleries.

Run `tools/tv-ui-suite.py` from the checkout whose release package is installed. See
[tv-ui-suite.md](tv-ui-suite.md) for locking, capture and approval requirements. All playback plans
require `--allow-playback` and an explicitly confirmed local library item. Playback affects resume
history. The runner checks the installed binary and current route before sending each macro.

| Plan | Starting state | Coverage |
| --- | --- | --- |
| `tests/tv/3-playback-details-watchlist/playback.json` | Local detail, Play focused | Start, advancing video, pause, short forward/back seeks, resume, exit |
| `tests/tv/3-playback-details-watchlist/resume.json` | Same detail, Play focused | Resume saved position; compare position evidence with prior exit |
| `tests/tv/3-playback-details-watchlist/held-seek.json` | Playing, no overlay | Repeated RIGHT held through capture, then release and landing |
| `tests/tv/3-playback-details-watchlist/player-panels.json` | Playing, no overlay | Subtitle/audio panels, subtitle options, quality list, information, return |

The panel plan inspects options without changing track or quality. `hold` accepts LEFT/RIGHT only
in authorized `player/none`; repeat events run while the screenshot is captured and release in a
finally block. Each run produces its own manifest and gallery. A matched route is navigation
evidence; visually inspect decoded frames, changing previews, text, focus and spacing separately.

The initial device fixture was a local 4K HEVC HDR10 movie with English EAC3 5.1 and English SRT.
It exercised actual decoding and seek previews. Screenshots cannot establish audible output or
speaker/channel fidelity. Intro/credit skips, chapter navigation, episode auto-next, alternate
versions, quality/track changes and recovery from a network interruption require suitable separate
fixtures; opening their menus is not a completed functional test of those actions.
