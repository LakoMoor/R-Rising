# Binding saved entities to current handles

The pinned original server allocates new ECS entities when restoring job
rows (`DeserializeLoadFile`: call site `0x7f0c3fa`). It associates each
saved handle with the corresponding newly allocated handle before rewriting
references. `SetupOldToNewEntityRemappingHashMap` (`0x7f0f050`) builds both
mapping directions. Both keys include the handle version.

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
