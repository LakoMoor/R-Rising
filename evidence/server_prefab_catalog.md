# Dedicated server scene layouts and prefab registrations

The isolated CrossOver boot loaded 11 GameData scenes; nine registered
23,583 prefabs. These same nine `.0.entities` files supply every prefab GUID
and linked group size in both retained autosaves. The integration test in
`crates/rrising-content/tests/prefab_catalog.rs` lists the runtime-selected
files and compares every registration with both save headers.

The dedicated server uses 65,536-byte chunks with a 64-byte header. The client
uses 32,768-byte chunks. In the pinned server, `WriteChunks` at RVA
`0x90a4920` writes blocks of `0x10000` bytes; `CreateArchetype` at
`0x906ee70` uses a payload size of `0xffc0`. Rust uses the size of the
chunk stream and the number of groups in BufferData to distinguish both
formats. For all 372 server files, the layout pass resolved all 1,390 scene
type hashes, 10,464 archetypes and 14,128 chunks containing 512,967 rows.

The server producer chain is `PrefabCollectionSystem.OnUpdate` (RVA
`0x7aacaa0`), `RegisterGameData` (`0x7aad8f0`), then `RegisterPrefabs`
(`0x7aae970`). The registration row contributes an i32 GUID from
`Stunlock.Core.RegisterPrefabEvent` and a scene-local entity at byte 656 of
the 664-byte `Stunlock.Core.RegisterPrefab` component. Rust follows that
handle to `Unity.Entities.LinkedEntityGroup`, reading inline or external
buffers in serialized order. A scan of all prefab-tagged rows would also
include extra rows the runtime does not register.

```sh
python3 tools/probe_type_registry.py --server --install /path/to/VRising_Server \
  --check --output /tmp/rrising-server-layouts.json
VRISING_SERVER_DATA=/path/to/VRisingServer_Data \
RRISING_SERVER_LAYOUT_REPORT=/tmp/rrising-server-layouts.json \
RRISING_SAVE_DIR=/path/to/captured/saves \
  cargo test --locked -p rrising-content --test prefab_catalog -- --include-ignored
```

Each catalog contains scene-local handles. A live ECS still has to load and
remap the scene entities before joining the catalogs or using them for save
restoration. The test compares group sizes and validates that group entries
refer to their source scene; it does not compare newly allocated runtime
handles or restore the game.

The live ECS projection allocates fresh handles for 41,205 prefab roots and
children and for every persistent job row. It restores scalar columns for
`ProjectM.Buff`, `ProjectM.EntityOwner`, `ProjectM.EntityInput` and
`ProjectM.MapCollision` (157 values in save 4 and 156 in save 5). The
integration test verifies the liveness of every mapped entity and checks
rewritten fields in ECS storage, including a buff target and an owner that
points to a scene prefab. The same test checks all 1,772 prefab linked groups
and restores 3,153 saved buffer rows (142 elements) in each autosave, across
`Unity.Entities.LinkedEntityGroup` and `ProjectM.MegaStaticDestroyedBuffer`.
Linked handles are rebound; destroyed-static elements match original bytes.
The store owns overflow separately from chunk bytes, so compaction preserves
payloads and destruction releases them without using serialized pointers.
Other archetype members, prefab component bodies, system state and gameplay
scheduling remain outside the projection.

To run the projection from the native CLI after generating the layout report:

```sh
cargo run --locked -p rrising-app -- \
  --data /path/to/VRisingServer_Data --type-layouts /tmp/rrising-server-layouts.json \
  --inspect-save /path/to/AutoSave_4.save.gz --restore-projection
```

The report includes an `ecs_projection` summary and still reports
`world_restored: false`.

A freshly generated server layout report also identifies value types whose
nested fields have no managed references, native addresses or unknown
storage. The loader admits observed columns only when their stable hash,
serialized stride, buffer category and entity-reference flag agree with the
server layout. It leaves blob-bearing types and types that require managed,
cleanup, enable-mask or chunk-component handling out of this projection.
Older reports without this classification still select the six individually
verified types.

| Original save | Admitted / observed types with columns | Scalar rows | Buffer rows | Buffer elements |
| --- | ---: | ---: | ---: | ---: |
| 4 | 263 / 283 | 1,826,360 | 276,642 | 794,036 |
| 5 | 263 / 281 | 1,826,352 | 276,642 | 793,963 |

Counts are projected values, not counts of implemented component behavior.
Integration tests compare live translation and velocity bytes along with the
previously checked references and buffers. The projected map leaves 471,190
reference occurrences unresolved in each save. They are cleared by the
inspected loader rule for a missing binding; whether the original would have
additional bindings has not been established. A full boot and restore,
including scene entities beyond this prefab projection, remains unverified.

The [MegaStatic reference audit](save_mega_statics.md) traces the unresolved
references and derives prefab and placement keys for the missing instances.

An independent prefab clone projection now exercises the registered groups
that the captured MegaStatic instances do not use. The pinned native
`InstantiateEntitiesGroup` (RVA `0x907bc10`) allocates clone rows and
uses `PatchEntitiesForPrefab` (`0x91901d0`) to rewrite references.
The Rust path records in-group target ordinals from recovered scalar and
buffer layouts. It allocates fresh handles before copying values, rebuilds
the root's LinkedEntityGroup, and rewrites each supported field using the
handles of that clone. Null references remain null. Values whose references
leave the group are omitted until external entity mapping is available.
Tags, special storage, pointers, network state, and runtime gameplay
remain outside this projection.

Tests clone all 1,772 original linked groups twice (38,788 live rows).
They compare 293,848 supported values and 39,500 remapped fields, including
9,790 fields inside dynamic buffers. This is a test against original scene
data, not a native execution replay. It does not insert any of those groups
into the restored autosaves.

```sh
cargo run --locked -p rrising-app -- \
  --data /path/to/VRisingServer_Data --type-layouts /tmp/rrising-server-layouts.json \
  --clone-prefab -609878016
```

The example checks a four-member group and reports 69 copied values and
10 remapped references.
