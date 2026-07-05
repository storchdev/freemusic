# Verifying changes

How to verify changes to `app`/`video-pipeline`/export, split out of CLAUDE.md. Per CLAUDE.md's
top-level rule: never run the app yourself — these are patterns to hand to the user, or (for
static file analysis like ffprobe on an export output) safe to run directly.

## Verifying changes to `app` or `video-pipeline`

> Per the "never run the app yourself" rule above: don't execute the commands in this section
> yourself. Ask the user to run them and report back what they observed or a teed log file.

Since there's no test suite for playback/timing correctness, changes to the decode or render path
should be checked by actually running the app, not just by `cargo build` succeeding:

```sh
scripts/run-app.sh <video-file>   # or plain `cargo run --bin app -- <video-file>`
```

For anything touching seek/playback timing specifically, a static test pattern isn't enough to
tell whether frames are actually advancing — generate a clip with a visible per-frame marker and a
realistic (multi-second) keyframe interval, e.g. `scripts/gen-test-video.sh out.mp4 30` (wraps the
exact command below):

```sh
ffmpeg -y -f lavfi -i "testsrc=size=640x360:rate=30:duration=30" \
  -vf "drawtext=fontfile=/usr/share/fonts/TTF/DejaVuSans-Bold.ttf:text=frame\ %{n}:fontcolor=white:fontsize=64:x=20:y=20:box=1:boxcolor=black@0.6" \
  -c:v libx264 -g 60 -pix_fmt yuv420p out.mp4
```

This is how the reseek-every-frame and seek-timestamp-units bugs above were actually caught —
`cargo build` and `cargo clippy` were clean for both.

For a synthetic MIDI file to pair with the above (no note-visualizer-specific tooling needed —
any minimal single-track SMF works), a short Python script writing raw MIDI bytes is enough; see
git history for milestone 2's throwaway generator if a reference is useful.

**Real `.mid` files can have their first note far into the file** — if reusing an existing
performance recording (rather than a purpose-built synthetic MIDI) to test the note overlay,
check where its first note-on actually lands before assuming "no notes visible" is a rendering
bug. This is a real trap: `/home/hs/midimaxxing/test.mid` (division=220 ticks/quarter, 120bpm)
has its first note-on at tick 10193 ≈ 23.2s in, so a 10-second test video paired with it never
overlaps any note content at all — visually indistinguishable from the overlay being broken.
Fixed by either using a longer test clip (`gen-test-video.sh`'s default duration is 30s
specifically because of this) or temporarily dragging the sync offset very negative
(`midi_time = position - offset`, so a large negative offset pulls a late note into an early
video position) to confirm the overlay itself does render once the timeline actually overlaps.

### Screenshotting the app under WSL2

WSLg's Weston compositor does **not** support the `wlr-screencopy` protocol, so `grim` fails with
"compositor doesn't support the screen capture protocol" against the default `WAYLAND_DISPLAY`.

**Simplest working method**: force the X11 backend by launching with `WAYLAND_DISPLAY` unset
(`scripts/run-app.sh`, or `env -u WAYLAND_DISPLAY DISPLAY=:0 cargo run --bin app -- ...`
directly; see the `libxkbcommon-x11` dependency note above for why this fallback exists at
all). Once running on X11/XWayland, the window shows up for standard tools:
`scripts/find-window.sh` finds the window id, `scripts/screenshot.sh` captures it (optionally
cropped), and `scripts/click.sh`/`scripts/drag.sh` drive it — all four resolve the window
themselves and operate in coordinates *relative to it*, which matters (see below). Don't bother
with the default Wayland-backend run for screenshotting — that window isn't visible to any X11
tool.

**Window-relative vs. absolute screen coordinates — the mistake that actually happened**:
`xdotool mousemove --window <id> x y` moves relative to the target window, but plain
`xdotool mousemove x y` (no `--window`) is *absolute screen coordinates*. Mixing the two doesn't
error — it just silently no-ops, since the click/drag lands wherever the pointer physically
already was (often outside the window, or over a completely different widget). This actually
happened during milestone 4 testing: coordinates were read off a `screenshot.sh`-style
window-relative crop, but then fed to bare `xdotool mousemove x y` calls, and every single
slider drag did nothing with no error to indicate why. `scripts/click.sh`/`scripts/drag.sh`
exist specifically so this class of mistake requires actively bypassing them to make again —
prefer those over hand-rolled `xdotool` invocations.

