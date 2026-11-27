# Spawn-chain definition linkage

Offline reader for the pinned server's SpawnChainConstants blob references. Field offsets
come from pinned metadata: a 12-byte root with MainElementIndex at 8; element stride 96;
transition stride 44. Checked relative arrays are used throughout. The reader exposes
indices, prefab GUIDs, transition identifiers/targets/importance/end flags and the auto-transition
boolean. Element names and auto-transition timing/configuration are not decoded yet.

All 2,990 installed chain records resolve to 1,495 identical-per-GUID definitions, containing
4,297 elements and 15,727 transitions. Every main index is in range; every element's stored
index matches its ordinal; every non-ending transition targets an in-range element.
A pinned-data integration test checks these observations rather than assuming them in the parser.

The 220 prior verified weighted selections (55 settings, four seeds) all identify existing
chain definitions with a main element; all those main prefab GUIDs have a TileData definition.
This is data linkage, not proof that runtime startup always spawns the main element or keeps
it active. World placement, original seed initialization, setting replacements, spawn execution,
transition selection and scheduling still need integration and runtime verification.

Reproduce: set VRISING_SERVER_DATA and RRISING_SERVER_LAYOUT_REPORT, then run
`cargo test --locked -p rrising-core --test original_spawn_chains -- --ignored --nocapture`.
Synthetic malformed nested-array checks are in the content crate's spawn_chain test.
No original blobs, definitions or decompilation are published. Counts and code are public;
original assets remain local. No gameplay collision or performance parity claim is made.

Follow-up: auto-transition identifier, index, cooldown and duration are now decoded. The
pinned-data test checks all 2,996 enabled elements; see auto_chain_clock.md for arithmetic
oracles and the remaining execution boundaries.
