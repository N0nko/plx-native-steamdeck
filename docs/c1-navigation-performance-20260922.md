# Navigation performance update

The user reported a keyboard covering scrolled content, sticky filter tabs, stalled contextual tiers, delayed detail openings and thumbnail pop-in. Captures `tv153-user-bug-now*.png` reproduce the keyboard overlap. Private device logs record detail completion at 238–563 ms; they do not establish a synchronous trailer-fetch stall. Trailer fetching is already on workers and autoplay waits for the existing two-second dwell.

Changes:
- Close integrated keyboards when focus leaves their keys or wheel scrolling begins; clip results to the keyboard boundary while it remains open. Results return Up to filters, and Down returns to results.
- Promote pointer/geometric entries into contextual rows just like the remote Down path. Filtering an active context keeps its parent trail.
- Replace quadratic discovery key lookup/pruning with temporary indexed lookup; preserve identities and remove a full-page clone.
- Seed native detail heroes from retained library/search rows as well as Home before the asynchronous metadata/extras work completes. Exact server/item matching prevents borrowing another server's artwork.
- Add a bounded encoded-art disk tier using existing poster workers: 192 files, 512 KiB each, at most 96 MiB. Full request and origin are hashed, partitioning sizes and credential grants. Sign-out clears it; GPU residency limits stay unchanged.
- Simplify the search placeholder to `Search`.

The three TV fixture groups include focus round trips, contextual tiers, first-image review and repeated tab/cache review. This update is for the user's manual testing on .153. Build/install evidence lives in the private local artifact directory; no new device sweep or playback test is performed in this turn. Transient TV composition artifacts and perceived latency require live device review and are not claimed resolved from host tests. No nightly rebase is included.
