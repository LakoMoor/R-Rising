# Unity terrain object survey

`VRISING_DATA=... cargo run -p rrising-content --example terrain_survey` opens the owner's
client data and all discovered ContentArchives, then scans the serialized object tables by
class ID, independent of type-tree names. On the pinned installation, no TerrainData (156)
or Terrain (218) objects were found. This covers the opened files only; it does not rule out
terrain represented as meshes, entity-scene data, separately stored data or runtime objects.

The current scene viewer draws baked Mesh objects referenced through MaterialMeshInfo; it does
not assemble a world by loading and positioning all runtime terrain chunks. Server-side
`StaticTransformCatalog` already reads terrain chunk coordinates, scene GUIDs and rotation,
and has a placement transform for baked static objects. Mapping those runtime chunk selections
to the corresponding client scenes and rendering their meshes is a concrete next step; neither
that mapping nor the player's live spawn location is established by this survey.

The subsequent `terrain_scene_mapping` example compares each server TerrainChunkMetadata
source GUID with filenames in the pinned client EntityScenes directory using Unity Hash128
parsing. All 248 server terrain coordinates map to a client file; the coordinates reference
189 distinct source GUIDs, all present among 218 client scene GUIDs. For example, server chunk
(2,0), yaw 0, points to client `672ca7a93fee20b46a9c7d87ff39a4c8.0.entities`. A pinned-data
integration test checks all 248 matches. The client's anchor scene independently decodes as
4,095 entities in 78 chunks with MaterialMeshInfo and LocalToWorld values; these are baked
section-local values, not yet placed into the runtime world. Matching GUIDs confirms file
availability, not the full streaming, neighboring-chunk placement, terrain surface, or spawn.

The anchor client scene was rendered locally with 3,786 mesh instances; the
[diagnostic image](../docs/screenshots/experimental-terrain-chunk.png) shows its baked,
section-local contents. It is not placed with neighbors and does not match original lighting,
materials or the original game screenshot. The extracted placement matrix is shared with the
existing static-object restore path, but its application to client rendering remains unverified.
