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
