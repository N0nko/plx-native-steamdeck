# Glass navigation performance pass — 2026-09-27

Baseline: e058703d, stable release on the test TV. The Middle season-row arrow sequence
(4 left, 4 right, repeated 5 times; 80 ms down/up) produced sustained 23–24 FPS,
44 ms median frames, p95 47–49 ms, with drawing dominating and no texture uploads.
Raw account-bearing logs are private in the artifact directory.

Changes:
- Owner review rejected excluding focus from the glass source and the broad analytic
  bloom. Restored the original white cursor inside the source: neighboring buttons
  refract its light again, accepting that this requires fresh filtering during motion.
- Plated tabs fade their whole optical material beneath the arriving white capsule;
  fading only the scrim left full-strength refraction that disappeared on the last frame.
  The glass rim now obeys cascade opacity too: at coverage .1, rim weight .2 and
  opacity .1, the previous expression kept a .2 rim; the corrected result is .02.
- Hero Next publishes incoming/outgoing artwork identities and reveal state at the same
  time as carousel slide positions. Input may paint before the next Tick; previously the
  outgoing slot was empty on that press frame.
- Detail layout measurements survive unchanged ticks; the existing content stamp and
  store invalidations still detect changed text, seasons and other geometry inputs.

The hero and layout regression tests were observed failing before each fix. The glass
regression now explicitly preserves the owner-preferred neighboring reflection. TV fixtures are maintained under
1-home-movies-tv/glass-performance.json and 3-playback-details-watchlist/glass-performance.json.
The timing sample is separate from captures and stack profiling. Stack samples used target
symbols only because the sampler reported a local binary mismatch; no file/line or profile
FPS claims are based on that sample.

Artifact directory: PlxNativeArtifacts/20260927-glass-performance. RESULT.md contains final
committed build, deployment and native timing evidence; host tests cannot establish pixels
or television frame pacing. No 120 Hz claim is made: prior SDL device probe reported refresh
unspecified and a 1920x1080 drawable, despite the panel's higher physical specifications.
