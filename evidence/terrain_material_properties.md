# Terrain-related material inspection

On the pinned client, `VRISING_DATA=... cargo run -p rrising-content --example
terrain_material_inventory` lists properties of indexed Material objects whose names contain
`terrain`. This is a name-based inventory, not a list of the materials used by any particular
terrain scene or a reconstruction of a shader.

For example `Trees_Pine01_Bark_Snow_atlas_TerrainBlend` stores `_BaseColorMap` scale `[1,4]`
and offset `[0,0]`. `UnityMaterialData` exposes these texture environment values, but the scene
viewer currently selects only the texture reference and uses mesh UV directly; it does not
pass this per-material scale/offset to its shader. This establishes a missing material input;
whether and how the specific original shader uses the field requires shader/runtime validation.

Separately, the viewer substitutes layer 0 for Texture2DArray `_BaseMaps` and does not implement
terrain blending. The data needed for layer selection, masks, and original shader logic is not
recovered here, so neither the green appearance nor the correct replacement layer can be
attributed to one cause from this inventory alone.
