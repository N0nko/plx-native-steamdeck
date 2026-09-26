# On-demand subtitles

Open **Play Version → Download subtitles**, or **Subtitles → Download subtitles** during playback. Select a language, search, then select a release matching the file. English is the initial language; Lithuanian is next. No provider account or additional service is installed.

The client uses Plex Media Server's `GET /library/metadata/{ratingKey}/subtitles?language=…&hearingImpaired=0&forced=0` and `PUT` to the same endpoint, with the returned provider `key` encoded as one query value and the selected `mediaItemID`. The local OpenAPI snapshot mislabels the search operation as “Add subtitles”; the search/download verbs are corroborated by the Python PlexAPI source. A read-only probe against this household's PMS returned eight SRT candidates on 2026-09-26. Downloads are explicit user actions and are not exercised against household media by the automated suite.

Search and download run on a worker. The panel binds to profile, server, rating key and Part; leaving it cancels pending application of results. Plex accepts downloads asynchronously, so the client waits for a uniquely identifiable stream on that exact Part. Failed/uncertain attachment is reported without automatically repeating the write. Existing playback and audio remain active while searching.

Only supported text formats are offered. Direct-play video uses the existing client-side subtitle renderer, without a video reload or burn-in. If playback is already transcoding for another reason, existing subtitle/transcode policy still applies; this feature does not promise that every file/track combination can direct play.

References: [Plex support](https://support.plex.tv/articles/subtitle-search/), [Python PlexAPI implementation](https://github.com/pkkid/python-plexapi/blob/master/plexapi/video.py).

Verification: API parser/transport, exact stream selection and cancellation tests; TV navigation fixture in `tests/tv/3-playback-details-watchlist/subtitle-search.json`. Provider download completion, subtitle synchronization and native playback must still be checked on a TV before calling end-to-end playback verified.
