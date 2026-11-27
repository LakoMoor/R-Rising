# Tile definitions and placement binding survey

Pinned client; offline evidence, no original game runtime capture. Reproduce with
`VRISING_DATA=... RRISING_LAYOUT_REPORT=... cargo run --locked -p rrising-content
--example tile_binding_inventory`.

Across the installed scenes, all 9,268 TileData rows have PrefabGUID values and map to
4,553 distinct GUIDs. No GUID names conflicting blob payloads. All 9,268 TilePosition rows
are in the same definition scenes and match a TileData GUID; none are in the diagnostic
visual scene `c6d34432693f24046ae5916650cc70ef`. This prevents treating the visual scene's
render instances as an already populated logical tile map. The Prefab tag itself is absent
on these serialized definition rows; their later runtime registration/instantiation has not
been replayed here.

`TileTemplateCatalog` reads and validates the local definitions, indexes them by original
GUID and exposes their dense collision tiles. It rejects conflicting definitions, invalid
references and missing GUIDs. A failed scene read commits no partial definitions. Blob bytes
remain process-local; there is no serialization or publication of these assets. The pinned
installation test constructs all 4,553 definitions and passes, including duplicate references.

Next integration needs the original runtime source of instantiated tile models (including
mega statics), their GUIDs, placement and enabled layers. No collision response, playable
scenario or comparative performance has been verified by this survey.
