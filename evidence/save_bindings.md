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

### Cross-check against a saved header (client AutoSave_0, not a server reference save)

`tests/persistence_exclusion.rs` (ignored; needs `RRISING_SAVE_DIR` and a local
`RRISING_EXCLUDE_JSON` built from the server dump) matches the 2,237 required component names of
one local client autosave against the `[ExcludeComponent]`/`[IncludeComponent]` lists: 210 match
(12 include, 197 option 0 `CopyDataFromPrefab`, 1 option 3 `SetDataToDefault`), and none of the 13
option-1 or 20 option-2 (`LogErrorIfComponentSaved`) types appear. That agrees with the option names
(no saved component may carry the log-error option) but is weak: the header lists required types, not
proof that their data was written, and name extraction from the dump is approximate. The raw server
reference saves under `/tmp` were removed by cleanup, so this pass could not rerun against them;
`tools/probe_crossover_server.py` regenerates them.

### Rerun on regenerated server autosaves (2026-11-25)

The reference run was regenerated with `tools/probe_crossover_server.py` (bottle-setup timeout raised
from 120 s to 900 s because the host was heavily loaded; the first attempt timed out). On two new
server autosaves (AutoSave_2 and AutoSave_3) the exclusion cross-check gives the same result as the
client save: 2,240 required components, 210 matching the lists (197 option 0, 1 option 3, 12
include) and none of the option-1/option-2 types. The shared-identity report gives 136 job
archetypes, of which 109 and 108 carry a shared identity, and 87 and 86 of those have a chunk-size
signature present in world data. These are the same weak, non-unique signals as before; shared values
are still not reproduced or compared. Raw saves remain outside Git.

### Saved column rows by exclusion option (server AutoSave_3)

Counting every saved job column row by the option of its component name: 0 rows for the 197
option-0 (`CopyDataFromPrefab`) types and the 1 option-3 type, 2 rows for `IncludeComponent` types,
and 2,126,024 rows for unlisted types. So the original does not write data columns for components it
excludes, which is consistent with the load path copying them from prefabs. It does not show which
prefab values are copied or reproduce them; the bit-array construction remains a consistent reading,
not a proven equivalence. Name matching is approximate (see above).

### Same check by stable type hash (server AutoSave_3)

Resolving the `[ExcludeComponent]`/`[IncludeComponent]` names to stable type hashes through
`tools/probe_type_registry.py` (203 of 225 option-0 names, 11 of 13 option-1, 9 of 20 option-2, the
single option-3 name and all 12 include names resolve; generics and types absent from shipped scenes
do not) and matching saved job column hashes gives the same result as the name match: no saved
column rows for resolved option-0, 1, 2 or 3 types, and 2 rows (2 distinct types) for include types.
Unresolved names mean some excluded types are not covered by this hash check. Test:
`tests/persistence_exclusion.rs` with `RRISING_EXCLUDE_HASHES_JSON` (local, not distributed).

### Saved archetype index against the original's own debug dump (server AutoSave_2)

The server writes `AutoSave_N.save.gz_DebugDump.txt` beside each save, listing for each of the 136
serialized archetypes its chunk and entity counts and an `Incl(...)`/`Excl(...)` name list.
`tests/persistence_debug_dump.rs` (ignored; needs the save and its dump) matches the Rust job
reader's archetypes one-to-one against that first dump section: all **136 / 136** archetypes agree
on chunk count and entity count, the dump's `Incl` names are all in the saved type list, and every
saved type is in `Incl` or `Excl`. So the saved type list contains excluded component types too,
which agrees with the loader copying their data from prefabs. The match is on names, entity and chunk
counts only: it does not compare component bytes or shared values, and no other save had a dump
(AutoSave_3 has none). One save, one build.

The same dump also lists each archetype's serialized size in bytes. Taking the Rust reader's
archetype start offsets, the distance to the next archetype in the same job equals a dump size with
the same chunk and entity counts for **133 / 133** archetypes (exact bytes, none off by one). The
three last-in-job archetypes have no following offset and were not checked. Still structure only.

Rust-only timing (no original comparison): opening and indexing the 7.6 MB gzip autosave, 274,652
entities, takes a median of 149.5 ms over five release runs (`snapshot_read_timing`), measured on
an Apple M-series host with a load average of about 18-27 from unrelated processes, so the figure is
noisy. The original's own load time is not comparable and was not measured.

