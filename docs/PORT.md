# Port evidence ledger

Every claim in [README.md](../README.md) and every deviation the port ships with is recorded here
with the command that produced it. Nothing in this file is a plan; it is what was measured on this
machine against this install.

This ledger contains historical investigation notes. Current acceptance results
and corrections are in [COMPATIBILITY.md](COMPATIBILITY.md).

## The original

| Fact | Value | How it was measured |
| --- | --- | --- |
| Engine | Unity **2022.3.58f1** | `VRising_Data/globalgamemanagers` + the TypeTree version the bundles ask for |
| Backend | IL2CPP (mono-less), x64 Windows | `GameAssembly.dll` + `global-metadata.dat` |
| Managed layer | `global-metadata.dat` **version 31**, 185 assemblies | Il2CppDumper 6.7.46 header output |
| `GameAssembly.dll` | 247,473,480 bytes | `ls -la` |
| Content | 4,411 files in `ContentArchives` (4,409 UnityFS bundles, `archive_dependencies.bin`, 1 other) | directory scan + `unity_formats::listing::list` |
| Entity scenes | 1,183 `.entities` + 218 `.entityheader`, magic `DOTSBIN!` ("EntityBinaryFile") | directory scan + magic check |
| Pipeline / audio / middleware | HDRP (`Unity.RenderPipelines.HighDefinition.Runtime`), FMOD (`fmod.dll`, `ProjectM.Fmod`), Epic Online Services (`Stunlock.Network.EosSdk`) | assembly list in the dump |

```sh
# The dump everything below is derived from.
DOTNET_ROLL_FORWARD=LatestMajor dotnet Il2CppDumper.dll \
    ../GameAssembly.dll ../VRising_Data/global-metadata.dat out/
# → dump.cs 2,731,403 lines · il2cpp.h · script.json (1,203,037 ScriptMethod entries)
#   DummyDll/ 142 assemblies · stringliteral.json
```

From `dump.cs` a structured inventory was built (`inventory.jsonl`): **43,440 types, 223,957
methods** with their RVAs. Game-authored code (`ProjectM*`, `Stunlock*`) is **26,428 types /
97,288 methods**, largest first: `ProjectM.dll` 23,043, `ProjectM.Gameplay.Systems.dll` 16,900,
`Stunlock.Network.EosSdk.dll` 13,305, `ProjectM.HUD.dll` 9,627, `ProjectM.Shared.dll` 9,468.
The address→name map (`funcs.tsv`) holds 159,654 addresses, of which **69,851 are game methods**.

## Type trees

The bundles store type trees but reference Unity's *common string* table, which ships in the
player's TypeTreeDB, not in the bundles. `assets/typetrees.tpk` (1,010,449 bytes, format 2, stored)
supplies it for 2022.3.58f1: **321 classes, 48,100 nodes, 5,162 strings, 109 common strings**,
generated from AssetRipper's `InfoJson/2022.3.58f1.json` by `work/make_tpk.py`.

## Content readers (`rrising-content`)

```sh
rustup run 1.99.0 cargo test -p rrising-content
```

```
test localization_and_settings_load ... ok
test opens_a_slice_of_the_content_archives ... ok
test reads_a_real_mesh_and_its_textures ... ok
test opens_the_whole_content_archive_folder ... ok
test content_index_fingerprint_is_stable ... ok
test base_colour_maps_decode_to_colour ... ok
```

`cargo test -p rrising-content` also runs `tests/entity_scenes.rs`, whose five tests are listed in
the ECS section below.

What those tests assert against the install:

- **Whole install indexed**: 4,409 archives → 4,418 files, **188,063 objects** in 2.5 s. Class
  counts include `GameObject` 126,182, `Mesh` 13,142, `Material` 7,619, `AnimationClip` 7,410,
  `Avatar` 159, `Animator` 521, `MeshFilter` 909.
- **A real mesh**: `Gloomrot_Sludgepool_Scenery02` — 140 vertices, 246 indices, position/normal/uv
  channels read from the file's own channel layout.
- **A real texture**: `TornCloak01_bc` — 512×512, Unity format id 12 (`Dxt5`), 10 mip levels,
  blocks taken as stored.
- **A real material**: `VampireMale_Cloak28` — shader reference plus 8 texture slots resolved.
- **Localization**: 16,842 English strings; **31 setting presets** (UTF-8 BOM stripped).
- **Texture pixels decoded from the file's own blocks** (`tests/texture_decode.rs`):
  `OnionPlant01_bc` 128×128 BC1 → mean rgb (0.441, 0.281, 0.235); `ResearchStationCloth01_bc`
  256×256 crunch→BC1 → (0.290, 0.288, 0.290), i.e. the art really is grey; `TornCloak01_bc`
  512×512 BC3 → (0.077, 0.003, 0.018). That is what makes "the frame has grey props" a statement
  about the content rather than about the sampler.

## Renderer (`rrising-render`)

```sh
rustup run 1.99.0 cargo test -p rrising-render          # draws_a_hand_built_triangle
rustup run 1.99.0 cargo run -p rrising-app -- --offscreen frame_full.png \
    --archives 5000 --items 48 --width 1600 --height 900
```

```
indexed 188063 objects from 4418 files in 2.5s
scene: 48 items
wrote frame_full.png (1600x900, 48 items, 26 textures bound, 610085 pixels covered by geometry, clear (39, 39, 48))
```

`draws_a_hand_built_triangle` is the guard that keeps the GPU path honest: it draws a triangle that
comes from no content file at all, with an identity `view_proj`, and fails if the vertex layout, the
instance buffer, the uniform block, the depth test or the readback stops working. (It caught a real
bug: the layout declared `Float32` where the shader reads `vec3<f32>`, so only the x of every
position arrived and every triangle collapsed to a line.)

Texture upload is verified per format: BC1/BC3/BC4/BC5/BC6h/BC7 are held by the device with the
file's own blocks (no transcoding), the mip chain is uploaded whole, and Unity's bottom-first rows
are flipped at upload only. Crunch (`DXT1Crunched` 28, `DXT5Crunched` 29) is unpacked by
`unity-formats`' crunch decoder into the DXT blocks it decodes to — measured on
`ResearchStationCloth01_bc` (256×256, format 28), which now binds as `_BaseColorMapTrim1`.

