# Barrier FX Lab

Standalone WebGL2 shader playground for exploring barrier looks (glow sigmas, wavy edge, electric
filaments/wisps) before committing any of it to `crates/render/src/barrier.wgsl`/`barrier.rs`.
Not wired into the app or build in any way — open `barrier-fx-lab.html` directly in a browser, no
build step, no server needed. Ported line-for-line from `barrier.wgsl`'s noise/glow math where the
two overlap (see the file's own comments), so slider values here mean the same thing they will in
the real barrier renderer.

Also includes a "Flash" section, for the separate barrier-hit flash effect
(`crates/render/src/effects.rs`/`effects.wgsl`, `project::FlashSpec`), aimed at a "photograph of
the sun from Earth" look, split into groups:

- **Bloom / glow** (`flashEnabled`) is today's real flash exactly — a fixed elliptical 3-layer
  additive corona, no more, no less. With everything else in this section disabled/off, this alone
  is today's app look.
- **God rays (volumetric)** (`godRaysEnabled`) is a handful of much wider and longer beams than a
  typical starburst. Sits on fixed evenly-spaced slots — no angular wander (an earlier iteration had
  the beams wander side to side, which read as wiggling rather than the intended look, so it was
  removed) — instead each beam's own *length* breathes in and out over time via noise
  (`godRayPulseSpeed`/`godRayPulseAmount`), like a corona ray visibly extending and retracting, each
  on its own phase so they don't pulse in lockstep. On top of that: an internal streak texture
  (`godRayStreakiness`, fixed scale/no drift — see `GOD_RAY_NOISE_SCALE` in the shader source) and a
  separate whole-beam brightness flicker (`godRayFlickerSpeed`/`godRayFlickerIntensity`).
  `godRayLength` goes down to 5px so the whole flash can be pulled tight into a small sun-like disc
  rather than a diffuse blob. The beam-shape exponent (`GOD_RAY_TAPER`) is likewise fixed rather than
  a slider, once the target look settled on a value.
- **Halo ring** (`ringRadius`/`ringWidth`/`ringIntensity`) is a small additional diffraction-halo
  accent at a fixed radius — what's left of an earlier, larger "lens flare (rays)" section once
  everything but the halo was cut as unneeded for this look.
- **Chromatic aberration** (`chromaticEnabled`/`chromaticAmount`) re-evaluates the entire light
  stack once per color channel at a slightly different radius, the same "error grows with distance
  from center" shape real lens dispersion has, rather than a flat color tint.

See the "Flash: app default" preset for today's exact look and "Flash: photoreal sunburst" for the
dialed-in target look (exported straight from the tool's own "Export settings" button).
`flashCoreStrength`/`flashGodRayStrength`/`flashRingStrength`/`flashTotalStrength`/
`flashContribution` in the shader source are the relevant functions if porting any of this into the
real renderer later — the first three return plain scalar strength (color folded in only once,
per-channel, in `flashContribution`) specifically so the chromatic-aberration re-sampling works.

`presets/` holds found looks worth keeping as a reference point. Each is the exact JSON produced by
the tool's own "Export settings" button (top of the control rail) — paste it back into the
`params` object in a browser console, or into the `PRESETS` map in the HTML, to reload it exactly.

- `seemusic-found.json` — closest match found so far to the SeeMusic electric/wispy edge in
  `../../sm-ex.png`.
