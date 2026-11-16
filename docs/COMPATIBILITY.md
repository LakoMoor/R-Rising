# Compatibility status — 2026-11-16

Target: V Rising v1.1.13.0-r99712-b17 (202605251526), native desktop and browser.

The goal is a compatible runtime that loads the owner's local game content. No
original assets, executables or decompiled sources should be distributed.

| Boundary | Implementation | Runtime connection | Evidence | Remaining acceptance |
| --- | --- | --- | --- | --- |
| Archive / object readers | Present | Native showcase | 4,409 archives, 188,063 indexed objects; real-content tests | Catalog read and wired into native startup; request scheduling remains |
| Entity scene format | Container, archetypes, chunks, masks, buffers, shared components, blobs | Reader tests only | 9 scene tests passed against local install | Build runtime world and resolve components |
| Math | Broad API coverage | Showcase transforms | 58 regression/unit tests passed | Original-runtime differential oracle; rounding deviations in PORT.md |
| ECS | Primitive types and layout | Readers; no running game world | 9 primitive tests passed | Entity allocation, queries, system scheduling |
| Rendering | Basic lit meshes | Native showcase | GPU triangle test; screenshots/native-content.png | Original scenes, animation, HDRP and visual comparisons |
| Gameplay / HUD / physics / sound / networking / saves | Not implemented | None | No end-to-end parity cases | First original-scene playable slice |
| Browser | Shared renderer + async WASM host | Local exported scene import | Chrome / Apple Metal-3: 48 objects, zero JS/WebGPU errors | Direct archive import, mobile texture fallback, game world |
| Performance | Unmeasured | None | No comparable original-runtime benchmark | Same-scene release measurements on both runtimes |

## Metrics

`progress.json` is reproducible with `python3 tools/port_coverage.py --json`.
It measures heuristic **member name matches** only. It can credit unrelated
identifiers and does not prove overload coverage, correctness, or runtime wiring.
Behavioral parity and overall game completion are unknown, not 2.7%.
There are no full-game original-runtime golden scenarios. Two original native leaf functions now have 1,024 executed input/output cases each (see evidence/original_leaf_oracles.md). Reader tests and GPU
regression tests must not be presented as proof of 1:1 compatibility.

## Validation in this session

- `cargo test --workspace --locked`: 82 non-GPU tests passed; the GPU test failed
  in the sandbox because Metal exposed no adapter. Its rerun outside the sandbox passed
  (1 GPU test); total: 83 passing tests across these runs.
- `cargo check --workspace --locked`: passed after the app changes.
- Original assets were available locally; install tests actually ran.
- Offscreen renderer outside sandbox: 1600 × 900, 48 objects, 26 textures,
  610,085 non-background pixels. This is a showcase grid, not original scene placement.
- `cargo check --workspace --target wasm32-unknown-unknown --locked`: failed in
  transitive `primase-foundations` Oodle backend (`libloading::Library` unavailable).
- Windows, Linux, Android, iOS and browser execution have not been verified.

## Publication

The owner confirmed rights-holder permission on 2026-11-16. GitHub publication
contains the independent runtime, format evidence and screenshots; original game
files, decompiled sources and exported scene data are excluded.

The workspace declares MIT OR Apache-2.0, but unity-formats and
primase-foundations declare GPL-3.0-or-later. License texts and dependency notices are included in THIRD_PARTY.md.
The native combined distribution must comply with the GPL dependencies.


## Browser milestone — 2026-11-16

The native-only parser dependency remains incompatible with wasm32. Rather than
patching or replacing its DLL loader with a stub, the renderer's Unity adapters
are now behind the default `unity` feature. `rrising-web` disables that feature
and uses a versioned local scene transfer. This is a real-content rendering slice,
not direct archive loading or game simulation.

- `cargo build --locked -p rrising-web --target wasm32-unknown-unknown`: passed.
- Native export indexed 188,063 objects from all 4,409 archives and exported 48
  meshes with transforms and material base textures to a local 35 MiB JSON file.
- Chrome headless with `--enable-unsafe-webgpu` on Apple Metal-3 loaded all 48
  objects. Captured page errors and console errors: **0**. Screenshot in
  `screenshots/browser-webgpu.png`. This is a development browser test, not a
  guarantee about default browser settings or other hardware.
- Native output after adapter separation was byte-identical to the earlier
  screenshot (`cmp docs/screenshots/native-content.png /private/tmp/native-current.png`).
- Browser surface uses an sRGB view to match native shader output conversion.
- Full native workspace test run passed (83 tests). Additional scene-transfer
  regression compares GPU output after JSON round-trip with direct GPU upload.
- Known inherited orientation limitation: compressed texture upload reverses
  block rows but not texel rows inside each BC block. This is not image parity
  with the original and remains to be fixed with reference pixel evidence.

All-device compatibility and performance improvement remain goals, not results.

- `cargo test -p rrising-render --locked`: 2 GPU tests passed, including JSON
  round-trip equivalence and invalid-index/version rejection. Total across the
  full run plus this added regression: 84 tests.
- Real exported 48-object scene re-imported by native offscreen path and produced
  a byte-identical PNG to direct loading (`cmp` passed).
- Strict Clippy remains blocked by pre-existing math generator lints:
  `needless_range_loop` at build.rs:78 and `too_many_arguments` at build.rs:717.
  The check was attempted and is not reported as passing.


## Catalog and chunk metadata milestone

- `cargo test --locked -p rrising-content --test catalog --test scene_runtime -- --include-ignored --nocapture`: 4 tests passed, including explicit local-install checks.
- Catalog: 4,410 archive records including a NULL sentinel, 4,409 physical archives,
  4,410 file records, 1,151 object locations, 1,904 dependency sets / 55,316 indices.
  All paths and cross references validated. Hash integrity and original request
  scheduling are not verified.
- Native open_content now reads archive order from the original catalog; malformed
  or missing referenced archives are errors. Installations lacking a catalog retain
  explicit logged scan fallback. Full archive indexing still yields 188,063 objects.
- All 1,183 EntityScenes loaded chunk metadata into ArchetypeChunkData: 31,723
  chunks, 1,385,671 entities, 648,652 enable masks. Growth and swap-back preserve
  original masks and disabled counts. This is metadata restoration, not gameplay.
- Corrected historical descriptions: implicit archetype slot 0 is Entity, not
  SharedComponentData. Buffer external allocations use Capacity × ElementSize;
  the old text calling that Length was inaccurate.
