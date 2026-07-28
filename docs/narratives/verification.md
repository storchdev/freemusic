# Verification: what worked and gotchas found

Narrative companion to `docs/verification.md` — the specific mistakes and discoveries behind that
file's current procedures.

## The `gen-test-video.sh` marker pattern caught real bugs

`scripts/gen-test-video.sh`'s per-frame counter and multi-second keyframe interval (not just a
static test card) is what actually caught the reseek-every-frame and seek-timestamp-unit bugs
described in `docs/narratives/architecture.md` — `cargo build` and `cargo clippy` were clean for
both, and a static test pattern wouldn't have shown whether frames were actually advancing.

## The 10-second test clip / late-first-note trap

A synthetic MIDI generator's throwaway script (see git history for milestone 2) paired with a short
test clip once looked like a broken note overlay. `/home/hs/midimaxxing/test.mid` (division=220
ticks/quarter, 120bpm) has its first note-on at tick 10193 (~23.2s in), so a 10-second test video
paired with it never overlapped any note content — visually indistinguishable from the overlay
being broken. This is why `gen-test-video.sh` defaults to 30 seconds, and why checking a real
`.mid`'s first note-on timing (or temporarily dragging the sync offset very negative to pull a late
note into an early video position) became the standard first check before assuming a rendering bug.

## Window-relative vs. absolute coordinates: the mistake that actually happened

`xdotool mousemove --window <id> x y` is window-relative; bare `xdotool mousemove x y` (no
`--window`) is absolute screen coordinates. Mixing the two doesn't error — it silently no-ops
wherever the pointer already was. This happened for real during milestone 4 testing: coordinates
were read off a window-relative screenshot crop but fed to bare `xdotool mousemove x y` calls, so
every slider drag did nothing with no error to indicate why. `scripts/click.sh`/`scripts/drag.sh`
were written specifically so this class of mistake requires actively bypassing them to make again.

A related false lead: a stuck mouse button left down from an earlier `xdotool mousedown` in the
same X session (never matched by a `mouseup`) can make a freshly-created window misinterpret the
first pointer motion as a continuation of an old drag, since X11 button state is server-side and
persists across separate process launches.

## Moving off WSL2 mid-milestone-6

Development moved off WSL2 partway through milestone 6 onto a native Arch/Hyprland machine. Most
of the WSL2-era screenshotting knowledge carried over unchanged (X11-backend forcing,
window-relative coordinates); what changed is documented as current fact in `docs/verification.md`
(xdotool not preinstalled, Hyprland's default tiling requiring float+resize before trusting
coordinates).

Handing click-and-eyeball-a-screenshot verification back to the user, rather than automating it
with `click.sh`/`drag.sh`, was an explicit ask during milestone 6 work — coordinate-based
automation against a tiling WM needed a fresh screenshot before nearly every click to re-derive
coordinates against shifting geometry, and didn't get more reliable with more scripting.

## Milestone 3-4: the drag-interaction/persistence verification pattern

Before the blanket "never run the app yourself" rule existed, this was verified directly: launch
via `scripts/run-app.sh` with the Bash tool's own `run_in_background: true` (a manual `cargo run
... &` + `disown` combined with `run_in_background: true` failed silently in practice — no log
file even created — so the script's own body stayed foreground and let the Bash tool do the
backgrounding), confirm the window exists via `scripts/find-window.sh` before proceeding (a
crash-on-launch otherwise just looks like "no window found" and is easy to misattribute to the
screenshot tooling), then drive one widget at a time with `click.sh`/`drag.sh` (internally
`mousemove`/`mousedown`/`mousemove`/`mouseup` as separate `xdotool` calls with a short `sleep`
between each, since a single combined call doesn't reliably register as a drag), reading results
back from the UI's own on-screen state (the "Sync & Project" window's live calibration-fraction
readout was added specifically to turn "did the drag work" from a pixel-measurement problem into a
text-reading one).

## Milestone 5: the MP4 export verification pattern

Also predates the "never run the app yourself" rule. Verified by driving the app to export (same
`run-app.sh`/`click.sh`/`drag.sh` pattern as above) for both the no-audio and with-audio paths,
then inspecting the output file with `ffprobe`/`ffmpeg` rather than trusting that the app simply
didn't crash — the only way to confirm the composited footage and the falling-notes overlay both
actually made it into the encoded file, versus e.g. silently exporting a blank or video-only frame.

## Static build troubleshooting history

`docs/building.md` has the current build instructions; `docs/narratives/building.md` has the full
historical failure-mode narrative behind the scripts and vendored FFmpeg patches — including the
static-`libx264`-vs-shared-lib trap, the two MSVC `ffmpeg-sys-next` build-script bugs, the x264
architecture-mismatch saga, and the CI-specific MSYS2/PowerShell/pkg-config hazards.

## Preview color management history

The current sRGB-encoding split between the interactive preview and export (`manual_srgb_encode`)
is described as current behavior in `docs/architecture.md`; the bugs that led to that design —
"interactive preview darker than mpv/iOS" and "exported notes/barrier/particles washed out" — are
in `docs/narratives/architecture.md`.