## ECS runtime (`rrising-ecs`) and entity scenes (`rrising-content::entity_scenes`)

```sh
rustup run 1.99.0 cargo test -p rrising-ecs                              # 9 tests
rustup run 1.99.0 cargo test -p rrising-content --test entity_scenes     # 9 tests
```

What those tests assert against the install, and what the shipped files say back:

- **Container** (`evidence/entity_scenes_fingerprint.txt`): all **1,183**
  `StreamingAssets/EntityScenes/*.entities` files parse as `DotsSerialization` v77 — the 152-byte
  `FileHeader` (`DOTSBIN!`, version 77, `Hash128 FileId`, file type `EntityBinaryFile`) plus data,
  metadata and nodes sections tile every file exactly. 2,303,710,267 bytes of node blobs,
  fingerprint `e2b9d375b335b807`.
- **Node tree**: the root is a 72-byte `FolderNode` tagged `SerializeUtility.WorldNodeType`; its six
  children are 80-byte `RevisionedRawDataNode`s — 72 + 6×80 = 552 = `NodesSectionSize` — and
  `NextSiblingOffset` counts from the root, `-1` ending the chain. A node is named by its 16-byte
  `Id`, not by `NodeTypeHash`: every data node shares one hash (`0x7ff5f067a875f185`) because they
  are all the same struct, and what differs is which of `SerializeUtility`'s twelve `Hash128`
  statics it carries. Those are GUID literals `SerializeUtility..cctor` @ 0x8fef290 feeds to
  `Hash128.StringToHash` @ 0x8fcb550, which reads each group of eight hex characters back to front —
  so on disk every byte of an `Id` is the literal's byte with its nibbles swapped. All twelve tags
  are re-derived from their literals with the ported `Hash128` and matched against the files.
- **Archetypes stream** (`evidence/entity_archetypes.txt`): `WriteArchetypesNode` @ 0x8fe9ba0 writes
  `int UniqueTypesCount`, that many `ulong` stable type hashes (`TypeInfo.Hash` @ 0x18),
  `int ArchetypeCount`, then per archetype `int Archetype.EntityCount` @ 0xbc,
  `int Archetype.TypesCount - 1` @ 0xc4 and that many `int uniqueIndex | 0x20000000`;
  `ReadArchetypes` @ 0x8fe0fa0 reads it back and rebuilds each archetype with
  `CreateArchetypeWithoutSimulateComponent`. Measured: 1,183 files, **17,579 archetypes**,
  **32,974 unique type entries**, **1,772 distinct stable type hashes**, **7,946 enableable
  slots**. In every one of the 1,183 files the stream's entity total equals the
  `CountForSerialization` total of that file's own chunks — **1,385,671** entities — which is what
  pins the first per-archetype int to `EntityCount` rather than to an archetype index.
- **Chunks stream** (`evidence/scene_chunks.txt`): 31,723 blocks of `Chunk.kChunkSize` = 32,768 in
  all 1,183 files, every `Flags` one of the four `ChunkFlags` values, every
  `CountForSerialization` in 0..=4,088, 2,607 distinct archetype indices.
- **EnabledBits stream** (`evidence/entity_enabled_bits.txt`):
  `WriteEnabledBits` @ 0x8fe7230 takes the archetype array `WriteArchetypesNode` returned and sizes
  two regions from it — `Archetype.Chunks.ComponentCount * Archetype.Chunks.Count * 16` and the same
  product times `4`, where `ArchetypeChunkData` keeps `Count` at `+0xc` and `ComponentCount` at
  `+0x14` — then writes `int totalEnabledBitsSize`, that many bytes archetype by archetype,
  `int totalHierarchicalDataSize`, and those bytes. A 16-byte unit is
  `ArchetypeChunkData.ComponentEnabledBits` for one (chunk, component) pair —
  `GetComponentEnabledMaskArrayForChunk` @ 0x911d5e0 indexes it as
  `chunkIndex * ComponentEnabledBitsSizeTotalPerChunk` and `GetEnabledArrayForTypeInChunk`
  @ 0x911d6a0 wraps it in an `UnsafeBitArray` of the chunk's entity count — and the `int` beside it
  is the matching `ComponentEnabledBitsHierarchicalData` entry. `ComponentCount` is the archetype's
  whole `TypesCount`, so the walk has to add back the `SharedComponentData` entry at `Types[0]` the
  `Archetypes` stream leaves out. Measured: 1,183 files, **648,652** (chunk, component) units,
  **10,378,432** bytes of masks and **2,594,608** bytes of hierarchical data, every stream exactly
  `8 + 20 × units` bytes, **every** mask inside its chunk's entity count, 45,360 units with every
  entity enabled, and the fullest chunk in the install holding 128 entities — exactly the width of
  one mask.
- **BufferData stream** (`evidence/entity_buffer_data.txt`): a `DynamicBuffer`'s slot in a chunk is
  a pointer, and `FillBufferPatchRecordsAndClearBufferPointer` @ 0x8fee550 records its offset and
  zeroes it before `WriteChunks` writes the block, so the contents travel separately.
  `WriteBufferDataElements` @ 0x8fe6080 — signature `(DotsSerializationWriter, int totalChunkCount,
  UnsafePtrList<Archetype>, NativeList<int> bufferPatchesCountPerChunk,
  NativeList<BufferPatchRecord>, NativeList<IntPtr> bufferDataList)` — writes `int totalChunkCount`,
  then one `int` patch count per chunk in archetype order, and per patch the 8-byte
  `BufferPatchRecord` (`ChunkOffset` @ 0x0, `AllocSizeBytes` @ 0x4) followed straight away by
  `AllocSizeBytes` bytes. Measured: 1,183 files carry the node, 384 of them carry patches —
  **89,120** patches, **339,707,598** bytes of buffer contents, every stream consumed exactly to its
  `DataSize`, every `totalChunkCount` equal to its file's chunk blocks, every offset inside the
  32,704-byte chunk buffer (max 32,512), offsets strictly ascending per chunk, and all 89,120
  landing on eight zero bytes at `block + Chunk.kBufferOffset + ChunkOffset` — the cleared pointer
  itself.
