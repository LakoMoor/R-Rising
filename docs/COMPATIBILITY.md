# Compatibility status — updated 2026-11-18

Target: V Rising v1.1.13.0-r99712-b17 (202605251526), native desktop and browser.

The goal is a compatible runtime that loads the owner's local game content. No
original assets, executables or decompiled sources should be distributed.

| Boundary | Implementation | Runtime connection | Evidence | Remaining acceptance |
| --- | --- | --- | --- | --- |
| Archive / object readers | Present | Native showcase | 4,409 archives, 188,063 indexed objects; real-content tests | Catalog read and wired into native startup; request scheduling remains |
| Entity scene format | Container, archetypes, chunks, masks, buffers, shared components, blobs | `world_adapter` builds one `World` per scene (opaque special components) | 9 scene tests + `world_adapter` test: all 1,183 scenes, 1,385,671 entities with preserved ids | Shared indices preserve scene-local chunk groups; unmanaged serialized values are retained as raw bytes, while managed objects and chunk components remain unmodelled; entity-reference remap is identity-validated (0 dangling), not native-replayed |
| Math | Broad API coverage | Scene transforms and transform system | 23 scalar and 8 matrix/quaternion replay targets, 2,048 inputs each; normalization uses a sqrt stub | Remaining overloads, SIMD approximations and transcendental rounding deviations in PORT.md |
| ECS | Primitive types and layout; EntityComponentStore entity table (alloc/free, versions, free list), create/destroy in chunks, minimal include/exclude query (default Disabled/Prefab exclusion matches `IsMatchingArchetype` @ 0x90f46c0 by decompiled-body reading; other option gates not ported), `World::build` from plain chunk/archetype input with preserved entity ids/versions (`SpecialTypes::new`; the old all-zero Default aliasing is gone) | Scene Worlds, deterministic schedule and TransformSystem; `--scene-sim` feeds viewer/export | Store tests derived from decompiled bodies; native replay for pure getters, comparisons and change versions; 372 real-scene transform scenarios | Shared/managed/chunk/cleanup archetypes, meta chunks, buffer heap, EntityQueryData parity, original system scheduling |
| Rendering | Basic lit meshes, sub-meshes and cutout pipeline | Original-scene viewer, optional transform-system output | GPU regressions; native/WebGPU screenshots; stored-vs-simulated Rust image comparison | Animation, HDRP, transparent materials, terrain blending and original-game visual parity |
| Saves | Container, complete header, jobs and partial ECS restoration | `--inspect-save --restore-projection` without GPU startup | Two CrossOver autosaves, all eight jobs across the two saves, 49 restored tag types with query counts checked, including 262,167 Disabled rows per save | Remaining component types and values, full world and system restoration, original-runtime replay |
| Gameplay / HUD / physics / sound / networking | Experimental movement and static action-bar frames; other systems not implemented | Native player diagnostic only | Original prefab frame references and GPU alpha test; no end-to-end parity cases | Game-state HUD, interactions, original system scheduling and playable slice |
| Browser | Shared renderer + async WASM host | Local exported scene import, experimental WASD and sampled walk/rest poses | Chrome 158 / Windows: input, movement, stop and pose switching tested; no original-game comparison | Direct archive import, collision, full Animator, gameplay, physical mobile-device verification |
| Performance | Rust-only absolute timings (see Performance measurement) | None | No comparable original-runtime benchmark | Same-scene release measurements on both runtimes |

## Metrics

`progress.json` is reproducible with `python3 tools/port_coverage.py --json`.
It measures heuristic **member name matches** only. It can credit unrelated
identifiers and does not prove overload coverage, correctness, or runtime wiring.
Behavioral parity and overall game completion are still unmeasured.
There are no full-game original-runtime golden scenarios. Original-function replays execute
x86-64 instructions under Rosetta: 7 earlier captures with 1,024 cases each, 87 captures with
2,048 cases each, one waypoint capture with 4,096 cases, and shared steering plus its normalizer
with 4,096 cases each, plus the aim-prediction pair and three small helpers (IsInSun, GetProgress, SoundVolumetric Lerp;
4,096 cases each). Two overlapping TypeIndex getters leave **100 distinct addresses** and 218,112
evaluations (before the easing capture; see `evidence/original_leaf_oracles.md` for the current 119 / 257,024). Matrix capture links original bodies and stubs sqrt; steering additionally
isolates five ECS accessors behind supplied-value callbacks and preinitializes metadata.
This does not run the original scheduler. Goldens are in `evidence/oracles/`; see
[original_leaf_oracles.md](../evidence/original_leaf_oracles.md). The replay found and fixed
integer `clamp` with lo > hi, `abs(i32::MIN)`, `Archetype.HasChunkComponents`, wrapping change
versions and the waypoint NaN branch. Reader tests and GPU
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
- Platform status (verified locally on an Apple Silicon macOS host, 2026-11-17, Rust 1.99.0):
  - Executed: macOS aarch64 only (build, tests, GPU via Metal). Browser execution is covered by the subsequent browser milestone.
  - Type-checked only, not linked or run: `cargo check --locked --workspace --target` passed for x86_64-apple-darwin, x86_64-pc-windows-msvc and x86_64-unknown-linux-gnu. This proves the code compiles for those targets, not that it runs there.
  - `cargo build --locked -p rrising-web --target wasm32-unknown-unknown` passed. The whole-workspace wasm check still fails in `primase-foundations` (see above), because `rrising-content` is native-only.
  - Run in CI (`.github/workflows/ci.yml`, GitHub run 44058518223, commit c33dd90, all green): fmt, clippy `-D warnings` and `cargo test` on ubuntu, windows and macOS runners, plus the wasm build. The first CI run failed because some original-data tests had a hard-coded local path; they now read `VRISING_DATA` and skip when no install exists, so CI green says nothing about original-content behaviour. The two GPU tests (`draws_a_hand_built_triangle`, `serialized_transfer_matches_direct_gpu_upload`) are skipped there, so GPU rendering is not covered by CI on any platform. Ignored original-data tests are not run in CI.
  - Android and iOS: not verified.

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
- Compressed texture upload now reverses block rows and texel rows inside
  BC1/BC3/BC4/BC5 blocks (synthetic-block unit tests). BC6H/BC7 blocks are not
  row-separable and still reverse block rows only; no reference-pixel comparison
  against the original has been done. native-content.png re-rendered after the
  change (the earlier byte-identity statement above applies to the prior build).

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


## Offline type layout and component access

`python3 tools/probe_type_registry.py --check --output /private/tmp/rrising-type-layouts.json`
reads the pinned IL2CPP metadata v31 and native registration. It derives storage
shapes for all 1,772 stable hashes observed in scenes (the last 64 were identity-only
until the managed-class and generic-instance sections below), matching type names and managed field
sizes/offsets. `native_size` is a marshalled size, not the ECS managed size (a
bool-backed structure can marshal to four bytes but occupy one byte in ECS).

Read-only access through `type_layouts::TypeLayouts` uses recovered storage
inputs and the original archetype allocation algorithm. It refuses unknown
hashes and checks entity uniqueness, ranges, buffer headers and external patches
without ever dereferencing serialized pointers. Remaining TypeInfo flags,
write groups, managed objects and generic types are not reconstructed. Successful
boundary checks do not establish arbitrary component semantics or runtime parity.

Acceptance against all 1,183 scene files:

```sh
RRISING_LAYOUT_REPORT=/private/tmp/rrising-type-layouts.json \
VRISING_DATA=/path/to/VRising_Data \
cargo test --locked -p rrising-content --test type_layouts -- --include-ignored --nocapture
```

Result: 3,615 admitted archetypes, 6,542 chunks, 422,447 entities, 34,750 buffer
headers, 5,362 external buffer patches and 6,252 chunk transform-component reads.
Unresolved archetypes are counted separately; they are never assigned fallback
layouts. No transform has yet been wired to the game's rendering references.

This check exposed and fixed two further archetype bugs against the shipped
CreateArchetype body: all TypeIndex flag bits participate in memory ordering
(RVA 0x8fb18c8, mask `0xffffc000`); an aligned payload of exactly 32,704 bytes is
valid (branch at 0x8fb15db..0x8fb1641 checks `< 0x7fc1`, not `< 0x7fc0`). Both
have focused regression tests in `rrising-ecs/tests/archetype_storage.rs`.

## Render component indexing

