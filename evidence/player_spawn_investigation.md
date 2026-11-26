# Player spawn location investigation

The pinned client layout report identifies `ProjectM.RespawnPoint` (stable hash
`a7b81042664bca03`) and `ProjectM.SpawnLocationSelector` (`f82e9371115bb9dc`).
The server metadata describes `RespawnCharacter` methods receiving a spawn-location index,
and `RespawnPoint` contains a `SpawnExitOffset`. These observations do not identify the
runtime spawn or its world coordinates.

A local scan on the pinned client of scene files ending in `.0.entities` found 50 rows
with one of those components and a Translation or LocalToWorld; all 50 were tagged
`Unity.Entities.Prefab`, including ten with SpawnLocationSelector. Their serialized
positions are prefab defaults, not evidence of a live spawn position. This search does
not cover other scene sections, unloaded assets, or server-generated and runtime state;
it cannot rule out live spawn markers elsewhere. The diagnostic player currently uses
the prefab's default position and therefore can be inside scenery.

The same scanner was then extended to every `*.entities` file in the pinned client
installation: all 1,183 files opened and resolved, with zero unresolved chunks. It again
found 50 matching rows with Translation or LocalToWorld, all marked Prefab, and all in
section 0; the 965 nonzero sections yielded no additional matching rows. This remains
limited to these component hashes and rows with a transform; it does not cover live
runtime entities, saved worlds or other representations of spawn positions. The local
exploratory scanner is `/private/tmp/rr-spawnscan/src/main.rs`, and its output is
`/private/tmp/rr-spawnscan/all-sections-output.txt`; neither contains game files.
