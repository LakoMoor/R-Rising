# Dependency and content notices

The original Rust code declares MIT OR Apache-2.0 (see LICENSE-MIT and LICENSE-APACHE).
This does not relicense dependencies or original V Rising content.

- `unity-formats` 0.5.0 and `primase-foundations` 0.3.6 are
  GPL-3.0-or-later. The native application links them; distribution of the combined
  work must comply with GPL-3.0-or-later. See LICENSE-GPL-3.0 and Cargo.lock.
- `wgpu` and `winit` use MIT OR Apache-2.0. Other dependency license metadata is
  available with `cargo metadata --locked` and each registry package's license files.
- `assets/typetrees.tpk` was generated from AssetRipper's Unity 2022.3.58f1
  InfoJson type description (per the existing evidence ledger). It describes
  serialized formats; it is not a game archive. Preserve upstream provenance.
- V Rising belongs to Stunlock Studios. An installed game is required; original
  game archives, executables, metadata dumps, decompiled code and local exported
  scenes are not included in this repository.
- Branding images in docs/branding were generated for this independent project
  with the built-in ImageGen tool; see PROMPTS.md. They are not the official game logo.
