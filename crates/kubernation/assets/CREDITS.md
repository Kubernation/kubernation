# Bundled asset credits

- `fonts/FiraSans-*.ttf` — Fira Sans by The Mozilla Foundation and
  Telefónica S.A., licensed under the **SIL Open Font License 1.1**
  (see `fonts/OFL.txt`).
- `fonts/LiberationSerif-Bold.ttf` — Liberation Serif (v2.1.5) by Red Hat,
  Inc. (digitized data © Google), licensed under the **SIL Open Font
  License 1.1** (see `fonts/OFL.txt`). Used for the map's place-name banners.
- `fonts/LiberationMono-Regular.ttf` — Liberation Mono by Red Hat, Inc.
  (digitized data © Google), licensed under the **SIL Open Font License 1.1**
  (see `fonts/OFL.txt`). Used for the log overlay (fixed-width so timestamps
  and columns align).

- `logo/mark.png`, `logo/full.png` — KuberNation's own logos (the compass
  mark and the "KuberNation" scene), downsized from the originals at the repo
  root for the window icon, top-bar emblem, and fog-screen splash.

The world map is rendered with original procedural geometry (isometric
diamonds, settlements, terrain) — no sprite assets are bundled.

## Third-party crate licenses

The shipped binary links many open-source Rust crates — mostly **MIT** /
**Apache-2.0**, with some **ISC** (the rustls/`ring` TLS stack), **BSD-3-Clause**,
**Zlib**, and **Unicode-3.0** components. The full per-crate license texts are in
`crates/kubernation/THIRD-PARTY-NOTICES.md`, generated from the dependency tree by
[`cargo-about`](https://github.com/EmbarkStudios/cargo-about) (`cargo about
generate about.hbs -o crates/kubernation/THIRD-PARTY-NOTICES.md`).
