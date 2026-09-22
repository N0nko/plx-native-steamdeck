# Stable TV screenshot macros

Use `tools/tv-ui-suite.py` inside WSL after a verified stable release deployment. It uses actual
remote input, a persistent SSH connection, one channel per key sequence, the release heartbeat,
and hardware display captures. It does not depend on development triggers. Set `SSHPASS` locally
if key authentication is unavailable; never put credentials in a plan or committed file.

```
python3 tools/tv-ui-suite.py --tv TV_ADDRESS --plan tests/tv/2-search-discover/keyboard-discovery.json --output /tmp/ui-review-RUN
```

The output directory must be new. Start the search plan from freshly launched Home. Each step
stores its keys, expected/observed route, time, image hash and heartbeat evidence. Open `index.html`
to review the whole run. Route mismatches stop subsequent input and preserve the partial gallery.
Installed bytes must match this checkout's `pkg/plxnative`, with a release startup log. The TV lock
is released even on failure. A screenshot and matching route are not proof of correct appearance.

Plans are content-dependent fixtures. Review the discovery card before creating a library or
playback continuation. Library actions must use a confirmed local item identity; never blindly
activate a Watchlist/Prepare/Repair/Mark Watched button. Playback plans declare `"playback":true`
and require `--allow-playback` as well as the user's permission for real viewing-state changes.
Use separate plans for library/detail/versions/tracks/person/settings and playback/seek/resume;
record unvisited screens explicitly instead of claiming complete coverage from a smoke sweep.

The [live discovery and playback map](live-discovery-playback.md) lists the reusable playback,
resume, panel and held-seek plans. A playback step may specify `"hold":"RIGHT"` or `"hold":"LEFT"`
to capture the seek preview while repeat events are still being sent. Release is guaranteed on
normal completion or a capture error; later input stops on any error.