Ported the 12-byte Unity.Rendering.MaterialMeshInfo component's static-index
conversions, submesh word, range marker and runtime-vs-array classification.
Five native leaf functions match 1,024 original byte replays each (seven leaf
functions now have replay evidence in total). Range construction is statically
recovered, not replay-verified. The inspector reports material/mesh records when
admitted; the present admitted scene-layout subset has none, because rendering
archetypes still included unresolved shared/managed types. (Superseded: with every archetype
admitted, the scene acceptance below reads 809,820 MaterialMeshInfo records; they are only
byte-range checked, and no mesh binding is wired.)


## Managed shared-array type identities

Extended TypeHash's terminal pointer/array name handling and retained unknown
field types explicitly. The pinned probe now matches **1,495 / 1,772** scene type
hashes, including `Unity.Rendering.ManagedRenderMeshArray` (`f423fa42b77d7330`)
and `ProjectM.Terrain.PerChunkFluffDataComponent` (`a26e40a1d5cc937c`). Both are
managed shared components: their arrays live outside chunk payloads. The layout
reader admits their zero-byte shared slots but does not deserialize managed
arrays or treat their fields as host pointers. The admitted chunk/transform set
remains unchanged: this is added identity evidence, not new mesh binding coverage.
`--check` now pins both identities and their shared/managed classifications.

## Generic, explicit-layout and managed-class identities

All **1,772 / 1,772** scene type hashes are now identified by name
(`tools/probe_generic_identities.py`, shared with `tools/probe_type_registry.py`).
The 279 formerly unresolved hashes split into: explicit-layout structs, whose
`FieldOffset` is hashed from IL2CPP's own field-offset table (the metadata carries
no attribute for it), and fields typed as constructed generics (native kind 0x15).
Of these, 1,708 had a recovered storage shape from metadata alone; 64 were identity-only
(61 managed `IComponentData` classes, `ProjectM.MountModelStreaming`, and two
`Rukhanka.NewBlobAssetDatabaseRecord<T>` instantiations), which kept 7,539 archetypes /
7,895 chunks refused until the section below.

## Managed components, IEquatable ordering bit, constructed generics

All 1,772 / 1,772 scene hashes now carry a storage shape (`tools/probe_type_registry.py
--check` pins 1,772 matches, 0 identity-only), and **all 17,579 archetypes / 31,723 chunks of
all 1,183 scenes** resolve and validate (original CreateArchetype algorithm, entity
uniqueness, buffer headers, external patches): 1,385,671 entities, 274,197 buffer headers,
89,120 external patches.

- **Managed `IComponentData` class (61 types).** `TypeManager.BuildComponentType`
  (RVA 0x905a560) has a managed-class branch at 0x905afa7: after
  `CheckIsAllowedAsManagedComponentData` it stores `sizeInChunk = 4` (`mov [rbp-0x51], 4`),
  category 0 (ComponentData), bufferCapacity -1, alignment 0 and `elementSize = max(4, 0)`;
  the zero-size bit (30) is only set when `sizeInChunk == 0`. The slot is a 4-byte
  managed-store index; the object lives outside the chunk. Only 4 passes the scene
  acceptance; slot sizes 1, 2, 8 and 16 each fail in the first scenes (offsets of every
  later type shift). An earlier 4-byte experiment diverged in four archetypes; that was not a
  slot-size problem (see next item).
  `type_layouts` admits a managed per-entity component only with size = element size = 4,
  and `component_bytes` returns `None` for them (the slot order among several managed types
  depends on entity-reference flag bits that offline analysis cannot derive for classes, so
  the index of a particular managed type is not claimed). No managed object is deserialized.
- **IEquatable ordering bit (the real cause of the 4 divergent archetypes).** Every TypeIndex
  flag bit orders memory. BuildComponentType sets bit 23 (`IsIEquatable`, `bts r14d,0x17`) from
  `GetInterfaces().Any(<lambda at 0x9065a80>)`, i.e. any `IEquatable<>` instance. The layout
  reader omitted it, so `ProjectM.ProfessorCoilPairElement` (a 144-byte buffer) sorted before
  16-byte buffers, whereas real chunk bytes (headers at 25,280 / 26,048 / 26,752 / 27,136
  in the first failing chunk) put it last. The probe now emits `iequatable` (23 types; Entity,
  registered through the special path with ordering 0, is excluded) and the reader sets the bit.
- **Constructed generics.** Metadata bakes no layout for generic instances, so
  `Identities.sequential_layout` applies the C# sequential/explicit rule to the constructed
  field types (cross-checked on 1,704 non-generic structs against IL2CPP's baked sizes and
  Entity offsets: 1,700 agree; the 4 that differ are explicit-layout types the calculator is
  not used for). Results: `MountModelStreaming` = 28 bytes, align 4, Entity at offset 16
  (`AssetGuid` 16 + `ReferenceToken<Entity>` 12); each `NewBlobAssetDatabaseRecord<T>` =
  `Hash128` 16 + `BlobAssetReference<T>` 8 = 24, inline capacity 5, 136 chunk bytes.
  These were not taken on faith: the scene acceptance rejects the neighbours (Mount size
  24 or 32; Rukhanka capacity 4/6 or element 8/16/32 all fail on buffer/pointer checks
  in the first scene using them), and only the derived values pass.

Limits: the acceptance proves the slot sizes, ordering and buffer geometry of every scene
archetype, not component semantics. The Entity references inside managed objects and blob
asset pointers are neither patched nor dereferenced. Per-type managed slot attribution among
several managed types, write groups and refcount/serialization flags remain unreconstructed.

## Scene to World adapter

`rrising_content::world_adapter` converts every archetype/chunk of a decoded scene into
`rrising_ecs::WorldInput` (ids preserved, component bytes sliced through `TypeLayouts`) and builds
a `World` per scene. Acceptance (`tests/world_adapter.rs`, ignored, needs the install and layout
report): all 1,183 scenes build, 1,385,671 entities are live at their stored ids. Entities with
Translation / LocalTransform / LocalToWorld (including disabled and prefab): 348,846 / 111,272 /
919,206 (counts of entities having the type, no semantic validation). Release build time for all
worlds: about 3.4 s on the Apple M4 (single run, warm cache, excludes scene decode and layout resolution).

Limits, stated plainly: the store refused archetypes with shared/managed/chunk/cleanup components,
so `EntityComponentStore::set_admit_opaque_special_components` / `World::build_with` add an opt-in
*opaque* mode (default still refuses; existing tests unchanged). It keeps the original layout
algorithm but does not model shared values (no `ChunkListMap`: entities with different shared values
share chunks), chunk-component storage (no meta chunks), managed objects or the cleanup archetype.
Managed 4-byte slots and buffer headers are copied as raw serialized bytes: slots are not valid
managed-store handles and buffers have no heap payload. The store re-packs entities, so it holds
27,497 chunks versus the scene's 31,723.

Entity references (`rrising_ecs::remap`, `World::resolve`, `World::check_hierarchy`). Field
locations come from the recovered layouts' `entity_offsets` (368 of 1,772 types have at least one;
the same data `CreateArchetype` turns into `EntityPatchInfo` / `BufferEntityPatchInfo`). The remap
rule is Unity's `RemapEntity`: table indexed by source index holding `(SourceVersion, Target)`;
version match gives the target, otherwise `Entity.Null`. **Provenance: derived from the documented
Unity semantics and data invariants, not re-read from the pinned `GameAssembly`**: the REA server
timed out in this session, so no body of `RemapEntity` was inspected and there is no native replay.
The adapter registers every stored id as its own target (ids are preserved, so the remap is the
identity) and patches per-entity component fields. Buffer elements are only classified (no heap
payload exists in the `World`); elements come from the scene's buffer patches or the inline bytes.
Measured over all 1,183 scenes (`entity_references_resolve_and_parent_chains_are_acyclic`, pinned):

| Where | Null | Resolves to a live entity in the same scene | Stale version | Dangling |
| --- | --- | --- | --- | --- |
| Per-entity component fields | 155,086 | 141,778 | 0 | 0 |
| Buffer elements | 10,723 | 1,491,308 | 0 | 0 |
| of which `Parent.Value` | 0 | 36,200 | 0 | 0 |

Every non-null reference names an entity stored in the same scene with its stored version, so the
serialized form already equals the live id space and the patch rewrites no bytes; the 0 dangling
and 0 stale counts are the evidence (this does not prove the original's table is an identity, only
that for these scenes it cannot differ). All 36,200 `Parent.Value` targets are live after the world
is built and the parent chains are acyclic, maximum depth 18. Not covered: Entity fields inside
managed components, blob assets and shared-component values (not modelled), and cross-scene
references (none observed, so none are explained or tested). Pointer-typed or `UnsafeList` fields
holding entities are not in `entity_offsets`.

