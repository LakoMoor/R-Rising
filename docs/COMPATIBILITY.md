# Compatibility status — 2026-11-16

Target: V Rising v1.1.13.0-r99712-b17 (202605251526), native desktop and browser.

The goal is a compatible runtime that loads the owner's local game content. No
original assets, executables or decompiled sources should be distributed.

| Boundary | Implementation | Runtime connection | Evidence | Remaining acceptance |
| --- | --- | --- | --- | --- |
| Archive / object readers | Present | Native showcase | 4,409 archives, 188,063 indexed objects; real-content tests | Catalog read and wired into native startup; request scheduling remains |
| Entity scene format | Container, archetypes, chunks, masks, buffers, shared components, blobs | `world_adapter` builds one `World` per scene (opaque special components) | 9 scene tests + `world_adapter` test: all 1,183 scenes, 1,385,671 entities with preserved ids | Shared values, chunk components, managed objects, buffer heap payloads, entity-reference remap |
| Math | Broad API coverage | Showcase transforms | 58 regression/unit tests passed | Replay oracle for 23 scalar int/float leaves (`oracle_math.rs`, 2,048 cases each); transcendental rounding deviations in PORT.md are not covered |
| ECS | Primitive types and layout; EntityComponentStore entity table (alloc/free, versions, free list), create/destroy in chunks, minimal include/exclude query (default Disabled/Prefab exclusion matches `IsMatchingArchetype` @ 0x90f46c0 by decompiled-body reading; other option gates not ported), `World::build` from plain chunk/archetype input with preserved entity ids/versions (`SpecialTypes::new`; the old all-zero Default aliasing is gone) | Unit tests only; not wired into a running game world | Port-side unit tests derived from decompiled bodies (`store.rs`); native-replay oracles only for the pure archetype getters, Hash128 compare/hash and Entity/ChunkIndex compares (`oracle_entities.rs`); store/creation functions are not pure leaves and have none | Shared/managed/chunk/cleanup archetypes, meta chunks, buffer heap, EntityQueryData parity, system scheduling |
| Rendering | Basic lit meshes | Native showcase | GPU triangle test; screenshots/native-content.png | Original scenes, animation, HDRP and visual comparisons |
| Gameplay / HUD / physics / sound / networking / saves | Not implemented | None | No end-to-end parity cases | First original-scene playable slice |
| Browser | Shared renderer + async WASM host | Local exported scene import | Chrome / Apple Metal-3: 48 objects, zero JS/WebGPU errors | Direct archive import, mobile texture fallback, game world |
| Performance | Rust-only absolute timings (see Performance measurement) | None | No comparable original-runtime benchmark | Same-scene release measurements on both runtimes |

## Metrics

`progress.json` is reproducible with `python3 tools/port_coverage.py --json`.
It measures heuristic **member name matches** only. It can credit unrelated
identifiers and does not prove overload coverage, correctness, or runtime wiring.
Behavioral parity and overall game completion are unknown, not 2.7%.
There are no full-game original-runtime golden scenarios. Original-function replay oracles exist only for pure x86-64 leaf functions executed under Rosetta, not a running game: 7 earlier functions with 1,024 cases each (type flags, MaterialMeshInfo) and 75 further Unity.Mathematics/Unity.Entities leaves with 2,048 cases each (`tools/capture_math_int.py`, `capture_math_float.py`, `capture_entities.py`; goldens in `evidence/oracles/`; see evidence/original_leaf_oracles.md). The replay found and fixed three Rust divergences: integer `clamp` with lo > hi, `abs(i32::MIN)`, and `Archetype.HasChunkComponents` (`!=`, not `> 0`). Reader tests and GPU
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
- Windows, Linux, Android and iOS execution have not been verified. Browser execution is covered by the subsequent browser milestone.

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
27,497 chunks versus the scene's 31,723. Entity references are not remapped.

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