A related false lead when a widget doesn't seem to respond: check for a **stuck mouse button**
left down from an earlier `xdotool mousedown` in the same X session that was never matched by a
`mouseup` (X11 button state persists across separate process launches, since it's server-side,
not per-app) — this can make freshly-created windows misinterpret the first pointer motion as a
continuation of an old drag. `xdotool mouseup 1` (and `2`, `3`) unconditionally clears it.

Fallback if X11-backend forcing ever stops working: WSLg surfaces each app window as a *native
Windows window* too, so PowerShell interop from WSL can find and capture it —
`Get-Process | Where-Object MainWindowTitle -like "freemusic*"` finds the window (title is
`"freemusic (<distro>)"`), then `user32.dll`'s `SetForegroundWindow`/`GetWindowRect` +
`System.Drawing.Graphics.CopyFromScreen` over that rect captures it to a PNG. Invoke via
`powershell.exe -NoProfile -ExecutionPolicy Bypass -File <script>.ps1 <out-path>`, converting WSL
paths to Windows paths with `wslpath -w` first. `System.Windows.Forms.SendKeys` from the same
script can drive keyboard input (e.g. Space to toggle play) after focusing the window.

### Screenshotting/driving the app under native Hyprland (current dev machine, not WSL2)

Development moved off WSL2 partway through milestone 6 onto a native Arch/Hyprland (Wayland)
machine. Most of the WSL2 section above still applies unchanged (the X11-backend-forcing trick
in `run-app.sh`, window-relative-vs-absolute coordinates), but a few things differ:

- `DISPLAY=:0` is a real Xwayland server here too (Hyprland spawns one for XWayland app
  compatibility, running as `Xwayland :0 -rootless ...`), so nothing about `run-app.sh` needed to
  change — `find-window.sh`/`screenshot.sh`/`click.sh`/`drag.sh` all keep working as documented
  above.
- `xdotool` is **not preinstalled** on this machine's base image (unlike whatever WSL2 image had
  it already) — `sudo pacman -S xdotool` if `which xdotool` comes back empty.