## Performance measurement (Rust only)

Reproduce: `cargo run --locked --release -p rrising-app -- --data <VRising_Data> --bench --archives 100000 --frames 60`.
Measured 2026-11-17 on an Apple M4 (25.8 GB RAM), macOS 27.2.1, Rust 1.99.0 release build, wgpu/Metal, single run:

| Stage | Result |
| --- | --- |
| RuntimeContentCatalog load | 0.002 s (4,409 physical archives) |
| Archive indexing | 3.32 s (4,409 archives, 188,063 objects) |
| Decode of all scenes (container, chunks, archetypes, buffers, enabled bits, shared components, blob assets) | 1.31 s (1,183 / 1,183 fully decoded, 31,723 chunks, 2,198 MiB) |
| Offscreen frame, 1280x720, 24-item showcase | median 1.42 ms, min 1.28 ms over 60 frames (includes GPU readback; warm-up frame excluded) |

These are absolute numbers for the Rust port alone. **There is no original-game comparison**, so
nothing here shows the port is faster or slower than V Rising. An original comparison is not
currently feasible: the original has no equivalent headless "index/decode" entry points to time, its
frame time depends on a full world (not implemented here), and the showcase scene is not a scene the
original renders. A meaningful comparison needs the same scene on both runtimes with GPU capture,
which requires the original-runtime scenarios that do not yet exist. Page cache state was not
controlled; indexing/decode times are warm-cache-dependent.


## Original scene placement (`--scene`)

`r-rising --scene <file.0.entities> [--offscreen out.png | --export-web out.json]` reads each
entity's serialized `LocalToWorld`, resolves `MaterialMeshInfo` static indices through the scene's
first managed shared component (`ManagedRenderMeshArray`: two int arrays of indices) into the scene's
`Unity.Scenes.ReferencedUnityObjects` MonoBehaviour (found via the runtime catalog, whose object
records carry the GUID embedded in the `.entityheader`), and draws the referenced `Mesh` with the
referenced `Material`'s base map. The layout of the managed array bytes (2 header bytes, `i32 n`,
n ints, 1 byte, `i32 m`, m ints) is observed, not taken from a reader port; it is validated by
index range and by each array resolving to `Mesh` / `Material` objects. Scenes with more than one
`ManagedRenderMeshArray` are refused for resolution (chunk-to-array mapping not recovered).

Measured on `c6d34432...` (Silver_Mountain01): 10,353 entities, 9,898 with LocalToWorld, 9,495 with
MaterialMeshInfo, 567 referenced objects, 226 unique meshes (94 textures), 9,492 instances drawn,
3 unresolved (mesh unreadable or skinned) drawn as marker boxes. Four other large scenes resolve
12,910 / 12,911, 10,410 / 10,414, 9,610 / 9,611 and 9,103 / 9,105 renderers the same way.
Not done: sub-mesh selection, shader/material state, transparency, lighting, LODs, skinned meshes,
non-renderer entities. Placement is original data; the picture is not a parity claim. The browser
export transfers it through the existing JSON format (version 1, optional instanced items and a
shared texture pool); Chrome / WebGPU loaded the 316 draw groups with 0 page errors. A native
re-import of the same JSON differs from direct loading by a few pixels (2 of 94 textures did not
export), so byte identity is not claimed for scene exports.

### Runtime loop to renderer (`--scene-sim N`, `--compare out.json`)

`--scene ... --scene-sim N` builds the scene `World` through `world_adapter`, runs the `Schedule`
(`TransformSystem`) for N fixed frames, reads each drawn entity's `LocalToWorld` back from the
`World` by its stored entity id, and draws those matrices instead of the stored ones (works with
`--offscreen`, windowed and `--export-web`). The old path stays the default. `--compare out.json`
renders both ways offscreen (same camera, same GPU data, 1280x720, 3 frames) and writes the JSON
plus stored/sim/diff PNGs. This compares original-stored transforms with Rust-simulated transforms;
it is NOT a comparison with the original game's frames.

| scene | instances | matrices differing (bits) | max matrix delta | pixels differing | max channel delta |
|---|---|---|---|---|---|
| Silver_Mountain01 `c6d34432` | 9,495 | 0 | 0 | 0 / 921,600 (0%) | 0 |
| `63f65516` | 12,911 | 0 | 0 | 0 (0%) | 0 |
| `153b7233` (17,114 instances, mostly marker boxes) | 17,114 | 4,287 | 3.4e-3 | 0 (0%) | 0 |

In a sweep of ~90 of the largest scenes no framed render differed by a single pixel; the largest
matrix deltas were sub-pixel at the framed distance (a close-up on the worst instance of `153b7233`
also showed 0 differing pixels, but that instance was not on screen, so it is weak evidence). Most
entities have no authoring source and keep their stored matrix, so the image test mostly confirms
the pipeline wiring and that the transform system does not disturb stored placements. The diff
code is covered by a unit test that proves it detects differing pixels.

Release timings (Apple GPU, offscreen render + readback, median of 5; schedule median over frames
after the first): Silver_Mountain01 world build 7.6 ms, schedule first frame 0.054 ms (289
entities written), steady 0.008 ms, render 2.7 ms stored / 2.6 ms sim; `63f65516` 13.5 ms, 0.089 ms,
0.011 ms, render 3.0 / 2.6 ms; `153b7233` 77 ms, 3.0 ms (15,906 written), 0.55 ms, render 5.2 / 5.4 ms.
The sim `--export-web` JSON of Silver_Mountain01 is byte-identical to the stored export, so the
earlier Chrome / WebGPU check applies unchanged; Chrome was not re-run.


## Transform golden comparison (first original-data scenario)

The scene files store both authoring transforms and the `LocalToWorld` the original's own transform
system produced, so `LocalToWorld` is original-produced golden data. `rrising_ecs::transform` computes
it (`float4x4` TRS / `mul` via `rrising-math`; `crates/rrising-content/tests/transform_golden.rs`,
`#[ignore]`, needs the pinned install and `RRISING_LAYOUT_REPORT`) and the test compares all 1,183
scenes. Measured on the pinned build: 1,385,671 entities, 919,206 with a stored `LocalToWorld`.

- Comparable: 112,177 (12.2%) have an authoring source. The other 807,029 have none: 793,493 are
  `Static` entities with only a baked `LocalToWorld`, and 13,536 are `Parent`+`StaticChild` entities
  without any local transform. These are not compared.
- Component set in this build: `LocalTransform`, `Translation`, `Rotation`, uniform `Scale`,
  `PostTransformMatrix`, `Parent`, `Static`, `StaticChild`. There is no `NonUniformScale`,
  `CompositeScale` or `LocalToParent`, so scale types beyond uniform are not exercised.
- Source rule found by measurement: where `Translation`/`Rotation` exist they drive the matrix
  (`LocalTransform` is a stale twin for 174 of 111,272 entities that have both); scale is
  `Scale`, else `LocalTransform.Scale`; `Translate * (Rotate * Scale)` reproduces the stored `+0.0`
  where `float4x4.TRS` yields `-0.0`.
- Result, child step computed from the parent's stored `LocalToWorld` (the isolated step): 86,798 of
  112,177 bit-identical (88,677 treating +-0 as equal), 111,382 (99.3%) within 1e-5 (relative to
  `max(|v|,1)`), 767 off by more than 1e-3, max abs error 198.5. Recomputing parents up the chain
  instead gives 75,333 bit-identical and 111,368 within 1e-5 (rounding compounds).
- By category (step): root `LocalTransform+Translation+Rotation` 56,159 / 58,597 bit-identical, all
  within 3.6e-7; with `Parent` 15,246 / 21,732 identical, all within 7.6e-6; with `Parent` and
  non-unit `Scale` 61 / 885 identical, 883 within 1e-5; `PostTransformMatrix` roots only 2,070 /
  14,318 identical (98.6% within 1e-5, 80 over 1e-3).
- Mismatch causes: (1) the large errors (>1e-3) are concentrated in `Static` entities (671 of 767),
  whose stored matrix was baked; consistent with the authoring values having changed after the
  bake (not verified), and not a pattern a different formula fixed. (2) A ~1% tail of unit-scale root entities differs by 1 ulp for
  reasons not recovered: fused multiply-add forms of the rotation matrix, quaternion
  normalization and the multiplication association were tried and none improved the match.
  (3) The stored `PostTransformMatrix` product keeps translation and `w` exactly while a plain 4x4
  product does not, so the original's exact combination is not recovered; `mul(local, post)` is
  the right order (the reverse is far worse) but not bit-exact.