### What the dump's `Excl` names are, against the attribute lists (server AutoSave_2)

The dump names 125 distinct components under `Excl(...)` and 282 under `Incl(...)`; none appear in
both. Of the 125 excluded names, 65 are `[ExcludeComponent(0)]` types, 3 option 1, 1 option 3 and 3
`[IncludeComponent]` types; **53 carry no attribute in the server dump** (examples: `Disabled`,
`Attached`, `BlobAssetOwner`, `DisableWhenNoPlayersInRange`). None of the `Incl` names has an exclude
option. So the attribute lists explain only about 56% (69 of 125) of what the original excludes at save
time; another rule (or name-resolution gaps in this extraction) accounts for the rest, and the three
`IncludeComponent` names under `Excl` are unexplained. This corrects the earlier reading that the
attribute alone defines the excluded set: it is necessary-looking for `Incl` (no included name has it)
but not sufficient to reproduce `Excl`. Reproducing the full set needs the remaining rule, not yet
recovered. Test: `dump_excluded_names_versus_exclude_component_lists`.

### Other exclusion inputs found in the server (partial)

`PersistenceV2Utility` holds three static fields: `_CustomHandledTypeData`, `_ExcludedAssemblies`
(`HashSet<string>`) and `_CustomExcludedManagedTypes` (`HashSet<string>`), with
`IsFromExcludedAssembly` (VA `0x187993ba0`: looks up the type's assembly name in `_ExcludedAssemblies`)
and `IsCustomExcluded` (`0x187993c90`). Its static constructor (`0x1879923a0`) references the type
strings `Stunlock.Sequencer.SequenceAssetReferences`, `Stunlock.Sequencer.SequenceDebugCreateData`,
`Stunlock.Sequencer.SequenceAsset` and `Stunlock.Core.SystemUpdateFrequencySystem+Singleton`, the
only string literals it resolves. These are managed types, so they do not account for the 53
unlisted `Excl` names in the debug dump. The contents of `_ExcludedAssemblies` were not recovered
(no literal resolved), and `GetExcludedComponentTypes` (`0x18798f5e0`), `GetComponentsToRemoveFromArchetype`
(`0x187990140`) and `GetComponentTypesThatExcludeArchetypes` (`0x1879907c0`) were not read end to end.
Most of the 53 unexplained names are tags, physics components and Unity types; the likely missing
inputs are the excluded assemblies and the archetype-excluding set, unverified.

### Structure of `GetExcludedComponentTypes` (VA `0x18798f5e0`, static reading)

The function starts from `GetComponentTypesThatExcludeArchetypes` (`0x1879907c0`), then marks types
selected by an enumeration of the type manager's types with a per-type boolean flag (not identified),
then (after a managed-type lookup) adds `GetTypesToLogErrorFor` (`0x187990cc0`). The server metadata has
153 types with `[ComponentExcludesEntity]`; none of the 53 unexplained `Excl` names from the debug dump
is among them, so that set does not close the gap. Which flag the type-manager enumeration tests, and
what puts the 53 names (tags, physics components, Unity types) into `Excl`, remains unrecovered.

### More inputs found, and two hypotheses that failed (server AutoSave_2)

The `PersistenceV2Utility` static constructor registers custom entries through typeof references:
6 custom exclude-component, 29 component-excludes-archetype and 18 exclude-and-log-error calls. Its
resolved types include Unity physics, scene and entity-command-buffer singletons (for example
`PhysicsCollider`, `PhysicsWorldIndex`, `StaticPhysicsWorldBodyIndex`, `PostLoadCommandBuffer`,
`SceneTag`, `Prefab`); 3 of the 53 unexplained `Excl` names (`PhysicsCollider`, `PhysicsWorldIndex`,
`StaticPhysicsWorldBodyIndex`) appear there. Two tested hypotheses did not close the gap: (1) the
unexplained names are zero-sized tags: only 9 of 53 resolve as such, though none of the 282 `Incl`
names is zero-sized; (2) dump `Incl` equals the types with saved data columns: it holds for only 15 of
136 archetypes. About 50 of 125 excluded names (tags, game components) are therefore still unexplained.