- **SharedAndManagedComponents stream** (`evidence/entity_shared_and_managed_components.txt`):
  `WriteSharedAndManagedComponents` @ 0x8fe6d80 writes `int sharedComponentArray.Length` and the
  array — the remap from a chunk's shared component slot to the serialized shared component — then
  the unmanaged half @ 0x8feb380, which emits one 16-byte `SharedComponentRecord`
  (`StableTypeHash` @ 0x0, `ComponentSize` @ 0x8) per shared component and appends the value bytes
  to an `UnsafeAppendBuffer` it writes as `int length` plus those bytes, then the managed half
  @ 0x8fea7c0, which records `ComponentSize = 0xffffffff` for every managed object and writes
  `int count`, the records, `int blob.Length`, `int managedComponentCount` and the blob. Measured:
  1,183 files, **9,414,749** bytes, **82,485** remap entries (all **72,547** non-zero ones landing
  on `1..=serialized`, so the table is surjective), **3,366** unmanaged shared components over
  **67,054** value bytes, **903** managed shared components, and **8,921,059** bytes of managed blob
  holding **57,578** per-entity managed components. Every record hash appears in that same file's
  `Archetypes` unique type list.
- **BlobAssets stream** (`evidence/entity_blob_assets.txt`): `WriteBlobAssetNode` @ 0x8fe7ad0 opens
  the node, `GatherAllUsedBlobAssets` @ 0x8feceb0 collects every blob the world reaches,
  `CalculateBlobAssetBatchTotalSize` @ 0x8fe81f0 sizes the batch as `16 + sum(32 + align16(Length))`
  and records each blob's payload at `48 + sum(32 + align16(Length))` over the earlier blobs, and
  `WriteBlobAssetBatch` @ 0x8fe8380 writes `BlobAssetBatch.CreateForSerialize` @ 0x9070490's 16
  bytes (`TotalDataSize`, `BlobAssetHeaderCount`, `RefCount = 1`, `Padding = 0`) followed per blob by
  `BlobAssetHeader.CreateForSerialize` @ 0x90709c0's 32 bytes (`ValidationPtr = 0`, `Length`,
  `Allocator = 1`, `Hash`, `Padding = 0`) and the blob. `EndDeserializeWorld` @ 0x8fe34f0 hands the
  node's whole blob to `BlobAssetBatch.CreateFromMemory` @ 0x90704b0, which throws unless
  `TotalDataSize` equals the buffer size and `RefCount` is 1 and then walks it by `32 + Length`,
  patching each `ValidationPtr` to the bytes after its header and rejecting any `Allocator` that is
  not 1 — the batch is used in place, so the file layout *is* the runtime layout. Those recorded
  payload offsets are what a serialized `BlobAssetReferenceData` holds:
  `ReadUnmanagedSharedComponents` @ 0x8febba0 keeps `WorldDeserializationStatus.BlobAssetBuffer`
  @ 0x28 and writes back `base + value` into each of a type's
  `TypeInfo.BlobAssetRefCount` @ 0x3c reference slots, or null for `-1`. Measured: 1,183 files carry
  the node, **762** of them an empty 16-byte batch; the other **421** carry **85,442** blobs over
  **899,199,248** bytes, every `Length` a multiple of 16, every `ValidationPtr` and `Padding` zero,
  every `Allocator` 1, every batch tiled exactly to its `DataSize`, and **50,768** distinct `Hash`
  values of which **21,227** recur — the same blob is written into several scenes.

`rrising-ecs` carries the parts of `Unity.Entities` the scene reader needs, translated from the
decompiled bodies rather than paraphrased: `TypeIndex` (a 14-bit index plus 18 flag bits),
`TypeInfo`, `TypeManager`, `ComponentType`, `ComponentTypeInArchetype`, `Archetype` (the
`0x160`+128-byte layout, `CreateArchetype`'s sort by `(typeFlags, memoryOrderings)`, its backwards
category walk and its FNV-1a type hash), `Chunk` (32,768-byte allocation, 64-byte buffer offset,
4,088-entity cap, the 40-byte serialized header) and `NativeList`/`BufferHeader` growth.

Not ported yet, and the reason the scene reader stops short of a loaded world: the
`TypeNames` / `TypeNamesStringTable`, `Prefab` and `DebugSection` streams are identified and their
bytes are in hand, but their payloads are still unread; `EntityComponentStore` itself (allocation,
`CreateArchetype` registration, chunk import, shared component insertion) is not implemented; the
managed object blob is carved out but not decoded, which needs `ManagedObjectBinaryReader`; the
`.entityheader` file that carries the shared type table for a scene group is not parsed; and a
shared component's `BlobAssetReferenceData` slots are still read as plain ints rather than resolved
against the `BlobAssets` batch the way `ReadUnmanagedSharedComponents` @ 0x8febba0 does.

## Ghidra

- **Run 1** (full auto-analysis, `ApplyNames.java` as post script): 13 h 02 m elapsed, and after
  9 h it was still inside `AddressTableAnalyzer.processAddressTable` (confirmed with `jstack`), with
  nothing written to the project. Killed; no output was lost because none had been produced.
- **Run 2** (`-noanalysis` plus `FastDecompile.java`, which does only what the port needs — name each
  function from the dump, disassemble and flow its own body, then decompile in 8 threads into
  `evidence/decompiled/<assembly>.c`). First pass over `game_funcs.tsv` (69,851 game methods), log
  `work/ghidra_fast.log`:

  ```
  FAST flow DONE total=69851 created=6042 existed=63809 flowed=69851 bad=0 ready=69851
  FAST DONE decompiled=69851 failed=27 of 69851
  ```

