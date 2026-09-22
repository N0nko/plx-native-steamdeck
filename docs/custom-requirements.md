# Custom PlxNative requirements and rebase audit

The user's consolidated request history is the acceptance scope, not proof of implementation.
Later corrections win. Baseline: the preserved dirty `PlxNativeResearch` tree at
`e0a9fb36` (custom installed 0.6.6 lineage), its `docs/c1-playback-essentials.md` and
`docs/c1-play-version-20260921.md`. Rebase base: upstream `aa823c79277085796c5964da022a12a1a8a62490`.
The uploaded recovered integration is supporting evidence, never a replacement source tree.

## Acceptance inventory

| Area | Required behavior | Source / current evidence |
|---|---|---|
| Architecture | Native responsiveness; maintainable upstream ports; preserve working features | Upstream owned screens/stores/adapters retained; no legacy global UI overlay |
| Search | Local plus full Plex movie/show catalog; exact GUID deduplication; local result preferred | `search.rs`, `plex/catalog.rs`; retained custom behavior |
| Discovery | Empty-query trending/recommendation shelves; focused-title similar movies/shows and cast; actor filmography; repeated exploration and Back | `discovery.rs`, `screens/search/discovery.rs`; code retained; contextual cards now open details on the first activation |
| Catalog | Explicit Watchlist add/remove with readback; existing acquisition pipeline consumes Watchlist | `catalog_action.rs`, `app/adapters/catalog.rs`, `screens/catalog_details.rs` |
| Local navigation | Every card opens information first; explicit Play starts playback; catalog titles can open exact local library match | Search/library use Detail; Continue Watching now uses Detail; catalog matching resolves exact GUIDs from the server |
| Search layout | Centered Top Results / Movies / TV Shows; large posters; contextual actors separated from primary title rows | `screens/search/`; shared Glass controls |
| Keyboard | Compact A–Z then 1–0 grid; Space/Backspace above; no More/Done footer or backing panel; Search opens keyboard | `screens/search/keypad.rs`; entry opens letters; Back and return-memory regressions covered |
| T9 latest correction | 1 deletes; first 0 inserts space, second quick 0 changes it to dash; bottom right ABC returns to letters; remove unused punctuation buttons | Supersedes the earlier punctuation row and repeated-space behavior |
| Seeking | Nearest preview follows seek cursor; no stale unrelated image; smooth holds and stable seek bar; actual transport latency improvements | `ui/seek_preview.rs`, `ui/seek_presentation.rs`, `ff.rs`; device latency must be measured, not inferred |
| Playback | Preserve resume and explicit file/audio/subtitle identity; unobtrusive upper-right clock; finish time HH:MM; normal release has no FPS | Player/detail/version modules; stable build excludes devtools |
| Audio/subtitles | Main English preference over commentary; explicit choices win; avoid burn-in; direct playback where actually supported | Route/metadata policy preserved; experimental local audio is disabled, no native DTS/TrueHD claim |
| Markers | Plex-generated seek stills/intro/credit markers; hidden-HUD Skip activation | Player marker and overlay paths retained |
| Media state | Honest Ready/Local/Streaming/Queued/Preparing/Replacing/Unknown; measured progress; Ready without 100%; responsive bounded polling | `media_companion.rs`, `app/adapters/media_companion.rs` |
| Get Ready | Prepare the selected existing file through Ready Store; supplied popcorn only here | Detail Prepare and version Prepare use popcorn; audience rating uses original Crowd icon |
| Repair | Small expanding control after Mark Watched; replace selected release, including ready media; prefer suitable quality | Detail companion controls and Rust companion; distinct from materialization |
| Versions | Play Version, selected Part, audio/subtitles, status and preparation agree | `screens/versions.rs`, `media_version.rs`, `metadata/versions.rs` |
| Artwork | Preserve poster/thumbnail improvements and gradient/transition fixes | `posters.rs`, shaders and page ground; inspect TV output before claiming visual parity |
| TV show expansion | Browse all catalog seasons/episodes, distinguish owned/missing, request an exact season/episode | New extension, not present in the old catalog detail implementation; request backend choice pending |
| Backend | Ready Store bandwidth/progress, bounded stuck-grab recovery, cold streaming/eviction, companion after restart | Separate Rust/NzbDAV systems; app rebase must not reset their config or restart shared services |
| Deployment | Release stable, backup, verified installed bytes, preserve sign-in/private HTTPS pairing | .199 default; .153 explicitly authorized for this session; latest deployment still pending |

## Unfinished or exploratory work

- PC-hosted dictation was exploratory, not an acceptance requirement.
- Local DTS/TrueHD decode remains experimental and disabled; stereo conversion is not native passthrough.
- Before-login companion startup remains unresolved. Do not label a login task as pre-login startup.
- The existing companion only observes/prepares/repairs exact media already in Plex. It has no
  catalog season/episode acquisition endpoint. A Watchlist entry is not an episode request.
- Historic 0.6.6 notes explicitly deferred local lookup for recommendations absent from the original
  search results. The rebase must add exact-GUID lookup rather than guess from titles.

## Verification policy

Record host tests, simulator captures, ARM/package checks and TV results separately with the
commit tested. Synthetic simulator catalog omissions are not proof of production feature loss.
Never mutate Watchlist, downloads, watch history or playback merely to demonstrate navigation.
Keep private rollback archives, tokens, pairing and certificates outside distributable source.

Language/acquisition policy remains English OR Lithuanian OR exact original language; the
English audio preference is a playback default, not an English-only acquisition policy.
