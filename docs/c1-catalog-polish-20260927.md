# Catalog presentation follow-up

Source baseline: 7265935a. Target: spare .199 only.

The captured live card was shorter than adjacent posters. Its information rectangle and wide text buttons consumed the panel, and clamping the popup independently from a lower shelf could cover other rows.

The inline expansion now matches poster height, reserves its width in the row, and moves/restores the whole Search viewport. Search paints the shared frosted glass below the modal band, allowing the existing frozen host snapshot to retain it without adding per-frame modal blur. Controls use the existing disc widgets, with outlined/filled Watchlist bookmark assets and a focused action label.

Expanded catalog details retain available provider ratings and the exact local-library match, show navigable headshot cast credits, and request background trailers through the existing asynchronous PreviewStart route after the normal dwell. The shared library trailer transport moved to ui/trailer.rs. Up promotes; Back/Down collapse; leaving details stops the preview. The compact card retains its explicit Trailer action, including when background autoplay is off. Autoplay preference and preview volume remain the player's authority. Catalog parts have distinct negative-cache keys; cloud IDs remain separate from PMS rating keys.

Host regression cases cover row alignment/restoration, local-only library action, compact-to-full Back behavior, cast availability, dwell-before-preview, no feature-play intent, promotion collapse, and stop on cover. TV fixtures live in the existing Search/Discover and Playback/Details/Watchlist paths. Simulator artwork is synthetic; it does not establish LG video-plane or audio behavior.

Build, test, captures and private rollback are kept in the sibling PlxNativeArtifacts/20260927-catalog-polish directory. Device verification is recorded there after installation; no production credentials belong in this source tree.

Device follow-up found catalog trailer URLs without an explicit port were reassembled by the player's legacy PMS splitter with port 32400. Cloud trailer URLs now carry HTTPS port 443 explicitly. A route regression test exercises the actual StreamUrl split/reassembly and failed with 32400 before the fix; PMS origin defaults remain unchanged.

Follow-up: expanded title metadata follows the measured title height; action captions are centered below their icons. Inline horizontal edges dismiss to the adjacent Search poster using an instance-bound return message. The test suite includes both directions.

The port audit also covers external StreamUrl, curl resolver hints and absolute HLS children: full HTTP(S) URLs use 80/443 when omitted, explicit ports win, bare PMS overrides retain 32400, and HLS same-origin enforcement remains intact. Both TVs authenticated to the pinned companion on 8093; configured local service ports responded.

Keeping Up with the Kardashians is present in Plex but absent from Sonarr. The companion now reports structured unmanaged/ambiguous/numbering errors. The TV preserves local seasons and displays the reason without an endless transient retry; genuine network failures still retry. No series is added to Sonarr and no acquisition is triggered by these reads. Companion source and deployment evidence live outside this repository in PlexPipelineRust and the artifact directory.
