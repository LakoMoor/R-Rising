# Spawn position observations

Static inspection of the pinned client `.0.entities` scenes found 50 rows containing
`ProjectM.RespawnPoint` or `ProjectM.SpawnLocationSelector` with Translation/LocalToWorld;
all carried `Unity.Entities.Prefab` (10 had SpawnLocationSelector). They are templates, so
their stored transforms do not establish a live spawn point. The current player clone's
default prefab position likewise has not been validated as a location in the world.
The original server metadata contains `RespawnCharacter` and `RespawnPoint.SpawnExitOffset`;
runtime instantiation, selection and collision-safe placement remain to be reconstructed.
This inventory is a bounded static search, not proof that no live spawn exists elsewhere.