- Not covered: no `Unity.Transforms` body exists in the decompiled corpus and the REA session on
  the pinned `GameAssembly` timed out, so formulas are measured, not translated from code; the
  systems' scheduling, change-filtering and dirty-tracking; runtime-created entities; any entity
  without an authoring source (86% of stored `LocalToWorld`). This is agreement with stored original
  output on baked scene data, not a claim that the runtime system is ported.

#### Update: matrix/quaternion leaves replayed from the original

`tools/capture_math_matrix.py` replays eight original leaves under Rosetta (2,048 cases each,
`evidence/oracles/math_matrix.tsv`, inputs and outputs only): `float3x3(quaternion)`,
`float4x4(float3x3, float3)`-style `float4x4(quaternion, float3)`, `float4x4.TRS`, `Translate`,
`Scale(float3)`, `mul(float4x4, float4x4)`, `mul(quaternion, quaternion)` and
`normalize(quaternion)` (the il2cpp `sqrt(float)` wrapper, not a leaf, is stubbed by `sqrtss`; the
harness gained call-linking between captured functions and longer straight-line bodies). Inputs
include unit quaternions, axis rotations, negative zeros, non-unit and special values, scene-shaped
TRS matrices and general matrices. Rust diverged in two places, both fixed in `rrising-math`:
`float3x3(quaternion)` (original: `(a * -b - c * d) + k` per element, so `-0.0` becomes `+0.0`;
205 of 2,048 differed) and `quaternion * quaternion` (term order; 613 of 2,048 differed). `float4x4.TRS`
(which calls the former), `Translate`, `Scale`, `mul(float4x4, float4x4)` and `normalize(quaternion)`
are now bit-identical on all 2,048 cases (`crates/rrising-math/tests/oracle_matrix.rs`).

Effect on the golden comparison (isolated step): `trs_matrix` is now `float4x4.TRS`, which makes
the earlier `Translate * (Rotate * Scale)` workaround and the "`-0.0` vs stored `+0.0`" note obsolete.
Bit-identical 88,660 of 112,177 (was 86,798; 88,677 treating +-0 as equal, within 1e-5 111,382
unchanged, 767 over 1e-3 unchanged). Roots `LocalTransform+Translation+Rotation`: 58,021 of 58,597
bit-identical (was 56,159), all within 3.6e-7. Chain mode: 77,195 bit-identical (was 75,333).

Still unexplained: (1) 576 of those roots differ by 1 ulp or so from the stored value although
every involved leaf is now replayed bit-exact; a search over +-2 ulp neighbours of the stored
rotation recovers most of them, so the original likely rotated by a slightly different (e.g.
re-normalized or converted at bake time) quaternion than the serialized one, not a different
formula. (2) `Parent` children 15,246 of 21,732 and `PostTransformMatrix` roots 2,070 of 14,318
(even with an identity post matrix the product does not reproduce the stored bits) stay as before:
`mul` is exact, so the stored values come from a composition or inputs not recovered here, and
parent-with-scale remains weakest (61 of 885). (3) Entities without authoring source and
`Static` entities with baked mismatches are unchanged.

#### Update: first runtime loop (schedule and `TransformSystem` scenario)

`rrising_ecs::schedule` adds a deterministic `System` trait and `Schedule`: groups run in creation
order, systems inside a group in registration order refined by `update_after` constraints (stable
topological sort, ties by registration index; unknown names and cycles are errors), time advances in
whole fixed steps of integer nanoseconds (`run_frame`, or `advance` with an accumulator and a step
cap), and there is no hash-map iteration. Change versions follow the Unity convention: each system
remembers the `GlobalSystemVersion` it last ran at, a chunk "changed" for it when the component's
chunk change version is newer, its own writes are stamped with the version it runs at (so it does
not see them as changes next frame), and the global version is incremented after each system.
**Provenance: port-side design from the documented convention, not a translation of
`ComponentSystemGroup`/`SystemState`** (those bodies were not re-read). `EntityComponentStore` gained
chunk-level accessors (`chunk_component_bytes[_mut]`, `chunk_change_version`, `chunk_has_component`,
`try_component_bytes`, `increment_global_system_version`) and `World` gained `state_hash`,
`component_hash` and `component_versions` (FNV-1a 64 over component bytes; determinism
fingerprints, not original hashes).

`rrising_ecs::transform::TransformSystem` runs the measured transform step as a system. Roots (no
`Parent`) are processed per chunk and skipped when no input component's chunk version is newer than the
system's last run; children are ordered by hierarchy depth (parents before children, ties by query
order) and recomputed when their own inputs or their parent's `LocalToWorld` chunk changed. The matrix
is computed from authoring data only; a parent without an authoring source contributes the
`LocalToWorld` bytes currently in the `World` (the stored ones). Results are written into the
`LocalToWorld` component bytes. Entities without an authoring source are never written.

Scenario (`crates/rrising-content/tests/scene_scenario.rs`, ignored, needs the install and layout
report). **This is a data-based scenario on baked scene data, not execution of the original runtime**:
no original system code runs and the stored `LocalToWorld` is only a reference to compare with. For each
scene that has `LocalToWorld` (372 of 1,183; the other 811 contain no `LocalToWorld` at all, so the
system has nothing to write) it builds a `World` twice independently, snapshots the stored
`LocalToWorld`, then runs 1 and 3 fixed frames. Measured on the pinned build:

- Determinism: all 372 scenes give identical state hash, `LocalToWorld` hash and `LocalToWorld` change
  versions across the two independent builds, after frame 1 and after frame 3.
- Skip semantics: frame 1 wrote 89,513 root and 22,664 child matrices (112,177 total, 0 unresolved
  parents); frames 2 and 3 wrote 0 entities, left the state hash unchanged and bumped no
  `LocalToWorld` change version. The 807,029 entities with a stored `LocalToWorld` but no authoring
  source (all lie in these 372 scenes) were bit-identical to the snapshot afterwards (0 violations).
  Skipping on a later input change is covered by a synthetic test (`tests/schedule.rs`).
- Produced vs stored, hierarchy mode with computed parents, same tolerance as the golden test
  (relative to `max(|v|,1)`): 112,177 compared, 77,195 bit-identical (77,212 treating +-0 as equal),
  111,368 within 1e-5, 809 worse (781 over 1e-3, max relative error 7.07). These are exactly the golden
  test's "chain" numbers, so the system reproduces that composition; the causes of the remaining
  differences are the unchanged ones listed above (post-transform products, `Static` bake drift, 1 ulp tail).
- Frame time per scene world, release build, Apple M4, single run, scenes with `LocalToWorld` only,
  excluding scene decode and world build: frame 1 (everything computed) 29.5 microseconds mean, 11.0 ms
  total, worst 1.3 ms (`0eb9943b...`); a steady-state frame with nothing to do 7.3 microseconds mean,
  2.7 ms total. Single-threaded.

Limits: one system; no entity creation or destruction at runtime, no structural changes, no jobs or
parallelism; change filtering is per chunk (conservative); `NonUniformScale`/`CompositeScale` do not
exist in this build and are not handled; the frame-time numbers are for this tiny workload, not a
measure of the original game's simulation.

#### Update: Burst hypothesis tested, stored `LocalToWorld` is not reproducible from the stored inputs

Searched for the shipped transform job bodies: `evidence/decompiled` has no `Unity.Transforms` module (only
`LocalToWorld`/`LocalToParent`/`TransformHelpers` accessor call sites); `lib_burst_generated.dll`
(151 MB) is stripped of job names (strings show only `Unity.Transforms.PostTransformMa...` in a type
registration table, nothing for TRS/hierarchy/post-transform jobs), and the REA session on the large
binaries was not retried in depth, so no Burst body was read. Instead the hypothesis was tested on data
(112,177 comparable entities, temporary harness, nothing committed): fused multiply-add contraction of the
`float3x3(quaternion)` element (`a*b - c*d + k`, 7 contraction/association variants), of the
`float4x4` `mul` (8 variants including pairwise sums), different dot-product orders and several
quaternion normalizations (`q * (1/sqrt)`, `q / sqrt`, rsqrt estimate with a Newton step, fma dot).
None beats the current formulas: exact counts on the isolated step for roots without post
66,674 / 67,879 (best variant, `q / sqrt(fma dot)`, 66,734, within fitting noise and not adopted),
children without scale 15,246 / 21,732 (all fma `mul` variants lower, best 15,230), children with
scale 61 / 885 (no variant above 61), roots with post 5,612 / 20,602 (best 5,620). A double-precision
matrix from the f32 quaternion, with or without normalization, is worse (61,9xx).