- **Hyprland tiles new windows by default**, so a freshly launched app window's geometry is
  whatever the tiling layout assigns it (observed: a non-16:9 shape like 760×922 logical), not
  the `1280×720` `with_inner_size` the app actually requested — this makes coordinates computed
  from a screenshot unreliable until the window is floated. Find the window's address (`hyprctl
  clients -j`, match on `.title == "freemusic"` — its `.class` is just `"app"`, the binary name,
  not useful for matching), then `hyprctl dispatch focuswindow "address:<addr>"` →
  `togglefloating "address:<addr>"` → `resizewindowpixel "exact <W> <H>,address:<addr>"` →
  `centerwindow`. Even then, the resize can be applied asynchronously — a screenshot taken
  immediately after dispatching it can still capture the pre-resize size, so re-check the
  screenshot's own reported dimensions (or re-query `hyprctl clients -j`) before trusting
  coordinates computed from it, rather than assuming the size you just requested took effect.
- **Prefer asking the user to drive interactive verification themselves** (clicking tabs,
  dragging calibration/crop handles, exercising playback) rather than automating it with
  `click.sh`/`drag.sh` — this was an explicit ask during milestone 6 work, and matches the
  environment reality above: coordinate-based automation against a tiling WM is slow (every
  click needs a fresh screenshot to re-derive coordinates against, since geometry can shift
  between screenshots) and doesn't obviously get more reliable with more scripting. Building,
  launching (`scripts/run-app.sh`, backgrounded), and killing (`scripts/kill-app.sh`) the app are
  still squarely useful to do directly — it's specifically the click-and-eyeball-a-screenshot
  testing loop to hand off.

### Verifying drag interactions and persistence (milestones 3–4 pattern)

> Historical record of how this was verified pre-dating the "never run the app yourself" rule
> above. Don't execute steps 1-2 yourself anymore — hand the whole sequence to the user and ask
> them to report back the readouts step 3 describes (or a screenshot).

Milestone 3's actual content — calibration handles, sync-offset drag value, save/load — and
milestone 4's (transform sliders, crop handles) are interaction rather than rendering, so
pixel-diffing screenshots isn't the right check. The pattern that worked, in order:

1. Launch via `scripts/run-app.sh`, invoked through the Bash tool's `run_in_background: true`
   rather than foreground — you need the shell free afterward to fire the other scripts at the
   still-running process. The script itself must **not** self-background with `&`/`disown`; a
   manual `cargo run ... &` + `disown` combined with `run_in_background: true` failed silently in
   practice (no log file even created) — let the Bash tool's own backgrounding do that job, keep
   the script's own body foreground. Confirm the process is alive and the window exists
   (`scripts/find-window.sh`) before proceeding, since a crash-on-launch otherwise just looks
   like "no window found" and is easy to misattribute to the screenshot tooling instead.
2. Drive one widget at a time, screenshotting after each: `scripts/click.sh x y` for a button;
   `scripts/drag.sh x1 y1 x2 y2` for a drag (calibration/crop handles, sliders, DragValues) —
   internally this issues `mousemove`/`mousedown`/`mousemove`/`mouseup` as separate `xdotool`
   invocations with a short `sleep` between each, since a single combined
   mousedown/move/mouseup call doesn't reliably register as a drag. Remember these take
   **window-relative**, not absolute, coordinates (see above).
3. **Read back the result from the UI's own on-screen state, not pixel positions.** The "Sync &
   Project" window prints the live calibration fraction (`"Keyboard: 0%–71% of width"`) and the
   sync offset value directly — screenshot and eyeball those numbers rather than trying to
   measure a handle's pixel position or infer a note lane's on-screen offset. This is why that
   readout label was worth adding to the UI in the first place: it turns "did the drag work" from
   a pixel-measurement problem into a text-reading one.
4. For save/load specifically: click Save, `cat` the resulting `.ron` file directly (fastest way
   to confirm the written values, no screenshot needed), then deliberately change the in-app state
   (Reset calibration button, drag the offset elsewhere) and click Load, confirming the readout
   reverts. Changing *fewer* than all fields before reloading (e.g. only resetting calibration,
   leaving the offset alone) is still a valid check as long as at least one field demonstrably
   round-trips — it doesn't have to be a from-scratch process restart.
5. A real `.mid` file already on the machine is fine to point the CLI arg at for this kind of
   test — the exact notes/timing don't matter for verifying calibration/offset/persistence
   plumbing, unlike milestone 2's overlay-rendering checks where note content mattered.

### Verifying MP4 export (milestone 5 pattern)

> Historical record predating the "never run the app yourself" rule above. Ask the user to
> perform steps 1-2 and hand you the resulting file (or its path); step 3's `ffprobe`/`ffmpeg`
> inspection of the *output file* is fine for you to run yourself — that's static-file analysis,
> not running the app.

Export is neither a pure rendering change (screenshot-comparable) nor a simple UI-state
round-trip (readable off an on-screen label) — the thing that actually needs checking is a
video *file*, so the verification loop is different from milestones 2-4's:

1. Drive the app exactly as in the drag-interaction pattern above (`run-app.sh` backgrounded,
   `click.sh`/`drag.sh` against window-relative coordinates) to type an output path into the
   "Export" window's text field and click "Export"; screenshot to confirm the progress bar
   appears and advances across a couple of redraws (`export_run.is_some()` keeps the app
   redrawing on its own — no need to nudge playback).
2. Test both the no-audio and with-audio paths explicitly — they're genuinely different code
   paths (`with_audio` gates whether `mp4-encoder` creates an audio stream at all), and
   `scripts/gen-test-video.sh`'s own clips are video-only, so they alone won't exercise audio
   muxing. Generate a second clip with a tone (`ffmpeg -f lavfi -i testsrc=... -f lavfi -i
   "sine=frequency=440" -c:v libx264 -c:a aac -shortest ...`) to cover that path.
3. **Verify the output file directly, not just that the app didn't crash**: `ffprobe` for
   codec/duration/fps/dimensions, then extract a single mid-timeline frame
   (`ffmpeg -i out.mp4 -vf "select=eq(n\,N)" -vframes 1 frame.png`) and read the PNG — this is
   the only way to confirm the composited footage *and* the falling-notes overlay both actually
   made it into the encoded file, versus e.g. silently exporting a blank or video-only frame.
