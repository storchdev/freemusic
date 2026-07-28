# Verifying changes

How to verify changes to `app`/`video-pipeline`/export. Per CLAUDE.md's top-level rule: never run
the app yourself — hand these steps to the user (running `ffprobe`/`ffmpeg` on an already-produced
output file is the one exception, since that's static-file analysis, not running the app).

## Verifying changes to `app` or `video-pipeline`

There is no automated test suite for playback/timing correctness, so changes to the decode or
render path need to be checked by running the app, not just by `cargo build` succeeding:

```sh
scripts/run-app.sh <video-file>   # or plain `cargo run --bin app -- <video-file>`
```

`scripts/run-app.sh` runs the app in the foreground of the script itself — background the script
invocation, not the app inside it, when the shell needs to stay free afterward.

For anything touching seek/playback timing, use a clip with a visible per-frame marker and a
multi-second keyframe interval rather than a static test pattern, since only a moving marker shows
whether frames are actually advancing:

```sh
scripts/gen-test-video.sh out.mp4 30   # wraps the ffmpeg testsrc+drawtext command below
```

```sh
ffmpeg -y -f lavfi -i "testsrc=size=640x360:rate=30:duration=30" \
  -vf "drawtext=fontfile=/usr/share/fonts/TTF/DejaVuSans-Bold.ttf:text=frame\ %{n}:fontcolor=white:fontsize=64:x=20:y=20:box=1:boxcolor=black@0.6" \
  -c:v libx264 -g 60 -pix_fmt yuv420p out.mp4
```

For a synthetic MIDI file to pair with it, any minimal single-track SMF works — a short script
writing raw MIDI bytes by hand is enough.

`gen-test-video.sh`'s default duration is 30 seconds because a real `.mid` file's first note can
land well into the file — check where a `.mid`'s first note-on actually falls (`midi_time =
position - offset`, controlled by the sync offset) before assuming "no notes visible" against a
short test clip is a rendering bug rather than a timeline-overlap issue.

### Screenshotting the app under WSL2

WSLg's Weston compositor does not support the `wlr-screencopy` protocol, so `grim` fails against
the default Wayland display. Force the X11 backend instead (`scripts/run-app.sh`, or `env -u
WAYLAND_DISPLAY DISPLAY=:0 cargo run --bin app -- ...` directly); once running on X11/XWayland,
`scripts/find-window.sh` finds the window id, `scripts/screenshot.sh` captures it (optionally
cropped), and `scripts/click.sh`/`scripts/drag.sh` drive it — all four resolve the window
themselves and take coordinates relative to it.

`scripts/click.sh`/`scripts/drag.sh` issue `xdotool` calls scoped to the target window
(`--window <id>`); a bare `xdotool mousemove x y` with no `--window` is absolute screen
coordinates and silently no-ops against window-relative coordinates instead of erroring, so prefer
the wrapper scripts over hand-rolled `xdotool` invocations. `xdotool mouseup 1 2 3` clears any
stuck mouse-button state left over from an earlier session (X11 button state is server-side, not
per-process).

Fallback if X11-backend forcing doesn't work: WSLg also surfaces each app window as a native
Windows window, so PowerShell interop from WSL can capture it — `Get-Process | Where-Object
MainWindowTitle -like "freemusic*"` finds the window (title is `"freemusic (<distro>)"`), then
`user32.dll`'s `SetForegroundWindow`/`GetWindowRect` + `System.Drawing.Graphics.CopyFromScreen`
captures it to a PNG; `System.Windows.Forms.SendKeys` can drive keyboard input the same way.
Invoke via `powershell.exe -NoProfile -ExecutionPolicy Bypass -File <script>.ps1 <out-path>`,
converting WSL paths with `wslpath -w` first.

### Screenshotting/driving the app under native Hyprland (current dev machine, not WSL2)

The WSL2 section above still applies (X11-backend forcing, window-relative coordinates).
`DISPLAY=:0` is a real Xwayland server here too. `xdotool` is not preinstalled (`sudo pacman -S
xdotool` if missing).

Hyprland tiles new windows by default, so a freshly launched window's geometry is whatever the
tiling layout assigns rather than the size the app requested. Float and resize it before trusting
screenshot coordinates: find the window's address with `hyprctl clients -j` (match on `.title ==
"freemusic"` — `.class` is just `"app"`), then `hyprctl dispatch focuswindow "address:<addr>"` ->
`togglefloating "address:<addr>"` -> `resizewindowpixel "exact <W> <H>,address:<addr>"` ->
`centerwindow`. The resize can apply asynchronously, so re-check the screenshot's own reported
dimensions (or re-query `hyprctl clients -j`) before trusting coordinates computed from it.

Prefer asking the user to drive interactive verification themselves (clicking tabs, dragging
calibration/crop handles, exercising playback) over automating it with `click.sh`/`drag.sh` —
coordinate-based automation against a tiling WM needs a fresh screenshot before nearly every click
to re-derive coordinates. Building, launching (`scripts/run-app.sh`, backgrounded), and killing
(`scripts/kill-app.sh`) the app are still fine to do directly; it's the click-and-eyeball-a-
screenshot loop to hand off.

### Verifying interactive drag/persistence changes

For changes to calibration handles, sync-offset dragging, transform sliders/crop handles, or
save/load, screenshot pixel-diffing isn't the right check. Ask the user to:

1. Launch the app and drive one widget at a time (`scripts/click.sh`/`scripts/drag.sh`,
   window-relative coordinates), screenshotting after each step.
2. Read the result back from the UI's own on-screen state rather than pixel positions — e.g. the
   "Sync & Project" window prints the live calibration fraction and sync offset value directly, so
   screenshot and read those numbers instead of measuring a handle's pixel position.
3. For save/load: click Save, read the resulting `.ron` file directly to confirm the written
   values, then change the in-app state and click Load, confirming the readout reverts.
4. Any `.mid` file already on the machine is fine to pair with this kind of test — exact note
   content doesn't matter for calibration/offset/persistence checks.

### Verifying MP4 export

Export needs a produced video file inspected, not just "the app didn't crash" or a screenshot
comparison. Ask the user to:

1. Drive the app to type an output path into the Export window and click Export; a screenshot
   confirms the progress bar appears and advances.
2. Test both the no-audio and with-audio export paths (`with_audio` gates whether `mp4-encoder`
   creates an audio stream at all) — pair with a clip that has an actual audio track (e.g. `ffmpeg
   -f lavfi -i testsrc=... -f lavfi -i "sine=frequency=440" -c:v libx264 -c:a aac -shortest ...`).
3. Once the user hands back the output file (or its path), verify it directly: `ffprobe` for
   codec/duration/fps/dimensions, then extract a mid-timeline frame (`ffmpeg -i out.mp4 -vf
   "select=eq(n\,N)" -vframes 1 frame.png`) and read the PNG to confirm both the composited
   footage and the falling-notes overlay made it into the encoded file.
