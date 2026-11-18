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
