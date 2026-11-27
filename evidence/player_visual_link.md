# Client player hybrid-model references (pinned v1.1.13)

`VRISING_DATA=... RRISING_LAYOUT_REPORT=... cargo run -p rrising-content --example player_visual_trace`
locates prefab GUID 38526109 in client scene `295c92b7a799a3443b40e9ef192b5727.0.entities`.
The three linked members include a root carrying `ProjectM.PlayerCharacter`, `ProjectM.CurrentGender`,
`ProjectM.CustomizationFeatures`, `ProjectM.Hybrid.UseHybridModel`, `HybridModelSeed`, and
`AnimationStateBuffer`. `UseHybridModel` is skipped by the partial clone because it contains a
blob reference. Its serialized first field resolves to a 48-byte blob. The BlobArray header is
relative offset 8, length 2; the 20-byte entries contain these 16-byte AssetGuid values (raw byte
order), followed by model type `1` and padding:

```
60398cf90a047ec4c8ce962b8b8b89ca  1
cd323caa3f6421f4dbcb0827b8aaa4c4  1
```

The exact-build IL2CPP metadata identifies `UseHybridModel.Models` as
`BlobAssetReference<UseHybridModelPrefabsBlob>`, whose entries are AssetGuid and HybridModelType;
type 1 is GameObject. The metadata identifies `HybridModelSystem` as a presentation system with
model streaming and an entity-to-GameObject map; actual selection/animation has not been replayed.
An exact-byte search of original `.entities` files finds the first GUID in streaming subscene
`ddfc6a4db4009e2448b8f8b6f0965b8f.406.entities` and the second in section `.407.entities`, as well
as both in that subscene's `.0.entities`. This is a byte-location observation; the semantic section
binding still requires decoding the section metadata.

`VRISING_DATA=... cargo run -p rrising-content --example visual_assets_trace` resolves candidate
ReferencedUnityObjects for the streaming scene and finds `HYB_VampireMale_Prefab` at object
`ae19a255904fd765632aa97a2d7c670c:3580734981422474692` and `HYB_VampireFemale_Prefab` at
`b6c00f47467ce13af74ddea1cd781444:1755848559842945088`.

The referenced-object catalog records' serialized `RuntimeGlobalObjectId.SceneObjectIdentifier0`
are 406 for male and 407 for female, with the same streaming scene GUID and generation type 6.
On the pinned server image, `SceneHeaderUtility.CreateSceneSectionHash` at RVA `0x998acb0`
sign-extends the section-index argument and stores it in the first 64-bit word of the returned ID;
it stores the scene GUID at +0x10 and generation type 6 at +0x28. Thus these records correspond
to sections 406 and 407 respectively. The focused example verifies both catalog identities and
the single occurrence of the corresponding `UseHybridModel` GUID bytes in each section:

| UseHybridModel GUID (raw bytes) | section | referenced GameObject |
| --- | ---: | --- |
| `60398cf90a047ec4c8ce962b8b8b89ca` | 406 | `HYB_VampireMale_Prefab` |
| `cd323caa3f6421f4dbcb0827b8aaa4c4` | 407 | `HYB_VampireFemale_Prefab` |

The section content has not been semantically decoded to prove the role of its GUID occurrence;
this is a strong static association, not an observed runtime selection. Gender-dependent selection,
runtime entity binding, animator state and rig evaluation remain unverified.

The pinned client contains a candidate `VampireMale_Face_Standard_Prefab` in serialized
file `ae526d3c19a62d5cad7d4b511d1752c3`. Its `LOD00_GRP` contains `Head_GEO` and
`Eyebrows01_GEO` renderers, readable by `Scene::renderers`; these meshes are not skinned.
Their attachment to the gameplay skeleton, offset, customization selection and animation
have not been established. This is an asset inventory, not a reconstructed head binding.

`player_material_inventory` additionally reads the materials actually referenced by the two
candidate gameplay renderers. `Body_GEO` has 2,640 vertices and one submesh, using
`Vampire_Male_Body01`; `Cape_GEO` has 291 vertices and one submesh, using
`CloakUnforgiving_ClothDebugMaterial_MAT`. Both `_BaseColorMap` entries use unit UV scale
and zero offset. The face prefab's static `Head_GEO` references the same body material.
The runtime now binds the two gameplay-prefab base maps; the cloak's explicitly named debug
material does not establish the appearance of an equipped cloak in the original game. The
head attachment, runtime material replacement and shader behavior remain unknown.
