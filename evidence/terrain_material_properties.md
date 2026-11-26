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

The focused `terrain_shader_inventory` example reads compiled-variant metadata for the shader
referenced by `ElrisSnowWhispy01_Terrain`. On the pinned installation the shader is named
`Shader Graphs/Shader_Terrain`, with platform 4 (D3D11); among the inspected vertex/fragment
variant parameter fields is `_WorldUVScale`. The same material stores that value as 0.075.
`_BaseMaps` did not appear among the texture parameters returned by this inventory; that is
not evidence it is unused. Variant parameter blobs or other stages may need additional decoding.
These are metadata observations, not shader instruction analysis or proof of the formula.

The inventory now requests merged variant-specific parameter blobs instead of only common
parameters. On the pinned installation it visits 252 vertex/fragment variants: 192 decode
fully, while 60 use incomplete common-parameter fallback because the current unity-formats
parser rejects an entry of kind 2. In fallback metadata `_WorldUVScale` is a scalar at offset 0
in `UnityPerMaterial`, with no resolved binding slot. `_BaseMaps` is still not returned, but
the 60 incomplete variants prevent a negative conclusion. The shader instructions and UV
formula have not been decoded; changing the renderer from this metadata would be speculative.