Decisive observation: for roots with a pure Y rotation (`x = z = 0`) the stored sine lane differs by
exactly 1 ulp from `round(2*y*w)` in 370 of 76,282 cases, and the same happens with an identity
`PostTransformMatrix`. A single rounded product of the two serialized floats cannot produce another value
by any operation order, fma or normalization, so the stored `LocalToWorld` was not computed from the
stored `Rotation` bits. The stored matrices most plausibly come from the baker (the GameObject transform's
native matrix or a higher-precision source quaternion that was rounded to float afterwards), not from
the runtime TRS system, and so cannot be bit-reproduced from the scene's authoring components. The
rotation errors then also propagate into `Parent` children and scaled children, which explains why
those categories are weakest and why exact `mul` and TRS leaves do not close the gap. This is an
inference from the data; it is not verified against the baker. No formula or threshold changed; the
numbers above are the unchanged baseline (88,660 / 112,177 step, 77,195 chain, 111,368 within 1e-5).

### Original-scene viewer follow-up: sub-meshes, transparency, base maps, array mapping

Findings from the install and what changed in `--scene` (placement is original data; none of this is a
rendering-parity claim):

- **The white cylinder was a fog sheet, not a missing texture.** In Silver_Mountain01 (`c6d34432...`)
  the 18 instances of material `AbyssClouds` (queue 3400, `_SURFACE_TYPE_TRANSPARENT`) were drawn
  opaque, white-and-grey. Materials are now classified from stored facts only (`_SURFACE_TYPE_TRANSPARENT`
  keyword, `_SurfaceType` = 1, custom queue >= 3000); transparent ones are excluded and counted
  (`excluded: transparent`), not blended. Limit: a transparent material that relies only on its shader's
  default queue (not read) is still drawn opaque.
- **Alpha test is real and common.** 7,743 of 9,474 drawn instances in Silver_Mountain01 use
  `_ALPHATEST_ON` (rocks included). The renderer gained a second fragment entry (`fs_cutout`, a new
  `DrawCall::cutout` flag and an optional `cutout` field in the transfer JSON) that discards texels with
  base-map alpha < 0.5. The material's own `_AlphaCutoff` is not read.
- **Real white/grey patches: `Texture2DArray` base maps.** 1,691 instances (`GrassProjection`, `_BaseMaps`)
  failed to upload (`m_TextureFormat` absent; array formats are `GraphicsFormat` ids). The array is now
  decoded (BC formats only; the per-layer byte count must match the file exactly, which it does) and
  **layer 0** is shown, because no per-instance layer selector is decoded. Those instances are counted as
  "Texture2DArray base map shown as layer 0"; layer 0 is an assumption, not the original's choice. Any
  other base map that cannot be uploaded (for example a `Cubemap`, 9 instances in `153b7233...`) or a
  material with no base map is drawn with a flat 1x1 grey texture and counted, never the white neutral
  texture. BC6H/BC7 mips are still flipped by block row only, so detail inside 4x4 blocks is mirrored.
