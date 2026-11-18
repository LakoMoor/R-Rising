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
