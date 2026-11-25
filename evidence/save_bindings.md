# Binding saved entities to current handles

The pinned original server allocates new ECS entities when restoring job
rows (`DeserializeLoadFile`: call site `0x7f0c3fa`). It associates each
saved handle with the corresponding newly allocated handle before rewriting
references. `SetupOldToNewEntityRemappingHashMap` (`0x7f0f050`) builds both
mapping directions. Both keys include the handle version.

## Enable bits at job entity allocation

In both original autosaves, every job archetype declares the zero-sized
enableable type `Unity.Entities.Simulate` (saved index `0x410212eb`, server
stable hash `0x2d6324ec1523c9e0`). It is the only enableable type in these
job archetypes. All eight jobs across both saves consume their complete members
with no serialized `Simulate` column or per-row mask.

On the pinned dedicated server, `ChunkDataUtility.Allocate` calls
`InitializeComponents` at RVA `0x90ea222`. It looks up each enableable
component's mask and passes `true` to `UnsafeBitArray.SetBits` at
`0x90e8b87`. The Rust projection now includes `Simulate` in the saved job
archetypes when its server layout is available. The real-save integration
test checks the enabled bit on all 274,238 and 274,236 corresponding Rust
entities. This reproduces the allocation default. Later changes made by
the original system schedule have yet to be compared.

## Structural tags and default queries

The saved job archetypes also specify 48 ordinary zero-sized component
types. This includes `Unity.Entities.Disabled` (saved index `0x400212e8`,
server stable hash `0x9225a4b002d710ea`). Rust resolves these names against
the pinned server layout and requires the saved flag bits to agree. The
server probe now emits 349 supplemental layouts for zero-sized components
absent from the scene tables. The 1,390 scene-hash matches are unchanged.
Reports generated before the supplemental layouts existed can still be
read, with unrecovered types counted as skipped.

The server's `EntityQueryManager.IsMatchingArchetype` tests the Disabled
archetype bit at RVA `0x91b2a9d` and rejects the archetype when the matching
query option is clear (`0x91b2aa6`). The integration test checks each
restored Disabled entity against its source archetype and compares the
row count for each of the 49 restored tag types against the autosave.

| Save | Rows with Disabled | Default Simulate query | Simulate query including disabled |
| --- | ---: | ---: | ---: |
| AutoSave_4 | 262,167 | 12,071 | 274,238 |
| AutoSave_5 | 262,167 | 12,069 | 274,236 |

Six other zero-sized identities in the job archetypes are shared components.
Their shared values and chunk grouping are still unmeasured in the Rust
projection. Regenerate the server report with
`python3 tools/probe_type_registry.py --install /path/to/server --server --check --output /tmp/rrising-server-layouts.json`.

The same routine looks up prefab roots by the exact signed GUID
(`0x7f0fa5e`). It maps the root first. When the current and saved linked
groups have the same length, their entries pair by ordinal starting at 1
(`0x7f0fd74` and `0x7f0fd7f`): the first entry denotes the root. A missing
GUID skips the group; a length mismatch keeps the root and skips the
children. Conflicts retain the first value of each direction independently,
and equal duplicate pairs do not change the result. The routine adds
`Entity.Null` in both directions after the prefab loop.

Both original saves contain 23,583 unique prefab GUIDs. Among them, 1,772
have a linked group; every group starts with its own root. After skipping
that entry there are 17,622 child handles. All 41,205 distinct prefab
handles are separate from the persistent job handles.

`SavedSnapshot::bind_entities` takes caller-allocated handles in job row
order and the current prefab groups to produce these mappings. The original
test uses synthetic current handles and catalogs built from the two captured
source topologies: it checks 315,444 and 315,442 pairs in both directions,
including null, against the [derived oracle](oracles/save_bindings.json).
The same test patches copied [reference fields](save_references.md) from original scalar and buffer payloads
with those bindings, including a reference to a prefab root. It does not run
a second ECS or load converted prefab content. Populating the current catalog
from server scenes, obtaining all patch layouts and writing the patched
values into a running world remain separate steps.

