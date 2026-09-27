# Native glass experiment

Rollback: checkpoint/discovery-before-glass-20260927 (2d459864). The shared fs_glass.frag shader has one PLX_LIQUID_GLASS_EXPERIMENT switch; setting it to 0 restores the previous shader path, while retaining Watchlist alignment.

Independent native adaptation informed by Nik Delvin's CSS/SVG recreation (https://github.com/nikdelvin/liquid-glass) and the WebGL approach at https://github.com/ybouane/liquidglass. No third-party implementation was copied. This is an experiment, not a claim of Apple equivalence.

The shared glass bevel now uses a bounded Snell-law curve (index 1.45), an inexpensive fifth-power grazing highlight above the tint, and 0.6-pixel RGB separation only on the light-blur rim. The flat interior takes its previous early-out. Deep menus avoid the additional two colour samples. Blur passes, refresh/caching, textures, dither, density/legibility controls and UI hit geometry are unchanged. PageDip source-alpha blending is preserved for the separated colour samples.

The standalone Watchlist icon now starts at the same x as the description, removing the extra 58px inset. With a local-library action it follows that pill with the shared 32px gap. Its caption remains centered underneath.

Native captures and production heartbeat samples are required before calling the material usable; idle FPS cannot measure motion. The previous hardware budget document describes a different LG model, so its numeric frame-rate claims are not evidence for this C1. Fixture: tests/tv/2-search-discover/glass-material-experiment.json.
