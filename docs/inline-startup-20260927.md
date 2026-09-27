# Inline discovery and black startup

Catalog posters in Search/Discover expand within their row. The information area opens full details; the available library, trailer and Watchlist actions remain on the right. Open in Library still requires the exact local Plex identity. Full details add supplied genres, cast, runtime, certification, studio and backdrop without another metadata request. Back returns to the compact card, then restores the previous search geometry and focus. Filmography catalog destinations open full details directly.

The system splash is generated from `assets/splash.svg` by `python3 tools/mksplash.py` under Linux/WSL with system librsvg and Cairo. It is an opaque 1920x1080 black image with a centered white PLX mark. The old master artwork remains available for historical reference; it is no longer the startup source.

Home clears to black and omits the ambient wash while at the hero. With no artwork, it also omits the artwork scrim; its gray tint previously produced a visible gradient even over an otherwise black clear. Existing grid ambience, artwork transitions, glass controls and menu shaders are retained.

The Search/Discover catalog fixture now covers information/action focus, full details, and both Back steps. Home has a startup capture fixture. Existing Playback/Details/Watchlist fixtures remain the action destination checks. Simulator captures establish layout only; native visual and playback behavior require the TV. Recorder state shapes changed, so old state replay recordings must be re-recorded.
