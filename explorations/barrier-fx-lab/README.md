# Barrier FX Lab

Standalone WebGL2 shader playground for exploring barrier looks (glow sigmas, wavy edge, strand
bundle) before committing any of it to `crates/render/src/barrier.wgsl`/`barrier.rs`. Not wired
into the app or build in any way — open `barrier-fx-lab.html` directly in a browser, no build
step, no server needed. Ported line-for-line from `barrier.wgsl`'s noise/glow math where the two
overlap (see the file's own comments), so slider values here mean the same thing they will in the
real barrier renderer.

Also includes a "Flash" section, for the separate barrier-hit flash effect
(`crates/render/src/effects.rs`/`effects.wgsl`, `project::FlashSpec`), split into groups:

- **Bloom / glow** (`flashEnabled`) is today's real flash exactly — a fixed elliptical 3-layer
  additive corona, no more, no less. With everything else in this section disabled/off, this alone
  is today's app look. `coreLegacyPlateau` is an A/B toggle for the flat-plateau fix (see
  `flashCoreStrength`'s own comment) — off is the shipped behavior.
- **Halo ring** (`ringRadius`/`ringWidth`/`ringIntensity`) is a small additional diffraction-halo
  accent at a fixed radius, also shipped today (`project::FlashSpec::ring`).
- **Flame corona** (`flameEnabled`) is the one piece still lab-only: continuous flame stacks
  wrapping the full 360 degrees around the light center, closer to an aurora curtain or
  solar-corona photograph than a beam-based starburst — see `flameCoronaStrength`'s own comment
  for the shape/streak/flicker math, including how the angular sampling avoids the seam a raw
  `atan2`-based coordinate would show at "pointing left" regardless of tongue count.
- **Chromatic aberration** (`chromaticEnabled`/`chromaticAmount`) re-evaluates the entire light
  stack once per color channel at a slightly different radius, the same "error grows with distance
  from center" shape real lens dispersion has, rather than a flat color tint. Also shipped today.

See the "Flash: app default" preset for today's exact look and "Flash: aurora corona" for the
dialed-in flame-corona look (exported straight from the tool's own "Export settings" button).
`flashCoreStrength`/`flameCoronaStrength`/`flashRingStrength`/`flashTotalStrength`/
`flashContribution` in the shader source are the relevant functions if porting any of this into the
real renderer later — the first three return plain scalar strength (color folded in only once,
per-channel, in `flashContribution`) specifically so the chromatic-aberration re-sampling works.

`presets/` holds found looks worth keeping as a reference point. Each is the exact JSON produced by
the tool's own "Export settings" button (top of the control rail) — paste it back into the
`params` object in a browser console, or into the `PRESETS` map in the HTML, to reload it exactly.

- `seemusic-found.json` — closest match found so far to the SeeMusic edge in `../../sm-ex.png`,
  from back when this lab still had electric filament/wisp controls (since removed as unused
  experiments); only its wavy-edge/strand-bundle fields still apply to the current tool.
