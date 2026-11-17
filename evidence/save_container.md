# Original persistence container — version 2

The new `rrising-content::persistence::PersistentSave` reads the outer container
of the two actual original-server autosaves captured through CrossOver. It exposes
member slices and metadata; it does **not** restore entities, systems, a game world
or player progress. Original payloads and debug dumps are kept outside Git.

## Original evidence

All native addresses below belong to the **server** image pinned in
[reference.server.lock.json](../reference.server.lock.json), SHA-256 `41ba4eb2…`.
These are metadata/native-instruction findings, not new isolated-function replays.

| Original boundary | Server RVA | Observed role |
| --- | --- | --- |
| `RawPersistentSaveData.LoadSaveDataAtPath` | `0x7f1e1b0` | A `.gz` suffix chooses gzip; a file's outer version must be 2 |
| `RawPersistentSaveData.ReadVersion1` | `0x7f1e660` | Legacy directory format; not implemented here |
| `RawPersistentSaveData.ReadVersion2` | `0x7f1e9e0` | Reads the complete member table, then sequential payloads |
| `RawPersistentSaveData.ValidateRequiredFiles` | `0x7f1f2b0` | Header, world and systems plus at least one serialization job |
| `SerializePersistenceSystemV2.ScheduleSerializationJobs` | `0x7f283a0` | Populates four job slots, then Header, WorldData and Systems |
| `WriteAndZipToDiskJob.Execute` | `0x7f31d70` | Writes present slots in ascending order |
| `WriteAndZipToDiskJob.WriteFileEntry` | `0x7f32cb0` | Writes string name and signed byte length |
| `FileEntry.get_EntryExists` | `0x7f330f0` | Name/buffer presence, including a zero-length allocated buffer |

After gzip decompression, the format is:

```text
i32 little-endian version (2)
i32 little-endian member count
for each member: .NET 7-bit byte length, UTF-8 name, i32 little-endian data length
for each member: exactly data length bytes, in table order
```

The string has no NUL terminator or alignment. The writer converts buffer bits to
bytes with signed `(LengthBits + 7) >> 3`. No inner checksum/padding was found.
The native reader allows additional names and any table order. It rejects duplicate
names through `Dictionary.Add`; `SerializationJob_` members retain table order.
The Rust reader follows these structural rules and accepts empty jobs.

## Measured files and Rust validation

[Captured metadata](oracles/save_containers.json) records independent Python gzip
decoding and member ranges, SHA-256 and FNV-1a fingerprints. It contains no member
payload bytes. Rust's ignored original-data test reads both private files and
matches all 14 ranges and payload fingerprints; ordinary CI tests use synthetic data.

| Original file | Compressed bytes | Complete decompressed bytes | Table | Members |
| --- | ---: | ---: | ---: | ---: |
| AutoSave_4.save.gz | 7,592,269 | 35,990,082 | 172 | 7 |
| AutoSave_5.save.gz | 7,592,236 | 35,988,840 | 172 | 7 |

The original log's `TotalUncompressedSaveSize` excludes the table's 172 bytes;
the values above include it. The seven names are `SerializationJob_0.save`
through `_3.save`, `Header.save`, `WorldData.save`, `Systems.save`. The empty
job's offset equals Header's offset, as expected. The header is identical in both
files; the world and active-job payloads differ as the original simulation runs.

```sh
cargo run -p rrising-app -- --inspect-save /path/to/AutoSave_4.save.gz
cargo test -p rrising-content --test persistence
RRISING_SAVE_DIR=/path/to/captured/saves \
  cargo test -p rrising-content --test persistence -- --ignored --nocapture
```

`--inspect-save` runs without opening a window/GPU or indexing game archives. It
prints the version, table size, uncompressed size, ordered member names/ranges,
the [decoded header's versions and counts](save_header.md) and
[serialization-job counts](save_jobs.md).
It does not extract files. The normal file-loading API also accepts an already
decompressed version-2 container, matching the original case-sensitive `.gz` choice.

## Explicit limits

The diagnostic reader is stricter than the original on malformed files. It rejects
short payloads and trailing decompressed/compressed bytes, invalid UTF-8 and 7-bit
length overflow, and limits decompression to 512 MiB by default (the stream API
accepts a caller-selected limit). Native `ReadBytes` may return a shorter array;
ReadVersion2 does not check its length or EOF. The original 7-bit reader can truncate
overflow bits in its fifth byte. Error-policy parity is therefore not claimed.

Four portable tests cover the real table shape, unknown names/UTF-8 lengths,
incomplete/invalid tables and gzip checksum/truncation/limit handling. The two
original files also pass the actual application command. The header and job
readers recover saved registry metadata, entity handles and component columns.
World data, system state and legacy v1 directories remain opaque. These are format
checks, **not save/load gameplay parity**.