- **Run 3** (naming pass over all 159,654 mapped addresses, so callees appear by name instead of
  `FUN_…`), log `work/ghidra_name.log`:

  ```
  FAST flow DONE total=159654 created=12134 existed=147520 flowed=159654 bad=0 ready=159654
  ```

- **Run 4** (decompile of the focus set — game assemblies plus `Unity.Mathematics` and
  `Unity.Entities`, `work/decompile_focus.tsv`), log `work/ghidra_decompile.log`:

  ```
  FAST DONE decompiled=81812 failed=21 of 81812
  ```

  Result: 50 files, 315 MB, 10,098,456 lines of C, 81,812 function bodies. Phase 1 of run 2 finished
  in about 4 minutes; the 13-hour auto-analysed run had produced nothing.

```sh
# once, without analysis: the import takes a couple of minutes and is saved into the project
GHIDRA_HEADLESS_MAXMEM=12G analyzeHeadless /tmp/rrising_proj2 rrising \
  -import ../GameAssembly.dll -noanalysis

# then: name + disassemble + decompile the dump's game methods
GHIDRA_HEADLESS_MAXMEM=12G analyzeHeadless /tmp/rrising_proj2 rrising \
  -process GameAssembly.dll -noanalysis \
  -scriptPath work/ghidra -postScript FastDecompile.java work/game_funcs.tsv \
      ../R-Rising/evidence/decompiled 8
```


## Generated surface (`rrising-math`)

`Unity.Mathematics` is source-generated in the original — `float4` alone declares 419 members — so
the port generates the same surface from the dump's own member list instead of hand-writing a subset.

```sh
python3 tools/port_coverage.py --emit-members work/inventory.jsonl   # → evidence/members.tsv
python3 tools/emit_type_facts.py <dump.cs> evidence/math_conversions.tsv evidence/math_fields.tsv \
    <stringliteral.json> evidence/math_tostring.tsv
rustup run 1.99.0 cargo build -p rrising-math                        # → crates/rrising-math/generated/vectors.rs
python3 tools/port_coverage.py                                       # measure the crates against the ledger
```

Measured:

```
distinct (assembly, type, member) rows: 187524
excluded, no Rust counterpart: 147

group                                                 ported   total   share
Unity.Mathematics* (ported by rrising-math)             4850    4961   97.8 %
Unity.Entities* (ported by rrising-ecs)                  184    8486    2.2 %
game-authored (ProjectM*, Stunlock*)                       1   79642    0.0 %
BCL (no crate assigned yet)                                0   29220    0.0 %
engine assemblies (no crate assigned yet)                  0   40839    0.0 %
middleware / other (no crate assigned yet)                 0   24229    0.0 %
ALL                                                     5035  187377    2.7 %
```

`crates/rrising-math/build.rs` reads the ledgers, emits the 63 vector and matrix types the dump
lists (40,590 lines: 2,886 swizzle getters, 462 swizzle setters, 472 `From` conversions, 1,239
operator impls, 63 `Display` impls) and then **fails the build three times over**: the emitted
`xxx`/`set_xy` set must equal the dump's, the emitted `impl From<S> for D` set must equal the dump's
conversion operators, and every generated type must find its `ToString` literal in the ledger with
an argument count that matches its own. A dropped `zyx`, an invented `set_xx`, a `From<int3> for
half3` the original never declares, or a format string that belongs to another shape is a compile
error, not a review finding. Every operator is gated on the ledger as well, so a type the dump gives
no `op_LeftShift` gets no `Shl`, and the generated file is committed under `generated/` so the
surface can be read and diffed.

`members.tsv` collapses overloads to distinct names, which is right for counting and too coarse for
conversions: `half3` declares `op_Explicit(float3)` and `op_Explicit(double3)` and nothing else,
while `float3` declares `op_Implicit(half3)`. So `evidence/math_conversions.tsv` keeps the
signatures and gates them, `evidence/math_fields.tsv` keeps the static fields, and
`evidence/math_tostring.tsv` keeps the format literals — `float3({0}f, {1}f, {2}f)`, the matrices
printed row by row with two spaces between rows, and the `f` only on the `float` families.

**What the denominator is.** IL2CPP's linker trims the metadata it ships. `quaternion` declares no
`op_Multiply`, `op_Addition` or `normalize` in `dump.cs`, although the game multiplies quaternions
constantly and the corpus shows no call to such a function: the bodies are inlined into their
callers and the metadata entries are gone. The same trimming shows in the fields — every vector type
keeps exactly one static field (`zero`, which is all-bits-zero and so needs no `.cctor`), while
`quaternion` and the square matrices keep a `.cctor` to build their non-zero `identity`.
`System.DateTime` in the same dump lists all three of its static fields, so this is trimming, not a
dumper bug. The percentage measures the shipped build's member list — what the port has to reproduce
to run the game — not the whole API of the libraries involved.

What is left in the assembly is the types the crate does not declare yet — `random` (36 members),
`min_max_aabb` (19), `noise` (17), `plane` (13), `aabb` (8), the mono-script-types holder (6),
`generate_points` (3) and a dozen attributes and job-registration helpers — 111 rows in total. Every
member of every type the crate *does* declare is accounted for: `--top Unity.Mathematics` reports no
gaps there.

## Deviations

Things the port does differently from the shipped game, each with what would close it. None of them
is a silent approximation: they are the price of running on a different GPU API and a different
math library, and each is a place where a golden test will have to allow a tolerance — or be fixed.

1. **Transcendentals.** `sin`/`cos`/`tan`/`atan` in `rrising-math` call Rust's libm. Unity's
   `mathematics` uses its own SIMD polynomial approximations, so results differ in the last bits.
   *Closes when:* the original's approximations are recovered from the decompiled bodies and the
   parity tests are extended to a bit-exact comparison.
2. **Determinant and inverse.** The original computes both through a SIMD shuffle network; the port
   expands cofactors. Same mathematical result, different float rounding order. The port also
   returns a zero matrix for a singular input where the original returns infinities.
   *Closes when:* the shuffle network is ported and the guard is matched to the original's behaviour.
