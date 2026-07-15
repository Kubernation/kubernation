# third_party — vendored, patched dependencies

## miniquad (0.4.10 + 2 macOS patches)

An exact copy of the `miniquad 0.4.10` crates.io source (MIT / Apache-2.0 —
license files included), wired in via `[patch.crates-io]` in the workspace root
`Cargo.toml`, carrying **two targeted macOS patches** (both marked
`[KuberNation patch N/2]` in `src/native/macos.rs`; nothing else is modified):

1. **`backingScaleFactor = 0` guard** — backported verbatim from upstream
   master commit `14c6fc31` (unreleased). `NSWindow.backingScaleFactor` returns
   `0.0` while the window is not attached to a screen (startup, and transiently
   during display/zoom transitions on multi-monitor setups); recording
   `dpi_scale = 0` propagates 0×0 / NaN dimensions downstream. The guard keeps
   the previous valid scale until a real value lands.
2. **Skip ALL GL work during interactive live-resize** — Apple's deprecated
   GL-on-Metal layer (`AppleMetalOpenGLRenderer`) can `EXC_BAD_ACCESS` when an
   app draws mid-live-resize on macOS 26 + Apple Silicon
   (liballeg/allegro5#1749 documents a guaranteed crash there), and miniquad's
   GL `drawRect:` runs a full app frame from the resize tracking loop — exactly
   that trigger. While `inLiveResize`, the patch skips the whole `drawRect:`
   body AND the `windowDidResize:` dimension refresh — both run
   `[gl_context update]` (a drawable reallocation, GL churn of the same class)
   — so the last-presented surface stays intact and the compositor stretches it
   for the duration of the drag (deterministic; skipping only the draw would
   leave a resized-but-never-presented drawable — implementation-defined
   visuals). Dimensions + content refresh on the first main-loop frame after
   the drag ends (that path's `update_dimensions` is unconditional). Scoped to
   `inLiveResize` — the fullscreen / zoom animations keep painting. Accepted
   side effects: the first post-drag frame sees one large frame-time delta
   (cosmetic — per-frame lerps snap), and miniquad's unused-here
   `blocking_event_loop` mode would defer the post-drag repaint (KuberNation
   never enables it).

**Why vendored:** macroquad (0.4.15, latest) pins `miniquad = "=0.4.10"` — an
exact-version requirement — so no released upgrade path can deliver either fix
(0.4.11, the only newer release, has a byte-identical macOS backend). A
`[patch.crates-io]` path override satisfies the `=0.4.10` pin while carrying
the patches. Context: the v0.73.0 "silent crash on maximize" investigation
(see the CLAUDE.md decision log).

One metadata-only tweak besides the two source patches: a `[lints.rust]
dead_code = "allow"` in the vendored `Cargo.toml` — path dependencies compile
with full lints (registry deps get `--cap-lints allow`), which surfaced
upstream's own pre-existing dead code as build noise.

**Drop this directory** (and the `[patch.crates-io]` section) when either
lands upstream in a release macroquad accepts.
