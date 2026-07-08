# Barrier FX Lab

Standalone WebGL2 shader playground for exploring barrier looks (glow sigmas, wavy edge, electric
filaments/wisps) before committing any of it to `crates/render/src/barrier.wgsl`/`barrier.rs`.
Not wired into the app or build in any way — open `barrier-fx-lab.html` directly in a browser, no
build step, no server needed. Ported line-for-line from `barrier.wgsl`'s noise/glow math where the
two overlap (see the file's own comments), so slider values here mean the same thing they will in
the real barrier renderer.

`presets/` holds found looks worth keeping as a reference point. Each is the exact JSON produced by
the tool's own "Export settings" button (top of the control rail) — paste it back into the
`params` object in a browser console, or into the `PRESETS` map in the HTML, to reload it exactly.

- `seemusic-found.json` — closest match found so far to the SeeMusic electric/wispy edge in
  `../../sm-ex.png`.
