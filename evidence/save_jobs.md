# Persistence jobs — saved entities and component columns

Rust reads the four `SerializationJob_*.save` members in each of two original
dedicated-server autosaves [captured through CrossOver](crossover_runtime.md).
This adds archetype, chunk, entity and component data to the previously decoded
[container](save_container.md) and [header](save_header.md). It does not apply
those values to a running World.

## Cpp2IL metadata cross-check

Cpp2IL `2022.1.0-pre-release.21` identifies metadata version 31.1 on the
pinned server. Its automatic search found the code registration at
`0x18a291870` but did not find metadata registration. The independently
recovered registration at `0x18cc02e60` allows initialization and maps
227,563 method definitions. The original release disables interactive
address entry in its CLI. A local build with that existing option enabled
was used to provide the address. Its `diffable-cs` output contains empty
method bodies, so it supports identity and signature lookup only. Native
instructions and actual save bytes remain the behavioral evidence. The
output stays local.

## Format evidence

The target is the dedicated server in [reference.server.lock.json](../reference.server.lock.json),
GameAssembly SHA-256 `41ba4eb2…`. The format was traced in the pinned server
instructions and checked independently against the original save bytes.

| Original method | Server RVA |
| --- | --- |
| `LoadPersistenceSystemV2.DeserializeLoadFile` | `0x7f0b760` |
| `SerializePersistentChunksJob.Execute` | `0x7f301a0` |
| `SerializePersistentChunksJob.OptimizeArchetypeComponents` | `0x7f30c00` |

A job has no version prefix. It stores archetypes consecutively to the end of
the member. Each archetype begins with a signed i32 type count, the full saved
type indices in column order, four omission bits (least significant first), and
an i32 chunk count starting four bits into its byte. Within each chunk the i32
entity count is followed by byte alignment and the entity handle array. Each
handle is an i32 index and i32 version. The first entity count starts four bits
into its byte; later chunks start at a byte boundary.

A component has a column only if its exact packed type index has a persistent
record in the header and it is neither shared, zero size, nor omitted by a flag.
Types without a column still belong to the archetype. Fixed-size values are
stored consecutively, one value per entity. A dynamic buffer instead stores a
signed i32 element count for each entity, followed by its byte-aligned elements.
The saved component size is the stride in both cases.

| Omission bit | Types omitted from chunk data |
| ---: | --- |
| 0 | `Unity.Transforms.LocalToWorld` |
| 1 | `Unity.Transforms.LocalTransform` |
| 2 | `Unity.Transforms.Translation`, `Rotation`, `CompositeScale` |
| 3 | `ProjectM.DestroyData`, `DestroyState`, `PlacementDestroyData` |

The original samples exercise masks `0x0`, `0x1`, `0x8`, `0x9`, `0xa`, and `0xb`;
the bit-2 rule comes from the native reader/writer inspection, not an observed
archetype with bit 2 set.

## Measured against original saves

The independent scan consumed each member to its exact end. The fourth job is
empty in both saves. Counts are rows within each save, not unique entities
across saves.

| Autosave | Job | Bytes consumed | Archetypes | Chunks | Entities | Component columns | Per-entity buffer records |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `AutoSave_4` | 0 | 15,867,153 | 16 | 1,872 | 224,552 | 14,936 | 225,743 |
| `AutoSave_4` | 1 | 12,068,726 | 37 | 693 | 46,115 | 4,563 | 47,621 |
| `AutoSave_4` | 2 | 578,231 | 87 | 108 | 3,571 | 1,253 | 5,460 |
| `AutoSave_4` | 3 | 0 | 0 | 0 | 0 | 0 | 0 |
| `AutoSave_5` | 0 | 15,866,088 | 15 | 1,871 | 224,551 | 14,934 | 225,742 |
| `AutoSave_5` | 1 | 12,068,726 | 37 | 693 | 46,115 | 4,563 | 47,621 |
| `AutoSave_5` | 2 | 578,054 | 86 | 107 | 3,570 | 1,245 | 5,460 |
| `AutoSave_5` | 3 | 0 | 0 | 0 | 0 | 0 | 0 |

Both captures contain the same sample for entity `(152830, 4)` in job 2: the
96-byte `ProjectM.EntityInput` value starts at job offset 542,777 and the 20-byte
`ProjectM.MapCollision` value starts at 543,694. A chunk in job 1 also supplies
an original `ProjectM.MegaStaticDestroyedBuffer` sample: of 128 entities, two
have nonempty buffers, with five elements across the chunk. The Rust integration
test uses the [derived job oracle](oracles/save_jobs.json) to check offsets,
counts and sample fingerprints in both original files.
The public evidence contains counts and fingerprints; the original binary data
and autosaves stay local.

```sh
RRISING_SAVE_DIR=/path/to/captured/saves \
  cargo test --locked -p rrising-content --test persistence_jobs -- --ignored --nocapture
cargo run --locked -p rrising-app -- --inspect-save /path/to/AutoSave_4.save.gz
```

`--inspect-save` lists the jobs and their per-member counts without opening a
window or loading game archives. The reader borrows the entity and component
bytes and uses a 64 MiB per-job budget for index entries. It rejects truncated
ranges, negative counts and unknown saved type indices; malformed-file
behavior has not been compared with the original loader. `world_restored`
still reports `false`. The snapshot indexes job rows by their original slot
and version, validates all job handles and header prefab references against
the [world version table](save_world.md), and keeps the component payloads
borrowed. The same report lists the [saved system records](save_systems.md).
The 284,158 nonpersistent diagnostic slots are disjoint from the job slots in
each capture; they contain all 23,583 header prefab roots and 19,394 linked
entity references.

The original loader allocates fresh handles with `EntityManager.CreateEntity`
(call site `0x7f0c3fa`). `SetupOldToNewEntityRemappingHashMap` (`0x7f0f050`)
pairs saved and fresh handles and binds prefab roots and children. This is why
the offline slot index cannot directly serve as a live-world import. See
[saved entity bindings](save_bindings.md) for the implemented mapping stage.

Typed game state, shared and omitted values, entity/blob remapping, and
saved-world restoration are outside this boundary. No
`MoveTowardsPositionBuff` column appears in either capture;
the `EntityInput` bytes are not a live player or steering scenario.
