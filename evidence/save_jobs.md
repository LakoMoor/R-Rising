# Persistence jobs — saved entities and component columns

Rust reads the four `SerializationJob_*.save` members in each of two original
dedicated-server autosaves [captured through CrossOver](crossover_runtime.md).
This adds archetype, chunk, entity and component data to the previously decoded
[container](save_container.md) and [header](save_header.md). It does not apply
those values to a running World.

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
still reports `false`.
Typed game state, shared values and omitted values, entity/blob remapping,
`WorldData` and `Systems` decoding, and saved-world restoration are outside
this boundary. No `MoveTowardsPositionBuff` column appears in either capture;
the `EntityInput` bytes are not a live player or steering scenario.
