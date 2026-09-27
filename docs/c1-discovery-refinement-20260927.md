# Discovery refinement checkpoint

This pass keeps the existing glass shaders, blur chain and dithering. The missing-poster fix from f11cb365 remains. A broad glass replacement is deferred until after the native checkpoint.

Expanded catalog details now use measured title and synopsis height, with genres and actions following in one flow. Open in Library is a labelled shared button; Watchlist keeps its bookmark and centered caption below. The information hit target is the poster, so it cannot cover the action row. Cast remains below the hero with bounded synopsis height.

Search retains the inline shelf geometry during dismissal and contracts it with the shared spring. The previous first closing frame moved a neighbour from x=1570 to x=656. The regression fails on that discontinuity; final geometry tests now wait for the spring to settle. The fading shell and returned poster share the same transition.

Discovery people use the existing Person and Filmography screens. The shared person owner canonicalizes plex://person/ URIs for provider requests, resolves cloud-only identities against each library, and distinguishes people with empty local keys by their global identity. Expanded catalog metadata also retains each validated provider Role identity as tagKey; malformed or mismatched person keys are not joined. No new metadata endpoint or parallel filmography implementation is added.

Tests and native captures belong to the three maintained paths in tests/tv. New coverage: catalog-flow-layout, catalog-close-motion, discovery-person-filmography. Native logs and private rollback assets are retained outside the repository. No household Watchlist, watch-state or acquisition action is part of this visual pass.

Research references: https://github.com/ybouane/liquidglass and https://github.com/rukkiecodes/liquid-glass describe independent implementations, not verified decompiled Apple code. Their backend dependencies and performance claims do not establish performance on LG C1. A future experiment must preserve this checkpoint, isolate the material behind shared primitives, and compare native frame timing and legibility before promotion.
