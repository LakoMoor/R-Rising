# Persistence header — original runtime registry

Rust now decodes the complete `Header.save` member captured from the running
original server, in addition to the [outer save container](save_container.md).
This recovers saved prefab handles and type/blob metadata. It does not decode
serialized component values or restore the game World.

The target is the dedicated server in [reference.server.lock.json](../reference.server.lock.json),
GameAssembly SHA-256 `41ba4eb2…`. The two original autosaves have an identical
821,700-byte header, SHA-256 `d5b5de36ea97da62fa2f0c897785c16f83a1ff9164ff1ecde7caf162dbc23971`.
[Derived observations](oracles/save_header.json) contain counts, boundaries and
fingerprints, not the original header payload.

## Native contract

These identities and wire operations were established from original metadata and
focused server instruction inspection; no original header parser was executed in
isolation. Actual original save bytes provide the runtime output to test against.

| Original method | Server RVA |
| --- | --- |
| `PersistenceHeaderMetadata.Deserialize` / `Serialize` | `0x7f1cd20` / `0x7f1cef0` |
| `PersistenceHeader_V1.Deserialize` | `0x7f1d030` |
| `PersistentComponentMetadata.Deserialize` | `0x7f1dae0` |
| `RequiredComponentMetadata.Deserialize` | `0x7f1dc50` |
| `PersistentBlobMetadata.Deserialize` | `0x7f1de10` |
| `SerializeHeader_Job.OriginalLambdaBody` | `0x7f348b0` |

The 28-byte metadata prefix holds signed i32 serialization format **7**, persistence
version **4**, game version **1/1/13/99712**, three u8 enum values (Server=1,
DedicatedServer=2, SubSceneConversion=0), then a one-bit false server-subscenes flag
and byte alignment. Format 7 is checked by the native metadata reader. The current
Rust profile also requires persistence version 4; other persistence versions have
not been measured here.

Each remaining section begins with a signed i32 count:

| Section | Record fields | Count | Byte range |
| --- | --- | ---: | --- |
| OldPrefabs | i32 PrefabGUID, Entity(index/version i32), i32 linked count, linked Entity array | 23,583 | 28..532,512 |
| PersistentComponents | u64 stable hash, i32 component size, i32 full saved TypeIndex | 1,542 | 532,512..557,188 |
| RequiredComponents | FixedString512 name, i32 full saved TypeIndex | 2,240 | 557,188..663,840 |
| BlobAssets | Entity, u64 component hash, i32 field offset, i32 element index, saved IntPtr64 bits | 4,933 | 663,840..821,700 |

Prefab GUIDs can be negative. The linked arrays contain **19,394** handles in total.
`UNKNOWN_0` is a valid zero hash/size/index record. Saved TypeIndex flag bits are
retained; these indices must not be treated as the port's current registry indices.
The blob address is a saved identifier, never dereferenced as a live pointer. There
are 2,003 null saved addresses in this header.

The name length is **nine bits**, range 0..509, followed by alignment to a byte and
UTF-8 bytes. It appears like an LE u16 in normal files because seven padding bits
are zero. Rust masks those padding bits, as does the native bit-reader. Synthetic
tests use nonzero padding and a multibyte UTF-8 name to guard this distinction.

## Independent confirmation of movement layouts

All 1,542 saved persistent component indices join the required-name table exactly.
That supplies the following actual runtime records, independently confirming the
earlier client metadata/layout reconstruction:

| Type | Stable hash | Component size | Saved TypeIndex |
| --- | --- | ---: | --- |
| ProjectM.MoveTowardsPositionBuff | `809388619ae080d7` | 40 | `0x20be6` |
| ProjectM.Buff | `5c0689788602b4e5` | 32 | `0x1015` |
| ProjectM.MapCollision | `179eefa3019ca077` | 20 | `0x21096` |
| ProjectM.Velocity | `8da56f730a0264b0` | 12 | `0x21097` |
| ProjectM.EntityInput | `cf49a498d7474572` | 96 | `0x1151` |
| Unity.Transforms.Translation | `a42de7cc0763e5be` | 12 | `0x212d4` |

## Validation and boundary

`--inspect-save` now reports header versions and section counts along with outer
member metadata. The original-data test decodes both actual headers, verifies
captured counts/fingerprint, joins every persistent type, and checks all six
movement records. A test-only canonical encoding of **every parsed field** is
byte-identical to the original 821,700 bytes. This is not a production save writer
or an original-native-parser comparison on arbitrary inputs.

```sh
cargo test -p rrising-content --test persistence_header
RRISING_SAVE_DIR=/path/to/captured/saves \
  cargo test -p rrising-content --test persistence_header -- --ignored --nocapture
cargo run -p rrising-app -- --inspect-save /path/to/AutoSave_4.save.gz
```

Three portable tests cover schema/alignment/flags, complete truncation prefixes and
invalid versions/counts, and the 509-byte UTF-8 capacity. Counts are bounded by
remaining bytes and EOF is required. Strict malformed-file UTF-8/length/EOF policy
is diagnostic; native List deserialization has no equivalent complete validation.
Uniqueness and cross-table joins are asserted on the measured files in tests,
not assumed to have been checked by the original parser.

WorldData, Systems and serialization-job payloads remain to decode, including
buffer/blob contents and entity remapping. No saved world is runnable in Rust yet.
