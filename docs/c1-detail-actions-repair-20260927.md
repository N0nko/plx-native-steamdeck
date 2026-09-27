# Detail action lighting and Repair — 27 September 2026

Detail Play, Play Version, Repair and Get Ready now use the same shared-source neighboring-focus reflections as the keyboard and season buttons. The existing circle-to-label expansion, hit targets, disabled alpha and geometry remain. Mixed pill/disc controls draw the focused face before the shared source is captured and their glyphs afterward; no additional blur per button is introduced.

Repair is an explicit first-click action. It publishes one typed request for the selected profile/server/item/Part and becomes busy immediately. Busy, disabled, mismatched Part and stale-profile activations cannot enqueue a repair. Get Ready retains its existing confirmation and preparation semantics.

Regression coverage exercises the actual movie and series action row, per-focus source refresh and rest reuse, the exact transformed/faded CircleButton paint arguments, and first-click/duplicate/stale Repair behavior. Native visual review remains necessary; host drawing assertions do not prove hardware composition or pacing.

The action-row navigation fixture lives under the existing Playback/Details/Watchlist test path. It uses arrows only. Real repair dispatch is a backend acquisition write and is verified separately on an explicitly selected title.

Backend evidence, private exports and device verification for this pass live outside the source tree under PlxNativeArtifacts/20260927-detail-repair. No credentials or household media identities are committed here.
