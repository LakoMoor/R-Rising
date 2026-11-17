# Entity references in original persistence columns

The target is the [pinned dedicated server](../reference.server.lock.json),
GameAssembly SHA-256 `41ba4eb27c53dd9dc1a3d3337a2c8550b8386421361792ee6ac159ef00ff0ac7`.
This slice identifies several saved references and the native missing-mapping
policy. It does not reproduce the original destination-world mapping or prove
runtime restoration. Two [CrossOver autosaves](save_container.md) supply the
observed saved values, not observations of the original loader applying them.

## Saved component layouts and observations

Field types and offsets come from the server IL2CPP type definitions and its
parallel native field offset tables. For these value types, subtract the
16-byte boxed object header from the metadata field offset. The saved
[header registry](save_header.md) independently gives the width and the exact
packed type index. The 96-byte serialized `EntityInput` stride comes from the
header; it must not be inferred by summing apparent field sizes.

| Type | Stable hash | Saved TypeIndex | Width | Entity offset |
| --- | --- | --- | ---: | --- |
| `ProjectM.Buff` | `5c0689788602b4e5` | `0x1015` | 32 | `Target` at 8 |
| `ProjectM.EntityOwner` | `4a55fea909ee9afc` | `0xfe6` | 8 | `Owner` at 0 |
| `Unity.Entities.LinkedEntityGroup` | `bea79ca45e4be521` | `0x4001767` | 8 per element | `Value` at 0 |
| `ProjectM.EntityInput` | `cf49a498d7474572` | `0x1151` | 96 | `HoveredEntity` at 48 |

Reference fields hold two little-endian i32 values (slot, version), including
the null pair (0, 0). An independent traversal used the
[job framing](save_jobs.md) to locate each typed column, read each field at its
value offset, and classified each non-null handle against the
[world version table and debug listing](save_world.md) and the saved job
handles. For the selected types no non-null versions disagreed with the world
version table, and every non-null handle belonged to one of the two saved
lists. Those lists had no overlapping slot indices in these captures.

| Type | Autosave 4 | Autosave 5 |
| --- | --- | --- |
| `Buff.Target` | 3 persistent | 3 persistent |
| `EntityOwner.Owner` | 83 persistent, 68 null, 1 nonpersistent | 83 persistent, 68 null |
| `LinkedEntityGroup.Value` | 136 persistent across 52 buffers | 136 persistent across 52 buffers |
| `EntityInput.HoveredEntity` | 1 null | 1 null |

Example byte offsets are relative to the decompressed job member:

| Component field | Member | Owner | Byte offset | Target |
| --- | --- | --- | ---: | --- |
| `Buff.Target` | `SerializationJob_2.save` | (152839, 4) | 546,784 | (152830, 4) |
| `EntityOwner.Owner`, autosave 4 only | `SerializationJob_2.save` | (559509, 3) | 578,155 | (36976, 1) |
| First `LinkedEntityGroup.Value` sample | `SerializationJob_1.save` | (86390, 3) | 11,958,464 | (86390, 3) |
| `EntityInput.HoveredEntity` | `SerializationJob_2.save` | (152830, 4) | 542,825 | (0, 0) |

The nonpersistent target (36976, 1) is also the root of prefab GUID 2016632390,
at zero-based header prefab ordinal 8,548 in both captures, with no linked
handles. A persistent-only index would miss that target. The debug listing
does not say which current-world handle should replace it.

## Native offset source and remap boundary

| Server RVA | Observed role |
| --- | --- |
| `0x7f0cd10` | `LoadPersistenceSystemV2.RemapEntities` accepts old-to-new and new-to-old maps. Its call at `0x7f0d2f7` supplies missing-remapping mode 1. |
| `0x245f7a0` | `SLSFieldRemapping.RemapFields<EntityFieldLookup, Entity>` (MethodSpec 929391 from metadata usage VA `0x18dbd0ed8`). Iterates selected types and routes by buffer flag. |
| `0x2a6c20` / `0x2a69b0` | Scalar-chunk and buffer-chunk paths converge on the same field-level remapper. |
| `0x910e8d0` / `0x910e270` | TypeManager type table and entity offset array pointers. |
| `0x245e730` | Field-level remapper resolves each offset and looks up the entity handle. |

The native TypeManager uses a 96-byte `TypeInfo` record indexed by the low 14
bits of the resolved runtime TypeIndex. `EntityOffsetCount` and
`EntityOffsetStartIndex` are at unboxed offsets `0x30` and `0x34`. Offset
records are signed i32 bytes from the component or buffer element start;
the remapper caches their pointer and count in `EntityFieldLookup`. The saved
TypeIndex is not an index into the current runtime table.

The leaf reads the full 64-bit field and passes it to the lookup at
`0x245e88e`. On success it writes the returned 64-bit value. On failure it
can record the owner/type/missing field, then checks the mode at
`0x245e8e8`: mode 0 keeps the old field, mode 1 zeroes the entire field at
`0x245e8fa`, and other values reach an exception path. The final write is at
`0x245e919`. This is static instruction evidence; neither missing-map
behavior nor prefab remapping was replayed in the original for these saves.
The clear-on-miss branch describes a lookup after constructing the native
map, not a rule to clear any reference absent from the persistent job list.
