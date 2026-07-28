# Narratives

This folder holds the "what worked / what didn't / gotchas found / bugs postmortems / design
decisions" narrative for this project — the story behind why the code looks the way it does.
Everything else under `docs/` (and `CLAUDE.md`, and `README.md`) describes the code and software as
it is right now, with no development-history framing; when a change is worth explaining to a future
reader but isn't itself a fact about current behavior, it belongs in here instead, in whichever file
already owns that area (or a new one, added to the list below).

Each file here is the narrative companion to a same-named file in `docs/` — read the `docs/*.md`
file first for what the system currently does, then its `docs/narratives/*.md` counterpart for why
it ended up that way.

- **`architecture.md`** — companion to `docs/architecture.md`. Neothesia-reuse history before the
  note pipeline was vendored in-tree, the video-transform rotation-matrix sign bug, the
  bind-group-layout-visibility panic, the crop-box preview overlay that was built and then removed,
  the hybrid-core scheduling bug behind mouse-move playback lag, the reseek and decode catch-up
  bugs, the video/preview/export darkness (sRGB) bugs, the two-render-pass split, the
  unthrottled-redraw perf bug, barrier/glow architectural history, and the note-activity
  duration-floor bug.
- **`ui-milestones.md`** — companion to `docs/ui.md`. The milestone-by-milestone history (6a–6e)
  of the UI restructure, barrier/note-highway styling, the File-menu/native-dialogs milestone,
  keyboard navigation, synced audio playback, the timeline/waveform polish pass, the note editor,
  the Style tab (full in-app `.fmstyle.ron` editing and the `Option<Style>` → `Style` unification
  behind it), and the bugs found building each one.
- **`fmstyle-milestone.md`** — companion to `docs/fmstyle-format.md`. The full phase-by-phase
  (Phase A–Y) development narrative of the `.fmstyle.ron` visual style format and its renderer.
- **`fmstyle-history.md`** — also a companion to `docs/fmstyle-format.md`. Design history and
  bug-fix postmortems (the black-key gradient bug, the three-generation glow/brightness redesign,
  the retracted per-pixel sheen sample, the rejected god-ray wander), plus the canonical
  breaking-change migration log for hand-migrating an old `.fmstyle.ron` file across schema
  changes.
- **`verification.md`** — companion to `docs/verification.md`. The specific mistakes and
  discoveries behind the current verification procedures (the WSL2/Hyprland screenshotting
  gotchas, the window-relative-vs-absolute-coordinates mistake, the milestone 3–5 verification
  patterns from before the "never run the app yourself" rule existed).
- **`building.md`** — companion to `docs/building.md`. The FFmpeg-vendoring story (the
  `ffmpeg-next`/BtbN-builds incompatibility and the patches that fixed it), the two
  `ffmpeg-sys-next` MSVC build-script bugs, the static-`libx264` invariant, the Windows libx264
  architecture-mismatch saga, and the CI-specific MSYS2/PowerShell/pkg-config hazards.
