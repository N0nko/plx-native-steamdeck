# Search scroll and shallow glass polish

Search's page-wide scissor stopped artwork at y=130, leaving a hard empty band at the top. The visual window now extends to the screen edge while pointer targets still reserve the standing bar's input region. The keyboard gutter and the lifted catalog opener retain their own bounds.

The shared output-dither area rule required both dimensions to reach 96px, so the 76px-high navigation glass never received anti-banding noise. The rule now accepts equal-area shallow surfaces with a minimum 48px short edge. Small control squares and thin strips remain unchanged. No shader, texture size, blur pass, sampling count, or animation was added.

Before-fix regression tests fail for both the artwork window and a 544x76 glass bar. Existing hit-boundary tests continue to protect top navigation. TV fixture: `tests/tv/2-search-discover/search-scroll-glass.json`; inspect actual captures, since route checks alone cannot establish foreground or visual correctness. TV screenshots establish composition; final panel banding remains subject to display processing and should also be judged on the physical screen.