3. **Triangle culling is off** (`cull_mode: None`). The original's winding convention has not been
   recovered, and guessing it would cull half of every model.
   *Closes when:* the winding is read out of the decompiled HDRP/culling code and a golden frame
   confirms it.
4. **Depth range.** The port builds matrices with the ported `Unity.Mathematics`, which produce
   Unity's clip space (z ∈ [-1, 1]); wgpu's is [0, 1]. One fixed matrix (`clip_fix()`, applied
   before the perspective divide) converts, and it lives in the renderer so the ported matrices stay
   the original's. The original on Metal runs reversed-Z from engine code the port has not recovered.
   *Closes when:* the engine-side projection/depth conventions are ported and `clip_fix` is replaced
   by them.
5. **The showcase camera's view matrix** is `z-flip · inverse(LookAt(eye, target, up))`, because
   `float4x4.LookAt` is the camera's local-to-world matrix (the original keeps the eye unnegated in
   the fourth column) while `PerspectiveFov` expects a point in front at negative view-space z.
   Unity's own `Camera.worldToCameraMatrix` is engine-native (not in `GameAssembly.dll`) and has not
   been recovered. *Closes when:* the port has its own camera system built from the decompiled
   `ProjectM` camera code.
6. **The lighting pass is not HDRP.** One directional light, a hemisphere ambient, half-Lambert, no
   shadows, no clustered lights, no volumetrics, no deferred G-buffer, no post stack. It reproduces
   the *look* of a plain lit prop, not the game's frame.
   *Closes when:* HDRP's passes are ported from `Unity.RenderPipelines.HighDefinition.Runtime`
   (4,411 methods in the dump).
7. **Crunch coverage is DXT-only.** `Dxt1Crunched`/`Dxt5Crunched` unpack; `EtcRgb4Crunched`/
   `Etc2Rgba8Crunched` and pre-2017.3 crunch are refused with an error naming the texture. The PC
   build uses the DXT family, so nothing in this install is lost — but the refusal is deliberate.
8. **Scene placement is a showcase grid.** The original places art through `EntityScenes/*.entities`
   (`DOTSBIN!`), which the port cannot parse yet, so `rrising-core::showcase` picks real meshes and
   their real materials out of the archive index and lays them out with the original's own `TRS`
   composition. *Closes when:* the entity binary format is parsed.
9. **`archive_dependencies.bin` is unparsed** (486,664 bytes): it is the serialized
   `RuntimeContentCatalog` — NativeHashMaps of scene/object/file/archive locations, file
   dependencies and a managed string table. Until it is read, the port indexes archives by scanning
   them rather than by following the game's own load order.
10. **Type trees come from AssetRipper's table**, converted to a `.tpk`, rather than from the
    bundles' own common strings. Verified by reading every archive in the install; a mismatch would
    show up as a failed read, not a silent wrong value.
11. **Texture orientation is flipped at upload** (Unity stores mips bottom-first, wgpu wants
    top-first). This is the only place orientation is touched; uv values stay as the file stores
    them.
12. **No live comparison.** The original ships Windows (D3D11/D3D12) and this host is Apple silicon,
    so frames are compared by offscreen PNG readback against screenshots, not against a running
    original.
13. **Untouched subsystems:** FMOD audio, animation/ECS blending, physics, Steam + Epic Online
    Services, saves, and the whole HUD (`ProjectM.HUD.dll`, 9,627 methods).
14. **`half` is not ported.** The dump gives `half` a `ushort` field and four conversions
    (`op_Explicit(float/double)`, `op_Implicit(→float/double)`), and `half2/3/4` the full swizzle
    set but **no arithmetic operators at all**. The decompiled `math.f16tof32` (RVA 0x612f520) and
    `math.f32tof16` (RVA 0x92b9520) bodies are bit-tricks on two constants whose values are in
    `.rdata` (read from the shipped file: `2^-112` = 0x07800000 and `2^-14` = 0x38800000), but
    Ghidra's overlapping-globals warnings mean the second one is not yet transcribed with
    confidence. ~600 members are left uncounted rather than guessed at.
    *Closes when:* the `f32tof16` body is disassembled instruction by instruction and the port's
    conversions are tested against round-trip and subnormal cases.