- **Sub-mesh selection** uses `MaterialMeshInfo` sub-mesh index (low 8 bits) against the mesh's own
  sub-mesh index ranges (`first_byte`, `index_count`, `base_vertex`); only triangle lists are drawn
  (others would be counted as `unusable sub-mesh`, 0 occurrences in the scenes tried). Multi-sub-mesh
  meshes get compacted per-sub-mesh vertex sets, which also keeps the browser transfer of
  `57768327...` at 210 MB (283 MB with whole-mesh copies, over the viewer's 256 MiB cap).
- **Chunk-to-array mapping.** `shared_component_remap` is one entry per shared (non-chunk-component)
  type of each chunk's archetype, in chunk-stream order; the entry count equals that sum in all 1,183
  scenes, and in all 13,286 `MaterialMeshInfo` chunks of the 347 scenes with a `ManagedRenderMeshArray`
  the chunk's values contain that array's serialized index (unmanaged components first, then managed, 1-based). The
  viewer now uses this per chunk, and locates each array in the managed blob by its observed layout
  instead of requiring it to be first (two scenes, `67c6a4f7...` and `969df757...`, have another managed
  record first and now resolve: 121 of 122 renderers in the former). **No scene in the install has more
  than one `ManagedRenderMeshArray`** (distribution: 0 arrays in 835 scenes, 1 in 348), so the
  multi-array branch is implemented but has never run on real data.
- Counts, before (whole meshes, no exclusion) -> after:
  - Silver_Mountain01: 9,492 drawn / 3 marker / 0 without base map / 0 excluded -> 9,474 drawn
    (2,056 on a sub-mesh of a multi-sub-mesh mesh) / 3 marker / 1,691 on a Texture2DArray layer 0 /
    0 grey fallback / 18 excluded transparent.
  - `57768327...` (10,414 renderers): 10,239 drawn (4,249 on a sub-mesh, 1,642 on array layer 0, 18
    with no base map -> grey) / 3 marker / 172 excluded transparent. Its "before" counts are the sum
    drawn + excluded; the before image was not rendered.
  - `153b7233...` (17,114 renderers): 16,930 drawn / 2 marker / 182 transparent excluded / 9 grey
    fallback / 2 array layer 0. Its 1%-99% framing is poor (scattered clusters), so it is not a showcase.
- Verified natively and by native re-import of the exported JSON for `57768327...`. **Chrome/WebGPU was
  not re-verified for this change** (the Chrome tools were unavailable in this session); the committed
  `browser-original-scene.png` predates it and is unchanged. Still not done: lighting, LODs, skinned
  meshes, real material parameters, terrain blending.

### 2026-11-17 continuation: transform World feeds the viewer

Reviewed Claude's committed work through `b9e6da0` and finished its uncommitted viewer integration.
`--scene-sim N` now builds the original scene World, runs N fixed transform frames, and reads
`LocalToWorld` back by the original entity handle for each mesh/marker instance. Windowed,
offscreen and browser-export paths use this integration. Missing readbacks are counted and retain
the stored matrix; this is explicit partial coverage. Zero frames and simulation without `--scene`
are rejected.

`--compare report.json` renders stored and simulated matrices with identical GPU resources and
camera, writes both PNGs and a pixel diff, and records matrix-bit differences, missing readbacks,
world-build, schedule and render-plus-readback timings. An extra close-up is produced when matrices
differ. Empty draw lists are errors: a blank image is not a successful compatibility test.
This is a Rust integration regression, **not an original-game screenshot comparison**.

Verified with the pinned install, all 4,409 archives, release, Apple M4 / Metal, 1280x720:
`c6d34432693f24046ae5916650cc70ef.0.entities` (Silver_Mountain01), 3 fixed frames,
9,477 instance readbacks (9,474 meshes and 3 unresolved markers), 0 missing, 0 differing matrix bits
and 0 differing pixels. The system wrote 289 entities on frame 1 and none on frame 3; this scene's
drawn instances had no matrix differences. The large `0eb9943b...` scene has no drawable instances
and is excluded from image evidence. Report: [scene-transform-render.json](verification/scene-transform-render.json).

Re-ran the original-data scenario: 372 scene Worlds, 112,177 computed transforms, 77,195 bit-exact,
111,368 within 1e-5, 0 untouched-entity violations, 0 writes on frames 2–3. These reproduce Claude's
recorded counts. Portable workspace tests: 129 passed, 7 ignored original-data tests; both GPU
regressions passed separately outside the GPU-restricted sandbox. `cargo fmt --check` and strict
workspace Clippy passed. Original-system execution and original-game performance remain unverified.

### 2026-11-17: original change-version semantics

`change_version.rs` implements `DidChange` (RVA 0x9072720) and
`IncrementGlobalSystemVersion` (0x9072730) from the pinned binary. Captured original instructions
ran before the implementation: 2,048 cases per function, no stubs, with zero, wrap-around and the
signed half-range. The previous unsigned comparison failed 315 cases; the new implementation
passes all of them. FrameContext delegates to this comparison and the store delegates its
increment to the shared helper. This fixes first-run handling of version zero and filtering
when the counter wraps. The transform-hierarchy regression verifies the actual consumer, not just
the helper. The scenario over 372 original scene Worlds still reproduces the prior exact counts.
No other original scheduling semantics are claimed by this fix.
Verification after the fix: 132 portable workspace tests passed (7 original-data tests ignored by
that command); the 372-scene scenario passed separately, with unchanged transform counts.
Formatting, strict workspace Clippy, WASM build and both generated-report comparisons passed.

### 2026-11-17: BC texture fallback and correct row orientation

`rrising-render` now uses the pinned MIT `bcdec_rs` 0.2.0 decoder (pure Rust, no new native
or Unity dependency in WASM). BC1/3/4/5 decode when BC support is absent. BC6H/BC7 always decode
before flipping texel rows: merely reversing block rows left details inside each block mirrored.
Chains with partial block rows (e.g. a 12x12 texture's 6x6 mip) also decode, as do non-block-sized
base levels. All mip dimensions, data lengths and arithmetic are checked before calling the decoder;
mip count, sRGB tagging and bottom-first source orientation are retained. Upload applies the final
Y flip once. BC4 stays R8, BC5 RG8; BC6H retains half floats and adds alpha 1 rather than clamping
HDR to 8-bit. Unity BC6H now maps to unsigned `Bc6hRgbUfloat`, as specified by the reader's enum;
the old mapping incorrectly selected signed BC6H. Re-export old scene transfers to correct that tag.

The fallback follows the original engine's documented fallback formats for BC6H/BC7
([Unity GPU format reference](https://docs.unity.com/en-us/engine/6000.0/manual/materials-and-shaders/textures/textures-getting-started/texture-compression-formats/texture-formats-reference));
this is format guidance, not observation of this V Rising build executing on mobile.
The tradeoff is extra decode time and uncompressed GPU memory: BC1 -> RGBA8 is 8x the compressed
storage, BC3/BC7 -> RGBA8 is 4x, BC6H -> RGBA16F is 8x, BC4/BC5 -> R8/RG8 is 2x. These are byte-size
ratios, not measured whole-application RAM or performance improvements.

Verification on Apple M4 / Metal:

- Five asset-independent tests cover BC1 row order, sRGB, cropped blocks and mip tails; BC3 alpha;
  BC4/5 channel storage; half-float alpha; malformed lengths/counts; and unsigned Unity BC6H mapping.
- `cargo test --locked -p rrising-render --test bc_gpu -- --ignored --nocapture` creates a device
  with **no BC feature enabled**, uploads and samples 10 format variants, and compares 640 synthetic
  blocks against a second device's hardware decoder, including all 14 BC6H and 8 BC7 modes. BC6H
  and BC7 agree bit-for-bit on these samples; a partial-row mip chain also uploads uncompressed.
- The initial 1/255 BC1–5 difference bound failed. First BC1 mismatch: block
  `63 a8 3f 5e 85 9b ce de`, green CPU 136/255 vs Metal 138/255. BC3 alpha, BC4 and BC5 also differ
  in palette interpolation. This is not evidence of equivalent numeric rules: different results
  remain. [D3D11.3 section 19.5.2](https://microsoft.github.io/DirectX-Specs/d3d/archive/D3D11_3_FunctionalSpec.htm)
  permits endpoint-dependent approximation for BC1–5, while requiring exact BC6H/BC7.
  The test now explicitly uses a **bounded regression** of 2/255 for these synthetic linear samples
  (4.8/255 after sRGB's slope bound), with this written justification; it is not a universal format
  conformance bound or a widened original-game parity threshold. Measured max: BC1/3 2/255,
  BC4/5 0.007598; sRGB max 0.010989. Full results: [bc-textures.json](verification/bc-textures.json).
- Original Silver_Mountain01: 9,474 mesh instances, 3 markers, 314 groups, 91 pooled textures
  (22 BC1 sRGB and 69 BC3 sRGB). Chrome 157 loaded the same 74.7 MB transfer both normally and
  with BC masked from the adapter before wgpu initialization. The latter requested no optional
  features and the actual browser device did not expose BC. Both rendered with 0 page/console
  errors. Reusable probe: `tools/check_browser_bc.cjs` (existing Playwright and Chrome required).
- Canvas pixels, same 1280x720: hardware vs CPU fallback differ at 12.18% of pixels, max channel
  delta 3/255, mean absolute RGB delta 0.0525/255. Native vs browser hardware differs at 113 pixels,
  max delta 38/255 (edge pixels), mean 0.000592/255. These are comparisons between Rust hosts,
  **not original-game image parity**. Native screenshot is byte-identical to the previously
  committed native scene screenshot. [No-BC browser capture](screenshots/browser-bc-fallback.png).

Release WASM, strict Clippy, formatting, portable tests and existing GPU regressions passed.
Physical Android/iOS GPUs, mobile memory limits, touch input, WebGL and gameplay remain unverified.
The shader, terrain-layer assumptions, transparency exclusions and other viewer limits are unchanged.

### 2026-11-17: movement waypoint selector

Ported `PathfindingUtility.TryGetTargetPositionFromWaypoints` to `rrising-core::pathfinding`.
The original instructions were captured first: 4,096 cases, no helper stubs, all bool results and
output float bits match, including unusual flags, int extremes, signed zero, infinity and NaN.
Native `comiss`/`ja` advances on unordered dots, unlike the decompiler's apparent `<=` branch;
the observed failing case was corrected without dropping inputs or widening comparisons.
[Movement contract and evidence](../evidence/movement_contract.md) records the exact caller and
component layouts. All 32 real scene query candidates are baked Prefabs with no active waypoints
and null targets; this does not exercise live steering. No gameplay completion percentage claimed.

### 2026-11-17: shared steering and original runtime access

The whole shared steering body and float2 normalizer now have 4,096 controlled native cases each.
Rust matches full 96-byte EntityInput writes, also through World/schedule integration. Five original
ECS accessors are replaced by supplied-value callbacks; metadata and sqrt initialization are
isolated. Scene loading resolves special types so Prefabs are excluded from live queries. All 32
shipped candidates remain inactive templates. See [movement_contract.md](../evidence/movement_contract.md).

The original dedicated server also boots through CrossOver in an isolated bottle with a cloned
install and fresh local profile. Six original autosaves and debug dumps were captured during a
90 s run; no game payload is published. Steam GameServer initialization logged an error, while
world startup and persistence completed. Cleanup was forced at the deadline and all owned Wine
services stopped. This is a runtime observation, not a deterministic parity test. Reproduction,
hashes, counts and limitations: [crossover_runtime.md](../evidence/crossover_runtime.md) and
[crossover-server.json](verification/crossover-server.json).

The first persistence boundary is now in Rust: the outer version-2 save container, with ordered
names/ranges and borrowed member slices. The actual app's `--inspect-save` path reads both captured
autosaves; the original-data test matches every member's offset, length and independent payload
fingerprint. Portable tests cover valid format and explicit diagnostic error/limit policy. This
does not restore game state or count as original-vs-Rust gameplay comparison.
Details and stricter malformed-file policy: [save_container.md](../evidence/save_container.md).

The header decoder now reads all 821,700 bytes of each original Header.save: 23,583 prefabs,
19,394 linked handles, 1,542 persistent and 2,240 required component records, 4,933 blob mappings.
Every parsed field re-encodes byte-identically in the original-data test; all persistent indices
join the saved name registry. Runtime records independently confirm six steering component
hashes/sizes. The 9-bit FixedString length and alignment are explicitly tested. No original
saved address is dereferenced. See [save_header.md](../evidence/save_header.md).

Rust reads the four serialization jobs in each of the two retained saves to the end of
their members, including one empty job per save. `AutoSave_4` contains 140 archetypes,
2,673 chunks and 274,238 entity rows; `AutoSave_5` contains 138 archetypes, 2,671 chunks
and 274,236 entity rows. The app lists the counts for each job; values can be read from
the component columns. Actual `EntityInput` and `MapCollision` samples are checked
against source fingerprints. The captures have no `MoveTowardsPositionBuff` column
and do not establish live steering parity. Details and reproduction: [save_jobs.md](../evidence/save_jobs.md).


Scene world construction now carries serialized shared-component indices into the ECS chunk
grouping. `FillSharedComponentArrays` (client RVA `0x8fee810`) and
`FillSharedComponentIndexRemap` (`0x8fee790`) indicate memory-order traversal of shared types;
the adapter checks every nonzero index against its serialized type hash. On the pinned client,
all 1,183 scenes built successfully, with 1,385,671 entities and 31,723 ECS chunks. These are
scene-local grouping identifiers, not restored shared-component values or a runtime parity test.

The scene adapter retains unmanaged shared records and exposes their raw serialized bytes by entity
and type. The pinned client contains 3,366 such records across three types; their recovered layouts
contain no entity or blob references and are marked portable. The other 903 records are managed
and remain opaque. The all-scenes test compares every retained record and entity lookup with its
source scene. Default shared values, managed objects, blob relocation and the original runtime's
type-specific equality/deduplication are not implemented by this inspection interface.

The three unmanaged shared record types found in the pinned client now have typed decoders:
`TileModelLayer`, `SceneSection` and `RenderFilterSettings`. All 3,366 original records pass
size and field checks; this does not establish system behavior or original equality semantics.
The managed mesh-array locator remains heuristic: the original object adapter at client RVA
`0x8fddae0` represents a null Unity object as index -1, which the locator now accepts. Record
boundaries and general managed-object deserialization remain unresolved.

An inventory of `MaterialMeshInfo` in all 1,183 pinned client scenes found 809,820 records: all
use static mesh and material indices, with no runtime IDs or material/mesh index ranges. Runtime
material IDs are now reported unresolved rather than drawn with a guessed neutral material. The
original runtime's `RenderMeshArray` has a separate 12-byte-per-entry `MaterialMeshIndex` array;
`ManagedRenderMeshArray` holds only mesh and material arrays, so runtime range rendering remains
unimplemented. This inventory is scene data, not a claim about dynamically spawned entities.

Scene world building now copies used dynamic-buffer elements into owned ECS storage and remaps
their entity references. The pinned client's 1,183 scenes contain 274,197 buffer rows, including
89,120 external allocations and 305,184,507 used bytes; a separate test compares every restored
row against its source and checks entity remapping. Capacity and inline/external placement may be
normalized. Known blob-reference fields are relocated during world building; unrecognized blob fields and other native pointers remain serialized.

Scene world loading relocates recovered blob-reference fields in scalar components and dynamic
buffers to aligned, world-owned blob payloads; -1 is mapped to null. An all-scenes check verifies
85,403 non-null and 8,444 null scalar fields against the original records. Buffer contents and
reference fields are also checked across all scenes. An independent inventory found 180,056
non-null and 45,067 null buffer fields, with no invalid known offsets. Type layouts remain unknown
for 16,711 component rows, so their possible blob references are not covered. This is a Rust-side
load path; original runtime execution and browser blob behavior have not been verified.

The pinned client probe now traverses generic value fields when determining whether nested blob
references exist. This recovers `ApplyBuffOnGameplayEvent` field offset 56 and the two closed
`NewBlobAssetDatabaseRecord<T>` layouts at offset 16. All 1,772 scene type reports now include
blob-offset results; their identities and shapes are offline-derived and are not a runtime TypeInfo
oracle. All-scene scalar and buffer load tests pass with the resulting report.

After scene blob relocation, the diagnostic world fingerprint normalizes registered blob addresses
to scene-local offsets and includes used dynamic-buffer contents, including externally owned
buffers. The all-scene transform scenario again matches across independent Rust world builds: 372
scenes contain LocalToWorld, while 811 have none. This is a port-side determinism check, not an
original-runtime behavioral oracle; 809 of 112,177 recomputed transforms differ from saved scene
matrices by more than 1e-5 relative error.

Static analysis of client `ProjectM.UpdateLocalTransformSystem.UpdateLocalTransformJob.Execute`
(RVA `0x77651f0`) shows that, when only Translation or Rotation exists, the corresponding
LocalTransform field is updated while the other remains unchanged. The combined port-side transform
adapter now preserves that field, with a synthetic test for both cases. This does not independently
port the ProjectM update job or Unity's LocalToWorldSystem and does not change the 809 scene-data
comparisons above; those remain unresolved.

A separate `LocalTransformSyncSystem` follows the client job's recovered query: LocalTransform
and either Translation or Rotation, excluding Static and the default Disabled/Prefab exclusions.
It updates present fields and copies Scale only when a translation/rotation change triggers the job.
An independent scene-data test checks 102 eligible rows over the pinned client scenes. The job also checks the chunk order version on structural changes. `TransformSystem::local_only` separately reads
LocalTransform for matrix computation; this remains a partial system port, without original runtime
replay, full Unity write-group filtering or automatic replacement of the legacy scene simulation.

Chunk structural order versions now occupy the Entity change-version slot, matching
`ArchetypeChunk.DidOrderChange` at client RVA `0x8ed6ce0`. Creating or removing entities stamps the
slot with the global system version; LocalTransform synchronization checks it as well as the
Translation and Rotation versions. Synthetic tests cover version updates and ordinary component
writes; this is not a replay of the original runtime job.

An experimental scene test schedules the partial ProjectM LocalTransform synchronization
before the local-only matrix pass. On 372 pinned scenes with LocalToWorld, an independent comparison
of the prior Rust path and the new path found 1,327 changed matrices among 919,206; 341 were closer
to baked matrices and 174 farther (the rest tied under summed absolute error). Baked matrices are
not an original runtime oracle. LocalToWorld write-group filtering, complete prefab/disabled handling,
ParentSystem and original game-loop scheduling remain incomplete; these results do not establish
runtime parity. The viewer retains its earlier simulation path until the new path handles prefab
filtering: an independent inspection of one scene found all 52 changed matrices belonged to prefab
entities, including 51 previously identical to baked values.

A host-side input boundary can write the recovered movement and aim fields of `EntityInput` into a
selected ECS entity while preserving the remaining input bytes. It does not yet connect keyboard or
mouse, identify a live player, run locomotion, or animate a character. Its field layout is supported
by the saved shared-steering replays; it is not a replay of the original client input pipeline.

The native viewer has an opt-in `--scene-live` mode which retains a scene World and Schedule and
advances existing steering/transform systems on fixed steps while the window is open. This is an
experimental ECS loop: without a selected entity WASD controls the camera. Input-to-locomotion,
collision, gameplay animation, HUD and original system ordering remain unimplemented. The mode
requires the owner's scene and layout report; interactive behavior has not been verified.

The native experimental scene viewer accepts `--scene-live --control-entity INDEX:VERSION`.
It validates the specified entity is live, not Prefab/Disabled and has a 96-byte EntityInput;
WASD uses the isolated GetMovement calculation with the viewer camera yaw and experimental mode 0, then writes X/Z into that component while preserving aim and action bytes. This is
a diagnostic input path, not a confirmed original keyboard mapping, automatic player identification or movement:
no player locomotion or collision is implemented and a steering system can overwrite input.
Original scene/world and GUI operation have not yet been verified for this mode.

`--list-input-entities <scene> --type-layouts <report>` lists live non-Prefab/non-Disabled
entities carrying EntityInput in a reconstructed scene. The list is diagnostic and does not identify
players. The pinned Silver Mountain scene returned no eligible entities in a local check; finding
or spawning a live controlled player still requires additional systems and content handling.

Client `GameplayInputSystem.GetMovement` at RVA `0x7d907d0` has an initial isolated instruction
replay (`tools/capture_player_get_movement.py`). The 560 recorded cases cover five camera
quaternions, zero and threshold-adjacent inputs, four modes and flag variations. The harness supplies
initialized metadata/zero state and substitutes `sqrtss` for a math wrapper; it is not the live
client input pipeline. The original applies thresholds, camera rotation and mode-dependent scaling,
so the experimental raw WASD input must not be interpreted as a port of this function. The Rust calculation matches all 560 recorded cases bit for bit. Arbitrary rotations, non-finite
values and the full gameplay pipeline still need verification.

On the pinned client's 1,183 baked entity scenes, the player-input inventory found 34 entities
with EntityInput only, 784 with Movement only, 1,036 with both, and two with EntityInput, Movement
and PlayerCharacter. Both player candidates carry Prefab, so neither is a live controllable player.
These counts describe baked scene data, not spawned entities or runtime gameplay.

For a local diagnostic, `--scene-live --spawn-player` now partially clones the client prefab
with GUID 38526109 into the loaded scene world and selects its live PlayerCharacter/EntityInput
entity for the experimental WASD input path. A pinned-data integration test confirms that this
creates three linked entities, including one live player input, in the Silver Mountain scene.
Unresolved external references and managed components are not cloned; an explicitly selected
diagnostic visual and optional looping clip are described below. Locomotion and collision are
not implemented in this path. This is not a playable demo or an original gameplay scenario.

On 2026-11-26 the native window was launched with this option and the pinned client Silver
Mountain scene, then stopped after a bounded 35-second run. It loaded all 4,409 catalog archives,
created a three-member player prefab and rendered more than 1,800 frames without a reported
render error. This verifies startup and continued rendering only; no keyboard interaction,
player visibility, gameplay parity or comparison to the original was checked in that run.
The pinned-data integration test additionally changes the spawned player's Translation and
confirms that a fixed transform step propagates it to LocalToWorld. This checks a rendering
prerequisite, not locomotion or the player's visual binding.

The experimental `--spawn-player` path now attaches the male gameplay-prefab body and cape
in their serialized pose to the cloned player's LocalToWorld, and frames a diagnostic camera
near it. A pinned-data test confirms both parts follow an explicitly changed position. The
model and first face are chosen explicitly rather than by the original runtime selection; it has no
original animation state. Its serialized materials supply base textures, including a cloak
debug-grid material that is not established as the runtime appearance. Other shader maps,
customization and material behavior are not implemented. `--scene ... --spawn-player --offscreen out.png` renders the same
diagnostic camera and player without opening a window. The resulting [scene screenshot](screenshots/experimental-player-in-scene.png)
shows the body partially obscured by world geometry: the prefab's default position is not a
verified spawn location, and the incomplete scene is missing substantial world geometry.
The experimental movement path described below applies isolated ordinary movement arithmetic
to this prefab; collision and the original command scheduler remain unimplemented.

The pinned-data test verifies and decodes both serialized base-map references. The Windows
[textured diagnostic frame](screenshots/windows-player-textured.png) bound 315 draw-group
textures versus 313 in the earlier grey-player frame, with no upload fallback. The renderer's
lighting and shaders are simplified; this is not a comparison to the original.

The build-specific candidate male face collection reader resolves 15/15 original prefab
entries through the player's serialized equipment component. The diagnostic picks entry 0 and
uses its serialized Head-bone mapping and offsets to draw the head and eyebrows with the body.
Walk samples move the head bone as well. Choice and transform composition are assumptions;
neither has been compared with original runtime behavior. Details and evidence limits are in
[player visual links](../evidence/player_visual_link.md).

The optional native `--diagnostic-hud` path reads eight ActionBar frame Images and their shared
`ItemFrame_Normal` sprite from the pinned client, and draws them in an alpha-blended overlay.
The prefab records 48×48 slots, frame size delta 6×6, horizontal spacing 8 and anchored
position (-103, 80); the current center-bottom parent and 1080-height scaling are assumptions.
An original-data test checks the references and dimensions; a synthetic GPU test checks
position and transparency. Windowed and offscreen Windows output were inspected. The local scene
export can now include the same static overlay and a sampled player; Chrome 158 rendered it with
and without BC support. There is no inventory, input handling, health, minimap, dynamic game state
or original Canvas implementation. See [HUD evidence](../evidence/hud_action_bar.md) and
[browser verification](verification/browser-player-hud.json).

For repeatable diagnostics, `--scene ... --spawn-player --offscreen out.png --animation-frame N`
renders one of the same 24 precomputed walk samples used by the live experimental loop (N=0..23).
Frames 0 and 6 were rendered from the pinned client with identical camera and produced different
PNG hashes. The player remains partly occluded at the unverified prefab position, and this does
not establish gameplay animation or parity with the original.

The [animation inventory](../evidence/player_animation_inventory.md) identifies the candidate
model's avatar, controller and walking/running clips and matches tracks to bone paths. Original
Animator evaluation and runtime selection are still unimplemented.

For diagnostics, `--scene-live --spawn-player --animate-player` samples 24 poses of the first
base-controller walk clip at startup and replaces player GPU vertex buffers. With
`--experimental-motion`, horizontal simulated displacement selects walk samples; stationary
updates select the serialized rest pose, not an idle animation. Without that option the walk
clip loops continuously. Neither path evaluates the original Animator, its overrides or transitions,
or establishes Unity parity. A pinned-data test checked both model parts' topology and changed
vertices between sampled poses; a separate test checks the diagnostic rest/walk clock.

`--follow-player` optionally translates the diagnostic camera with the player's displacement,
preserving its offset. This is not the original game's camera. A bounded native run with
`--experimental-motion` was visually inspected in a Windows window; the follow camera remains
an approximation and is not compared to the original.

`--player-position x,y,z` optionally places the partial player at explicit diagnostic coordinates;
these are not an original spawn point and are not checked for collisions. A pinned-data integration
test sends synthetic W press/release events through the same input handlers used by the window and
checks movement and stopping over fixed steps, including a press and release before the next step.
On Windows, a short injected W press produced OS press/release events and moved the player from
Z=22.5 to Z=22.633335. Pending presses are coalesced until a simulation update and can span up to
four catch-up steps, so their original timing and travel distance are not established.

`--scene-live --spawn-player --experimental-motion` opts into an approximate fixed-step
movement prototype. It deliberately executes the recovered ability-setup body, command-field
selection and ordinary movement arithmetic in a host-defined order for the selected player;
WASD feeds its EntityInput using the viewer's camera yaw. A pinned-data integration test checks
three steps of movement, two stopped steps and LocalToWorld propagation. This mode assumes
movement is permitted and reruns ability setup on every step; it does not implement the original
system's other writes, collision, network, ability state, original camera or game scheduler. The
prefab's starting position is unverified and may be obstructed. It is a movement prototype,
not evidence of gameplay parity or a playable game.

The owner's client archive `ade72d3099b8ad7fbfcba0827fe43cd7` also contains male and female
customization-screen character prefabs. The `character_inventory` example resolves their skinned
renderers, joints, blend indices and bind poses: body meshes have 2,640/3,428 vertices and 80/83
bones respectively. These are customization assets; their relationship to a spawned gameplay
character has not been established, and animation is not yet implemented.

An experimental CPU skinning path now draws the male customization prefab's body and cape in
their serialized pose, using bone transforms, bind poses and blend weights. The local diagnostic
`cargo run -p rrising-app --example character_pose -- /tmp/character.png` needs `VRISING_DATA`
and an available GPU. The [diagnostic screenshot](screenshots/experimental-character-pose.png)
shows this result with a neutral material: it has no head, textures, blend shapes or animation,
and is not a gameplay screenshot. Normal transformation currently assumes rigid bones; neither
visual parity nor animation accuracy has been measured.

The same diagnostic can target the candidate gameplay asset using
`character_pose /tmp/player.png ae19a255904fd765632aa97a2d7c670c HYB_VampireMale_Prefab/`.
On the pinned installation this rendered a 2,640-vertex body and 291-vertex cape and produced
the same PNG bytes as the customization pose above. This confirms the diagnostic output for
these two asset paths, not which HybridModel GUID selects them or how the game animates them.

The server `MoveEntitySystem.OnUpdate` calls `CommandUtility.DoMovement` (server GameAssembly
RVA `0x83acd50`). Its ordinary branch has a Rust implementation checked bit-for-bit against
2,240 isolated x86-64 instruction replays under Rosetta. The harness stubs world-type and debug
calls and substitutes `sqrtss`; its cases exclude force-position, move-stop, casting and unusual
input/speed values. This branch is not connected to the scene loop: determining its real ECS
inputs and handling the excluded paths, collision and original scheduling remain necessary.
The server RVA must not be applied to the client binary.

The server's command builder has a separate selection step: for a valid controlled entity,
it takes `EntityInput` when `Movement` is absent, or `Movement.MoveInput` when present unless
spell control suppresses movement. The bounded selection matches 36 isolated instruction-replay
cases, including non-finite bit patterns. Controller validation and actual ECS lookups were
supplied by the harness; this does not establish the full input pipeline or connect it to play.
See [input-to-movement evidence](../evidence/player_command_movement_path.md).

The pinned player prefab GUID `38526109` can be instantiated as a partial server-side ECS
projection: three linked entities and 120 candidate component values, including PlayerCharacter,
EntityInput, Movement, Translation and LocalToWorld on one entity. This projection deliberately
omits nonportable, managed, network and unresolved externally referenced components. It is not a
spawned or rendered player, nor a test of player movement or gameplay parity.

A local integration test now instantiates the partial server player prefab and writes a command to
its EntityInput through the same Rust host boundary. It verifies the movement fields and preservation
of the remaining bytes. No movement system is executed by this test; it is not an original runtime
comparison or an interactive player demo.
