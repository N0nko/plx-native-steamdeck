# Series requests and stable contextual search

This update keeps the existing library, catalog panel, Watchlist, version selection, preparation, playback, seek previews, icons, and trailer settings. Catalog details remain compact. Shows already in the library open the ordinary series view through Open in library; entirely new shows keep the existing Watchlist acquisition flow.

## Search

Selected result tabs stay white and dim slightly under focus; the checkmark is removed. Context remains exclusive to typed Movies/TV Shows searches. After two seconds on a title, cast appears first and similar titles below. Each level appends to the same document. Parent rows and scroll positions remain. Back focuses the owning parent; holding a different sibling replaces only its descendants. Cached results obey the same dwell. At most 20 contextual levels remain resident; explicit Discover directory navigation retains its existing page/Back behavior.

Library/Home omit Recently Watched and single-title actor/director recommendation shelves using Plex hub identifiers. Continue Watching, recently added/released titles and explicit collections retain their existing behavior. Live Plex evidence showed `movie.by.actor.or.director.*` with one item and `movie.recentlyviewed.*` for Recently Watched.

## Series

The optional paired companion catalog is requested on a worker after local details arrive. The server supplies a full season/episode index from the existing Sonarr-managed show, matched by one exact TVDB GUID. Plex supplies local ownership. Missing seasons and episodes stay focusable and muted. OK on a missing season requests its aired missing episodes; OK on an absent episode requests that exact number. Hover never requests acquisition. Existing episodes preserve their local ratingKey and selected Part.

Companion endpoints are HTTPS with the existing private certificate pin and bearer pairing:
- GET /v1/series: exact machine_id + local show rating_key; no mutation.
- POST /v1/request-episodes: those IDs + season and optional episode number.
- POST /v1/repair: existing exact episode replacement still enters Replace; afterward it attempts gaps in the same season.

The Rust companion uses its existing ArrClient and Sonarr EpisodeSearch with explicit episode IDs, leaving profiles and language policy untouched. It excludes existing Plex episodes, Sonarr files, unaired/unknown-date episodes, queued downloads and recent grabs. Commands are capped at 32 missing episodes and five explicit requests/minute. Durable reservations prevent retries for 30 minutes, including lost responses. The normal controller config stays noop with global apply disabled. No NzbDAV database edits, media deletion, shared-service restart, or broad SeasonSearch occurs.

Unmapped/unmanaged shows and nonstandard episode numbering degrade to the existing local series view; they are not matched by title. Missing episode stills use muted series art when no local still exists. Request errors remain distinct from acceptance. Repair can succeed even if checking season gaps fails; the status says gaps unknown.

## Verification

The old contextual promotion was reproduced by a failing host regression before editing. Tests cover preserved parents, sibling replacement, two-second cached dwell, exact missing-item actions, catalog merge identity, existing file/airdate/queue exclusions, and duplicate/lost-response suppression. The three TV suite groups were updated; acquisition fixture steps require a mock paired companion and are not proof of live grabs. Exact-tree build and deployment evidence is kept privately with the dated artifacts. Device visual/playback claims require captures and user testing.
