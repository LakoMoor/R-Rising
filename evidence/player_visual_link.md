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

## Face collection and attachment evidence

The candidate gameplay prefab's root has a `ProjectM.HybridEquipmentUser` MonoBehaviour.
Its stripped 404-byte script payload contains a pointer at byte 80 to the
`ProjectM.FeaturesCollection` named `VampireMale_FaceCollection`. A bounded scan of its
244-byte script payload finds a count of 15 followed by 15 records of 16 bytes; each starts
with a resolvable Unity object pointer and has a final four-byte word of zero. All 15 point
to distinct GameObjects; entry 0 is `VampireMale_Face_Standard_Prefab`, entry 14 is
`VampireMale_Face15_Incubus_Prefab`. The last word's meaning is unknown. Metadata identifies
`FeaturesCollection.Entries` and `HybridEquipmentUser.FaceCollection`; it does not establish
that byte 80 is a field solely by offset. The matching named collection and referenced prefab
identities are independent static anchors for this build-specific interpretation.

`rrising-content::player_customization::candidate_male_faces` follows and validates this
chain and indexes the entries; it does not select a face or connect it to the renderer.
On the pinned client, the ignored `player_face_collection_resolves_all_entries` test resolves
15/15 entries. Rest-pose body `Head_JNT` is at approximately (0, 1.680903, -0.042089);
the standard face mesh occupies local bounds (-0.110838, -0.058530, -0.083258) to
(0.110838, 0.164734, 0.122686). The head joint's rotation is not identity. These observations
do not establish the face-to-bone transform. Metadata also contains `HybridEquipmentUser`
attachment methods and a `HybridEquipmentAttachment` type with target-bone and local-offset
fields; neither their execution nor the standard face's binding has been observed. Next:
recover the serialized or runtime attachment and customization-index selection, then compare
against the original before claiming visual or behavioral parity.
