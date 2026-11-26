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

The static native viewer now accepts `--terrain-chunk x,z --server-data <server data>
--server-layouts <server report>` with a matching `--scene`. It checks the scene GUID and applies
the recovered placement matrix to instance transforms and camera bounds. Chunk (2,0) rendered
successfully using the pinned client/server files. This establishes the implementation path,
not its agreement with the running original, neighboring chunks, or live streaming.

The static viewer also accepts repeatable `--terrain-neighbor x,z` options alongside
`--terrain-chunk`. An offscreen run placed entries (2,0) and (3,0) together; their
[diagnostic screenshot](../docs/screenshots/experimental-two-chunks.png) shows two identical
sections because both entries use the same source GUID. This is a placement experiment, not
proof of the original world's runtime composition or streaming. Scene statistics principally
describe the primary section; merged geometry is not a complete world inventory.

A second offscreen run used distinct sources: (10,0), yaw 180°, GUID
`3b35dfc3f24e9bb45b6da67f70a87fbd`, and (10,1), yaw 270°, GUID
`a32a1d347a4b31d4c90744a4ede6acc5`. It drew 479 groups in the
[diagnostic image](../docs/screenshots/experimental-distinct-chunks.png). The image does not
establish correct seams, runtime loading, terrain shading or placement parity with the game.
