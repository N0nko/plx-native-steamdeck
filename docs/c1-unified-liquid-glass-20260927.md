# Shared liquid glass experiment

Baseline/checkpoint: 4720ab75 (before shared controls); original rollback tag checkpoint/discovery-before-glass-20260927.

The common Button, CircleButton, TransportButton and plated TabPill use the native backdrop shader for translucent faces. Shared panel_ground covers menus, track sheets and catalog panels. Focused opaque white controls retain clear selection. Regular sheet frost and 55% idle control frost expose the sampled content. Edge dispersion was initially 2.2 logical pixels; the keyboard follow-up increases it to 4.0 with the same two additional channel samples. This is native GLSL; Nik Delvin's CSS/SVG implementation was visual reference, not a webview dependency.

Controls within each structural page/snapshot segment share a source; their labels are excluded from its signature. Ordinary control focus remains above that source. Season strips and keyboard groups deliberately include their opaque focus plate below it, so neighboring glass reflects the white selection: moving that plate refreshes the shared source, settled focus reuses it. Actual underlay damage also refreshes it. Modals can declare a source above the surfaces band; hidden frozen-host paint remains excluded. The existing downsample/region budget remains in force.

Hardware movie/trailer planes are not in our GL framebuffer. These retain translucent reflected-edge controls and readable fallback panels, without falsely sampling stale/black video or adding screen capture to playback. Full video refraction is not implemented.

Release telemetry uses nine SDL performance-counter stamps, fixed 1ms histogram buckets (last bucket >=64ms), exact maximum, slow-frame count, and at most four phase logs per second. No GPU synchronization. Totals include swap wait; these are CPU wall-frame measurements, not isolated GPU times. Controlled replay suppresses production timing. An optional read-only SDL_GetCurrentDisplayMode lookup logs native refresh; it changes neither TV mode nor vsync.

Manual FFI review: DisplayMode follows bundled include/SDL2/SDL_video.h exactly (u32, three C ints, pointer); optional self-handle symbol, null/missing/failed query handled, no new firmware import or call from a frame loop. Native rate still needs checking after install.

Regression reproduced: three translucent buttons declared zero sources on baseline. New tests require one source, no control labels in its prefix, reuse through focus changes, refresh for background changes, and proper modal-band declaration. Hardware-video source refusal remains tested.

Validation and device observations are recorded in the artifact RESULT.md after build and installation. This source note does not claim a stutter-free or complete device sweep.