```sh
RRISING_SAVE_DIR=/path/to/captured/saves \
  cargo test --locked -p rrising-content --test persistence_bindings -- --ignored
```

## Shared-identity chunk grouping (exploratory, AutoSave_1)

`tests/persistence_shared_grouping.rs` (ignored; needs `RRISING_SAVE_DIR`, optional
`RRISING_SAVE_FILE`) counts job archetypes whose type list contains a shared-component
identity. In AutoSave_1: 136 job archetypes, 109 with a shared identity, and 89 of those
have a chunk-size signature that also occurs in the nonpersistent world-data archetypes.
This is a weak, non-unique match (identical size lists can occur by chance); it does **not**
recover shared values or prove the grouping. The job member itself stores no shared value
payload (chunks are consumed exactly without one), so values must come from elsewhere or be
derived on load; this remains unresolved and unmeasured.

## Where shared values come from (server RVAs, static observation)

The pinned server (`reference.server.lock.json`, GameAssembly SHA-256 `41ba4eb2…0ac7`) was
dumped with Il2CppDumper 6.7.46 (names and RVAs only; Cpp2IL 2022.1.0-pre-21 could not find the
registration structs for metadata 31.1). Selected functions were decompiled with Ghidra 12.2 in
an unanalyzed import (`tools/ghidra/ProbeServerFunctions.java`); decompiler output is kept
outside Git.

Observation: `LoadPersistenceSystemV2.SetDataOnLoadedEntityComponent` (RVA `0x7f14db0`) calls
`UnsafeEntityManagerUtility.CopySharedComponentData` (RVA `0x188f8dc40` VA) with the prefab source
entity and the loaded destination entities; it is reached from `CopyDataFromPrefabEntityToEntity`
(`0x7f13920`) under `CopyDataFromPrefabs` (`0x7f10e40`).

Inference (not yet verified against a save): shared component values of loaded entities are not
read from the job members but copied from the entity's prefab, which matches the job format
carrying no shared payload. Which components are excluded (`ShouldUseSavedComponentData`,
`GetComponentTypes_ExcludeAndSetDataToDefault`) is still unrecovered, so the Rust restore cannot
yet reproduce shared values. Next check: group saved entities by prefab GUID and compare
prefab-derived shared values with the debug-dump chunk grouping.

### Exclusion predicate (server, static observation)

`PersistenceV2Utility.ShouldUseSavedComponentData(saved, current, typeIndex)` (VA `0x1879922f0`)
reads bit `typeIndex & 0x3fff` in two bit arrays and returns true only when it is clear in
**both** the saved-excluded and the current-excluded sets. A type excluded in either set takes
its value from the prefab/default path instead of the save.
`GetComponentTypes_ExcludeAndSetDataToDefault` (`0x18798fdf0`) builds the current set by
enumerating types through reflection and an attribute lookup; the attribute's type name was not
resolved in this pass (no matching field in the server dump), so the member list of that set is
still unknown. The predicate is implemented in `persistence/exclusion.rs` with synthetic tests from this reading only; it is not yet applied in the restore path or compared with an original run.

### Exclusion attribute semantics (server metadata, static)

The server metadata defines `PersistenceV2.ExcludeComponentAttribute(ExcludePersistenceOptions)`
with `CopyDataFromPrefab = 0`, `RemoveComponentFromArchetype = 1`, `LogErrorIfComponentSaved = 2`,
`SetDataToDefault = 3`, and `IncludeComponentAttribute`. Counting `[ExcludeComponent(n)]` lines in
the server dump gives 225 types with option 0 (copy from prefab), 13 with 1, 20 with 2 and 1 with 3;
12 types carry `[IncludeComponent]`. This agrees with the loader path above (excluded components take
their value from the prefab) and identifies the `…ExcludeAndSetDataToDefault` set as the option-3
types. Mapping the attribute to the bit array was not decompiled end to end, so treat it as a
consistent reading rather than a proven equivalence. Not yet implemented in Rust.
