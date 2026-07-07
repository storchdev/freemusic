# freemusic

A native desktop app (Rust, `winit` + `wgpu` + `egui`) for making those Synthesia-style
piano-cover videos you see on YouTube and TikTok, with a real UI instead of manual video editing.
Cross-platform (Windows/macOS/Linux), and a free alternative to tools like SeeMusic.

## Features

- Load a video file and a MIDI file, and preview them composited together in real time
- Manual audio/MIDI sync with keyboard-driven fine calibration
- Video transforms: brightness, scale, crop, rotate, tilt, translate
- A "barrier" and note-highway renderer with an extensible `.fmstyle.ron` visual style format
  (gradients, sheen, glow, particles, per-key colors, wavy barrier, and more — see
  `docs/fmstyle-format.md`)
- Synced audio playback of the loaded video's own audio track during preview
- Native Open/Save dialogs, a File menu, keyboard shortcuts, and project files (`.fmproj.ron`)
  that save/restore the whole session
- Offline MP4 export (video + audio) of the composited result

> **Note:** the in-app UI only exposes a limited subset of what `.fmstyle.ron` can actually do —
> full styling control (gradients, sheen, glow, particles, per-key colors, custom shaders, etc.)
> currently requires hand-writing or generating a `.fmstyle.ron` file yourself and loading it via
> the Project tab's "Import style…" button (or a CLI arg — see below). A practical way to do this
> without learning the format by hand: hand `docs/fmstyle-format.md` (the field-by-field spec) to
> an LLM and ask it to generate a `.ron` file for the look you want. Broader UI support for
> `.fmstyle.ron` is on the roadmap below.

## Building

There are two different things "building this project" can mean:

- **Developing/running it yourself**: dynamically link against an FFmpeg you already have (or
  download) — fast to compile, no third-party build tools beyond the FFmpeg package itself. The
  resulting binary only runs on machines with those same FFmpeg libraries/DLLs available.
- **Producing a standalone release binary**: vendor and statically link FFmpeg + libx264 so the
  binary runs standalone on any machine, with no FFmpeg install step for whoever downloads it. If
  you just want a working `.exe`/binary and aren't modifying the code, download one from
  [Releases](../../releases) instead of building anything at all. Otherwise, see
  **[`docs/building.md`](docs/building.md)** for the Windows dev setup, the static-linking
  gotchas, the `scripts/build-static-*` helper scripts, and how the GitHub Releases binaries get
  built.

System dependencies for development (not vendored):

- FFmpeg dev libraries (`libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`)
  plus `clang`/`llvm` (needed by `ffmpeg-sys-next`'s bindgen step)
- A Vulkan loader and driver (or another `wgpu`-supported backend)
- `libxkbcommon-x11` if running under X11 (native Wayland doesn't need it)
- **Windows** has no package manager for these — see `docs/building.md` for the setup there.

```sh
cargo build --release
cargo run --bin app -- [video-file] [midi-file]                      # both args optional; drag-and-drop also works
cargo run --bin app -- project.fmproj.ron                            # or open a saved project directly
cargo run --bin app -- project.fmproj.ron mystyle.fmstyle.ron        # or open a project and a style file
```

See `CLAUDE.md` and `docs/` for architecture, verification, implementation notes, and the phased
build history behind each subsystem.

## Roadmap

Ideas being considered for future work, roughly grouped:

**`.fmstyle.ron` (visual style format)**
- Y-level-dependent note styles
- Flashes/particles that match note color
- Alpha (transparency) on notes
- Custom note textures and background textures, both compatible with note alpha — alpha would
  let a note "see through" into a static background
- Octave lines
- Reflectivity settings, for a metal-bar look
- Key-property-based styles (e.g. driven by pitch or velocity)
- Custom shaders

**UI**
- Better slider input/dragging mechanics
- More export options
- Broader `.fmstyle.ron` feature support in the UI

**End-to-end**
- Multiple styles within the same timeline (would require reworking the timeline UI and the
  `.fmstyle.ron` pipeline)

## License

This project is licensed under the GNU General Public License v3.0 (GPL-3.0-only) — see
[`LICENSE`](LICENSE).

### Third-party code and licenses

- `crates/render` depends on [`midi-file`](https://github.com/PolyMeilex/Neothesia) and
  [`piano-layout`](https://github.com/PolyMeilex/Neothesia), pinned git dependencies from the
  [Neothesia](https://github.com/PolyMeilex/Neothesia) project (GPL-3.0), by PolyMeilex and
  contributors.
- `crates/mp4-encoder` is a fork of Neothesia's own `ffmpeg-encoder` crate (GPL-3.0), adapted here
  with a parameterized frame rate, explicit codec selection, and optional audio muxing. See
  `docs/architecture.md` for the details of what changed.
- `crates/render`'s note-highway shader and rendering approach were originally based on
  Neothesia's vendored `neothesia-core` waterfall renderer before being rewritten in-tree; see
  `docs/fmstyle-milestone.md` for that history.
- All other dependencies are pulled from crates.io under their own published licenses (see
  `Cargo.lock` and each crate's own `Cargo.toml`/license file).

Neothesia is also GPL-3.0-licensed, so this project's own GPL-3.0-only license is compatible with
reusing and adapting its code. If you redistribute this project or a derivative of it, you must
keep the GPL-3.0 license and preserve copyright/attribution notices for the above third-party
code, per the terms in `LICENSE`.
