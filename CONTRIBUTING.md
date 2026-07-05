# Contributing

Thanks for your interest in contributing.

## Getting set up

Read `CLAUDE.md` first — it's the fastest way to get oriented on the codebase, the workspace
layout, and where the deeper design docs (`docs/`) live for each subsystem.

```sh
cargo build
cargo run --bin app -- [video-file] [midi-file]
```

System dependencies (FFmpeg dev libraries, a Vulkan driver, etc.) are listed in the README and in
`CLAUDE.md`'s "System dependencies" section.

## Before submitting a change

```sh
cargo fmt
cargo clippy --all-targets
```

The repo is kept `fmt`-clean and `clippy`-clean; please run both before opening a PR, or just run
`scripts/check.sh`, which does both.

There is no automated test suite for UI/rendering/timing behavior — this is intentional (see
`CLAUDE.md`). Changes to `app`, `video-pipeline`, `render`, or `export` should be verified
manually; `docs/verification.md` describes the patterns used historically (synthetic test clips,
screenshotting, drag-interaction checks, MP4 export checks). Describe how you tested a change in
your PR description.

## Licensing and third-party code

This project is GPL-3.0-only (see `LICENSE`). By contributing, you agree that your contributions
are licensed under the same terms.

Some code in this repository is adapted from or depends on the GPL-3.0-licensed
[Neothesia](https://github.com/PolyMeilex/Neothesia) project — see the README's "Third-party code
and licenses" section for specifics. If you bring in code from another project:

- Make sure its license is compatible with GPL-3.0 (most permissive licenses and other GPL-3.0
  code are fine; GPL-incompatible licenses, e.g. GPL-2.0-only or many "no derivatives"/
  non-commercial licenses, are not).
- Note the origin and license of the borrowed code in your PR description and in a comment or doc
  near the code itself, so attribution isn't lost.
- Don't copy in code you don't have the rights to relicense under GPL-3.0.

## Pull requests

- Keep PRs focused — one logical change per PR is easier to review than a bundle of unrelated
  fixes.
- Update the relevant doc in `docs/` (or `CLAUDE.md` for anything project-wide) alongside your
  code change if it affects architecture, data flow, or a gotcha future contributors should know
  about. These docs are meant to stay in sync with what the code actually does.
- Use clear, descriptive commit messages explaining *why* a change was made, not just what
  changed.