15. **Constructor overloads become named constructors.** C# `float3(float v)`, `float3(float, float,
    float)` and `float3(double3)` cannot coexist in Rust: `new` takes the components, `splat` the
    scalar, `copy` the vector and `from_int3` &c. the cross-family forms. The dump groups them under
    one `.ctor` member, so the measurement is unaffected.
16. **`op_Implicit` and `op_Explicit` both become `From`.** Rust has one infallible conversion trait
    where C# has two spellings of it; each generated impl's doc comment names which one the dump
    declares, so the distinction is recorded rather than lost.
17. **`==` is the original's `Equals`, not its `op_Equality`.** `float3 == float3` in C# returns a
    component-wise `bool3`, which Rust's `PartialEq` cannot express; the port derives `PartialEq` for
    `Equals` and spells `op_Equality` as `is_equal`, with `less_than`/`greater_than` &c. likewise
    returning `boolN` instead of implementing `PartialOrd`. Ported game code that reads
    `math.all(a == b)` must become `a.is_equal(b).all()` once `math.all` exists.
18. **`.cctor` becomes `const`.** The original's static constructors exist to initialise
    `static readonly` fields (`float3.zero`, `quaternion.identity`); in Rust those are compile-time
    items, so there is no code to run and no order to get wrong.
19. **Matrix `op_Multiply` is component-wise.** Transcribed from the decompiled
    `float3x2::op_Multiply(float3x2, float3x2)` at RVA 0x9333a30, which multiplies the six components
    pairwise and never sums a row against a column. The real product is `math.mul`, still unported.
20. **Two smaller behavioural substitutions.** `op_UnaryNegation` on the unsigned families compiles
    in C# to a wrapping negate, so the port uses `wrapping_neg` (Rust has no `-u32`); and
    `get_Item`/`set_Item` out of range panics where the original throws
    `IndexOutOfRangeException`.
21. **`half` keeps the library's 16-bit truncation.** `half(float)` is `f32tof16(v)` truncated to 16
    bits, so out-of-range inputs wrap instead of saturating: `70000.0f → 0x7c46` (a finite −2.125) and
    `1e30f → 0xca50` (−12.625). `to_f32` is `f16tof32`, and `op_Equality`/`GetHashCode` compare the raw
    short, so `+0.0 != -0.0` as halves although their floats are equal, and a NaN half equals itself.
    Bodies: `half.op_Explicit(float)` @ 0x929f960, `op_Implicit→float` @ 0x612ac20,
    `op_Equality` @ 0x17d88d0, `GetHashCode` @ 0x17d6a80.
22. **`RotationOrder` is the shipped enum, not the documented one.** This build has no `None` member:
    `XYZ = 0 … ZYX = 5` with `Default = 4`, i.e. the default Euler order is **ZXY**, which is also why
    `Euler(float3, math.RotationOrder order = 4)` records 4 as its default argument.
23. **`quaternion.EulerZYX` differs from the textbook composition.** The shipped body
    (@ 0x5b91150) negates the second product term in all four components relative to
    `mul(mul(Z, Y), X)`; an earlier hand-written `euler_zyx` here had that sign wrong and was removed.
    All six orders are now the body's half-angle product with its own sign vector, and each is pinned
    against the axis-rotation product it is supposed to be.
24. **`math.unitlog(quaternion)` can return NaN.** The body clamps `w` into `[-1, 1]` and then divides
    by `sqrt(1 − w·w)` with no guard, so any quaternion with `|w| ≥ 1` (including `identity`) yields
    `NaN` in xyz and `0` in w. Pinned by test rather than fixed.
25. **`math.orthonormalize(float3x3)` is not Gram-Schmidt.** It projects `c1` against the
    *unnormalised* `c0` and ignores `c2` entirely, so the result is orthonormal only when `c0` is a
    unit vector; the degenerate branch falls back to the canonical basis using the `1e-30f` at
    0x18cf28590. Transcribed verbatim (@ 0x92bae80) and pinned by test.
26. **Vector overloads are per-shape.** `refract_1284` @ 0x92b3960 is the **float2** overload (its
    `CONCAT44` returns two floats); the float3 body is a sibling. Its zero fallback is `k < 0`, not
    `k == 0`, so grazing incidence takes it.
27. **Conversions are gated by the dump.** `evidence/math_conversions.tsv` lists the 489
    `op_Implicit`/`op_Explicit` signatures the shipped metadata keeps, and the generator emits exactly
    those — so `half3(float3)` exists but `half3(int3)` does not, and `float3x3(float4x4)` exists but
    `float4x4(float3x3)` does not. A build-time cross-check fails if the emitted `From` set drifts.
28. **Generated statics exceed the trimmed metadata.** The library declares `one`, `eps`, `min_value`,
    `max_value`, `default_value` and the bool `truevalue`/`falsevalue`; the shipped metadata keeps only
    `zero` (and `identity` for the square types), so those extra items are real API but earn no
    coverage credit. `evidence/math_fields.tsv` records what the dump keeps.
29. **Coverage credits an identifier once per crate.** The tool matches member names against the set of
    identifiers the crate declares, so declaring `euler_zyx` once credits `EulerZYX` for `quaternion`,
    `float3x3` and `float4x4`. That biases the percentage upward for shared names, and it is recorded
    rather than hidden.
30. **`quaternion.normalize` has no zero guard.** The bodies that inline it (inside
    `quaternion(float3x3)` @ 0x93757a0 and `quaternion(float4x4)` @ 0x9375aa0) compute
    `1.0f / sqrt(y² + x² + z² + w²)` and multiply, with no branch; the guarded form is a different
    function, `math.normalizesafe`. An earlier version of this port had the guard on `normalize` and
    no `normalizesafe`, which is the wrong way round, so a zero-length quaternion now yields
    infinities here exactly as it does in the build.
31. **`LookRotationSafe`'s validity test is not `math.min`/`math.max`.** Both bodies fold the three
    squared lengths with `if !isnan(b) && b <= a { a = b }`, which keeps `a` when `b` is NaN where
    `math.min` would take `b`. The port reproduces the fold, the `kEpsilon`/`kMaxFloat` bounds read
    from VA 0x18cf28588 and 0x18cf295b0, and the identity fallback — the identity **quaternion** for
    `quaternion.LookRotationSafe`, not `quaternion(float3x3)` of the identity matrix, because in the
    degenerate case the basis it computed is already NaN.
32. **`fastinverse` is ported for `float4x4` only.** The dump declares four overloads
    (`float3x4`, `float4x4`, `double3x4`, `double4x4`); the other three are still unread shuffle
    unrolls. `math.fastinverse` is one member in the ledger, so the gap is closed, but the port is
    not complete for the name.
33. **`math`'s `RigidTransform` helpers are inherent methods.** Rust has no overloading, so
    `math.mul(RigidTransform, float4)` is `RigidTransform::mul_v4` and `math.float4x4(t)` is
    `RigidTransform::to_float4x4`; `math.hash`/`math.hashwide` for this type are `hash_rigid` and
    `hashwide_rigid` because the `boolN` forms already hold those names. The C# spelling is in each
    doc comment.
34. **`ToString` is now the shipped literal, and it was not before.** Every type's `Display`
    reproduces the format string the build actually formats with — `float3(1.5f, -2.5f, 3f)`, the
    matrices row by row with two spaces between rows, the `f` only on the `float` families. Until
    this pass the port printed bare components separated by spaces, which matches nothing in the
    build; the literals are in `evidence/math_tostring.tsv` and the generator fails if a type cannot
    find its own.
35. **`float3x3` and `float4x4` exist twice in the crate.** The generator emits the whole grid the
    dump lists, while `matrix.rs` hand-writes those two shapes because they carry the projection and
    view constructors. Only the hand-written ones are re-exported, so the generated copies are
    crate-internal duplicates. Closing this means either generating those methods too or excluding
    the two shapes from the grid.

## Port-side size

| Crate | Source lines | Tests |
| --- | --- | --- |
| `rrising-math` | 3,374 (+ 855 generator, + 40,590 generated) | 1,662 lines, 58 tests |
| `rrising-render` | 945 (+ 62 WGSL) | 108 lines, 1 test |
| `rrising-content` | 1,094 | 868 lines, 11 tests |
| `rrising-app` | 439 | — |
| `rrising-core` | 106 | — |
| `rrising-ecs` | 2,546 | 199 lines, 9 tests |

`rustup run 1.99.0 cargo test --workspace` → **79 tests, 0 failures** (58 math: 13 parity, 14
`math` statics, 10 `half` family, 6 Euler, 13 `RigidTransform`, 2 `ToString`; 4 content,
1 texture decode, 1 index fingerprint, 5 entity scenes, 9 ECS primitives, 1 renderer triangle).

## 2026-11-16 continuation

Current verification, limitations and next acceptance boundaries are recorded in
[COMPATIBILITY.md](COMPATIBILITY.md). Historical counts above were not all current:
this session ran 82 passing non-GPU tests before the sandbox GPU failure. No live
original-runtime parity or performance comparison has been made.

Added portable app path defaults and environment overrides, positive frame-size
validation, a JSON name-match report, and a fresh native offscreen screenshot.
No new gameplay systems have been claimed. The browser workspace build currently
fails in the native-only Oodle dependency; this is an observed build failure,
not browser support. Raw decompiled sources and game binaries are excluded from Git.


## Browser and publication preparation — 2026-11-16

The owner confirmed rights-holder permission. Added a shared renderer with optional
Unity adapters, native local-scene export/import, an asynchronous WebAssembly host,
local browser file selection and a WebGPU screenshot. Browser and native both
render the original 48-object showcase; neither executes original gameplay.
Reference build SHA-256 identities are pinned in reference.lock.json.

Both the synthetic JSON-transfer GPU regression and the real-scene PNG comparison
passed. Whole-workspace WASM compatibility is not claimed: only rrising-web's
dependency graph is browser-ready. Strict Clippy's existing math-generator lints
remain open. Detailed commands and known gaps are in COMPATIBILITY.md.

## Goal continuation: chunk correctness and runtime metadata

The full port remains the active goal. Two original native leaf functions now
have an executable oracle: [original_leaf_oracles.md](../evidence/original_leaf_oracles.md).
1,024 inputs per function execute the original x86-64 bytes under Rosetta and
match Rust outputs. This is narrower than running the original game.

Fixed two existing errors: TypeIndex's reference/serialization bits are inverted;
Chunk.SequenceNumber is at bytes 40..48, not 36..44, and short header buffers are
refused without writes. The archetype managed-reference branch keeps its original
raw bit condition after the getter correction.

Ported ArchetypeChunkData's type-major change/shared arrays, chunk-major masks
and disabled counts, exact Grow capacity, order-version exclusion in
SetAllChangeVersion, Add, RemoveAtSwapBack and Dispose. Native allocation is
replaced by safe vectors; unused capacity is zeroed, not uninitialized. Pointer
identity, MoveChunks and cached-list operations are not claimed. This metadata
is not a full EntityComponentStore or world scheduler.


## Storage layouts from the pinned binary

Offline IL2CPP inspection now resolves 1,493 / 1,772 stable type hashes observed
in scenes. `tools/probe_type_registry.py --check` regenerates a local JSON report;
no original game payload or raw type dump is committed. The read-only layout
bridge validates Entity and buffer storage before exposing component slices.
Exact current counts and limitations are in COMPATIBILITY.md.

Two original layout details repaired: full TypeIndex flags sort before stable
hashes; 32,704 aligned bytes fit a chunk exactly. With these corrections, all
admitted layouts across 1,183 files pass Entity uniqueness, buffer capacity and
external-allocation checks (6,542 chunks). The other archetypes stay unresolved.

`rrising-app --inspect-scene file.entities --type-layouts report.json` wires this
path into the executable and reports counts plus up to eight original transform
samples. It is explicitly read-only and has no simulation/world claim.

## 2026-11-17 continuation after Claude's implementation

Reviewed commits through `b9e6da0`; current architecture, scene-World, transform and render
evidence supersede the historical counts/limitations above. The finished `--scene-sim` viewer
integration and `--compare` evidence are in COMPATIBILITY.md and
`docs/verification/scene-transform-render.json`. Portable tests and the original-data transform
scenario were rerun; empty GPU comparisons are now refused.

Recovered and replayed the two original ChangeVersionUtility bodies before correcting the new
schedule's unsigned comparison. The wrapping signed difference and zero sentinel now match
all 2,048 original cases each, with an actual transform-hierarchy regression. Golden inventory
is reproducible with `tools/oracle_inventory.py`; it counts 92 distinct addresses and explicitly
records the normalization sqrt stub. These are bounded function checks, not game completion.
The full port goal remains active. Next acceptance: more original runtime system behavior and
live world/input/render integration, followed by a reproducible original-game scenario. Gameplay,
physics, audio, networking, saves and full-game performance comparison remain unimplemented.


The device-compatibility continuation adds CPU BC fallback and fixes BC6H/BC7 texel orientation,
unsigned Unity BC6H and partial block-row mip chains. Native GPU tests, WASM, and Chrome loading
of the same original scene with and without BC enabled passed. Five CPU tests and the dedicated
640-block GPU comparison document precision limits; physical mobile devices remain untested.
See COMPATIBILITY.md and docs/verification/bc-textures.json. No new game-parity percentage claimed.


Movement continuation: native waypoint selector plus the original shared steering job and
normalizer now have 4,096 controlled cases each, and all shared-job cases run through the Rust
World/schedule. The job oracle substitutes five component accessors and the sqrt wrapper; it does
not execute original ECS lookup or scheduling. The loader resolves special type identities so
default queries exclude baked Prefabs. The viewer runs shared steering before transforms when
live query candidates exist, but all 32 candidates in shipped scenes are inactive templates.
See evidence/movement_contract.md. Next acceptance is live runtime state, buff instantiation,
locomotion/navigation/collision and an original input-driven comparison. No gameplay parity claim.

CrossOver continuation: the original dedicated server now reaches world startup and produces
six autosaves with persistence debug dumps. The additional server image has its own pinned lock;
client RVAs are not reused. A fresh bottle, cloned install and local profile are created by
tools/probe_crossover_server.py. Raw outputs stay outside Git. See evidence/crossover_runtime.md
for measured results and the Steam initialization error. This removes the server launch blocker,
but does not establish deterministic gameplay or save-loading support in Rust.

The following slice decodes the outer version-2 persistence container in rrising-content and
connects it to the app through --inspect-save. Two original autosaves pass all fourteen member
range/payload fingerprint comparisons; empty jobs and exact table order are retained. The reader
uses the already-pinned flate2 dependency with its Rust backend. Its bounded, strict malformed-file
policy differs from native ReadBytes/EOF handling and is documented in evidence/save_container.md.
No entity/component/system restoration or save writing is implemented by this slice.

The next slice decodes format-7/persistence-4 Header.save completely. Original save output
confirms prefab/link handles, the saved component registry and blob mappings; a test-only
encoding of all parsed fields matches the 821,700 original bytes in both files. Required names
use nine-bit lengths followed by alignment, not u16 lengths. Runtime type indices retain flags
and are not installed into the port registry. See evidence/save_header.md.

The serialization-job reader then indexes archetypes, chunks, saved entity handles and scalar
or dynamic-buffer columns. It consumes all eight job members in two original autosaves to their
exact ends, including two empty members. The original-data check locates saved `EntityInput` and
`MapCollision` values by entity handle and compares fingerprints. See evidence/save_jobs.md.
The version table, nonpersistent diagnostics and system record envelope are
decoded. Native loading allocates fresh handles and binds saved prefab GUIDs
to current prefab entities before remapping references. The
[binding stage](../evidence/save_bindings.md) accepts fresh handles and current
prefab groups. The [server scene reader](../evidence/server_prefab_catalog.md)
recovers all 23,583 catalog registrations and linked group sizes. Installing
those scenes with live handles, restoring typed state and comparing replays remain.

Shared-value continuation: REA `search_procedures` on the 250 MB server GameAssembly timed out
(330 s provider limit), and no headless Ghidra is installed locally. The client's
`LoadPersistenceSystemV2.DeserializeLoadFile` (client RVA 0x7e472a0) is the candidate routine for
shared-value restoration, but its pseudo-code was not yet resolved to a rule. Shared-value restore
therefore stays unmeasured; see evidence/save_bindings.md. No parity claim changes.

Save-restore continuation (2026-11-25): Ghidra 12.2 and Il2CppDumper 6.7.46 now recover names and
RVAs of the pinned server image (Cpp2IL 2022.1.0-pre-21 cannot find the registration structs for
metadata 31.1). Loader path for shared and excluded components, the exclusion predicate, the
`ExcludeComponent` option semantics, and a structural match of the saved archetype index against the
original's own debug dump (136/136 archetypes, 133/133 byte spans, one autosave) are in
evidence/save_bindings.md. Unresolved: about 50 of 125 excluded names, the excluded-assembly set, and
shared/prefab value reproduction. Still unmeasured: any behavior, content-value, launch or
performance parity with the original; original-game scenarios remain 0.
Next acceptance: one scenario that runs on both the original (server through CrossOver) and the Rust
runtime from the same restored state and compares observable output.

## Rust-only save decode timing (not a comparison)

`r-rising --inspect-save AutoSave_3.save.gz` (7.6 MB gzip from the pinned original server, release
build, Apple Silicon host) takes 0.11 s wall time over five runs: decompress, decode header, jobs,
world data and systems. There is **no matching original-side figure**: the original server loads under
CrossOver/Rosetta with startup dominated by Wine and scene streaming, and its log does not isolate
save-load time. This number must not be read as a speed-up over the original.

## Windows diagnostic continuation (2026-11-27)

The local Windows client GameAssembly and metadata hashes match `reference.lock.json`.
`cargo check --locked -p rrising-app` and `cargo build --locked -p rrising-app` passed.
The layout probe matched 1,772 of 1,772 scene hashes. The new PowerShell launcher
checks the pinned hashes and keeps its generated layout under ignored `target/`.
An offscreen frame with the partial player and sample 6 of the walk clip rendered
at 1280x720: 9,474 scene mesh instances, 316 draw groups and 313 bound textures;
see [Windows diagnostic frame](screenshots/windows-player-diagnostic.png). This is
not a gameplay comparison. A pinned-data synthetic W press/release test passed
(1/1); it did not test operating-system key events. Original gameplay scenarios
remain 0, and no original-side performance comparison was performed.

Follow-up: in the actual Windows window a short W press previously arrived and was released
before a redraw, producing no movement. The diagnostic now retains a movement press until a
simulation update; the same injected press produced both OS events and moved Z from 22.5 to
22.633335. A pinned-data test covers a press/release before the simulation step. The latch
coalesces events and may apply them to up to four catch-up steps, so it is not an original input
timing claim. With experimental movement, diagnostic animation now chooses the serialized rest
pose when stationary and advances walk samples with horizontal displacement. This policy is not
the original Animator or idle animation. Gameplay parity and comparative performance remain unmeasured.
Windows validation: `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`
and `cargo test --locked` passed; the workspace run includes ignored tests. The separately
enabled pinned-data keyboard test passed, as did `original_walk_samples_update_the_gpu_render`
(3,344 pixels differed between two walk samples). Neither is an original runtime comparison.

Player material continuation: the candidate body and cape have one submesh and one material
each. Their serialized `_BaseColorMap` pointers now feed the normal scene texture path, with
identity UV scale/offset checked. On the pinned client both references decoded in the player
integration test; an offscreen Windows frame bound 315 textured groups (previously 313), with
no texture upload fallback. The cloak uses a debug-grid material, not a verified gameplay
selection; its runtime replacement, head attachment and original shaders remain unresolved.
