# Original saved entity versions and nonpersistent world records

The dedicated server was run through CrossOver and produced two retained
[original autosaves](save_container.md). Each `WorldData.save` member is 6,652,241
bytes. The Rust reader decodes the entity version table and optional debug listing
of nonpersistent archetypes and chunks as borrowed views into those member bytes.

The pinned target is [reference.server.lock.json](../reference.server.lock.json),
GameAssembly SHA-256 `41ba4eb27c53dd9dc1a3d3337a2c8550b8386421361792ee6ac159ef00ff0ac7`.
IL2CPP method tokens and the matching module's pointer table identify the native
methods below. Static instruction inspection establishes the fields and their
widths; the actual original captures verify the counts, order and boundaries.

| Original method | Server RVA |
| --- | --- |
| `SerializePersistenceSystemV2.SerializeWorldDataJob.Execute` | `0x7f31270` |
| `LoadPersistenceSystemV2.DeserializeWorldData` | `0x7f15850` |

The writer copies the entity version array before inspecting the job's
`SerializeAdditionalDebugData` flag. IL2CPP field identities for the debug
iterables are `NonPersistentArchetypesWithChunkCount` and
`NonPersistentArchetypeChunks`. Type hashes come from the TypeManager record;
these are stable hashes for the archetype's component types, not saved TypeIndex
values. Each chunk record contains entity indices without separate versions.
The version table is indexed independently by saved entity slot; its capacity
does not say which slots are live.

| Section | Encoding | Observed boundary |
| --- | --- | --- |
| Entity capacity | Signed 32-bit count | Bytes 0..4 |
| Entity versions | Capacity-many signed 32-bit values | Bytes 4..4,194,308 |
| Debug presence | Single low bit | Bit 33,554,464 |
| Debug listing, if present | i32 archetype count; for each archetype: i32 type count, u64 stable hashes, i32 chunk count; for each chunk: i32 index count, i32 entity indices | Bits 33,554,465..53,217,889 |
| Tail | Unused seven high bits in final payload byte, followed by four zero bytes in both captures | Bits 53,217,889..53,217,928 |

All debug words start at bit 1 of their corresponding byte, with the rest of the
word spanning the following bytes, in little-endian order. No interior byte
alignment follows the flag. The reader supports the flag-false path from static
inspection; both available runtime captures enabled it. The native reader's
counted traversal does not interpret the zero trailer. Its purpose is unknown;
Rust accepts either no additional bytes or the observed four zero bytes, and
ignores the seven alignment bits.

| Observation | Original autosave 4 | Original autosave 5 |
| --- | --- | --- |
| `WorldData.save` SHA-256 | `bd182c2d5b78533d141d93c8059d18b65deb263d45df3bc61b7e3eea1c2a068d` | `b65dfd8cac283b2ea6c6663b58dd557280db200518d2025b42aeb2860c08ca3d` |
| Entity capacity | 1,048,576 | 1,048,576 |
| Nonpersistent debug archetypes / chunks | 3,702 / 6,577 | 3,702 / 6,577 |
| Type hash entries / entity indices | 158,171 / 284,158 | 158,171 / 284,158 |
| Version at slot 152,847 | 11 | 13 |
| Versions at slots 559,505/559,507/559,509 | 7/3/3 | 9/6/6 |

The four listed versions are the only entries that changed between these captures;
the complete debug listing is identical. The first archetype has four hashes
and three chunks of sizes 128, 128 and 2. The final archetype has five hashes and
one chunk containing indices 2,013 through 2,019.

```sh
cargo test -p rrising-content --test persistence_world
RRISING_SAVE_DIR=/path/to/captured/saves \
  cargo test -p rrising-content --test persistence_world -- --ignored
```

The optional original-data test checks the fingerprints, counts, offsets and
version samples and byte-identically re-encodes both parsed members. No original
member data or executable is committed. The diagnostic reader bounds lengths,
allocations and indices; its malformed-input behavior has not been compared to
the native reader. The version array and the nonpersistent listing supply IDs
and structural information; they do not restore the runtime world.
