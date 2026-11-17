# Systems.save — saved clock and serializer records

The original dedicated server wrote a 1,859-byte `Systems.save` member with
eight populated records in each of the two [CrossOver autosaves](crossover_runtime.md).
Rust reads every envelope, borrows each serializer's bytes and reads the saved
world elapsed time and signed frame counter. Reading the member does not update
the live Rust world or run a serializer.

The checked target is [the pinned server](../reference.server.lock.json),
GameAssembly SHA-256 `41ba4eb2…`. Method identities come from the server
metadata and the wire operations from the corresponding native instructions.

| Original method or operation | Server RVA |
| --- | --- |
| `SerializePersistenceSystemV2.SerializeSystemData` | `0x7f27930` |
| `LoadPersistenceSystemV2.DeserializeSystemData` | `0x7f163b0` |
| `WorldUnmanaged.get_Time` | `0x9090680` |
| NetBuffer FixedString128 writer specialization | `0x1946c90` |
| NetBuffer FixedString128 reader specialization | `0x134faa0` |
| `WorldFrameSystem.Serialize` | `0x78a31d0` |
| `WorldFrameSystem.Deserialize` | `0x78a3390` |

The member starts with `WorldUnmanaged.Time.ElapsedTime`, an LE binary64, and
a signed i32 slot count. Each slot starts with a one-bit presence flag. When
the bit is set, seven low-to-high bits store the byte length of the system's
FixedString128 identifier (maximum 125 bytes), then byte alignment and the
UTF-8 identifier. A byte-aligned little-endian signed i32 specifies the
**payload length in bits**. The native writer aligns after the system emits
its payload before writing back the bit length, so the stored length is a
multiple of eight. The original reader tests the presence flag and advances
to the next slot on a false bit. Both captures contain only populated slots;
the Rust test for packed absent slots uses a synthetic fixture.

The native writer gets names from `IPersistentSystemSerializer.PersistentIdentifier`
and invokes each system's serializer through that interface. In the reader,
the name selects the deserializer and the recorded length bounds where parsing
resumes. Each record can be indexed without interpreting the system's custom
state. The `WorldFrame` serializer writes one signed i32 per its singleton's
native field type; the remaining seven systems are left as opaque records.

| Serializer identifier | Payload byte offset | Payload bytes |
| --- | ---: | ---: |
| `StablesSystem_MountItemGain_Persistence` | 56 | 8 |
| `ContestIdManagerSystem_Persistence` | 103 | 1,020 |
| `ServerBootstrapSystem` | 1,149 | 19 |
| `JewelSpawnSystem_Persistence` | 1,201 | 48 |
| `ActionRecordsPersistenceSystem` | 1,284 | 4 |
| `WarEventSystem_Persistence` | 1,319 | 26 |
| `WorldFrame` | 1,360 | 4 |
| `ModificationSystem` | 1,387 | 472 |

Both captures have the same record order, offsets and lengths. The saved
world elapsed time changes from about 57.581 s to 67.593 s and the frame from
3,871 to 4,162. Only the timer and frame bytes differ. The Rust test checks the
[derived fingerprints](oracles/save_systems.json) of every payload, then
re-encodes the envelope in test code and compares every byte to both original
members. It does not write a game save. Raw game data stays local.

```sh
cargo test --locked -p rrising-content --test persistence_systems
RRISING_SAVE_DIR=/path/to/captured/saves \
  cargo test --locked -p rrising-content --test persistence_systems -- --ignored --nocapture
```

The diagnostics reject truncation, negative or unaligned payload lengths,
names outside the FixedString128 capacity and non-UTF-8 names. Malformed-file
behavior is not compared against the original loader.
