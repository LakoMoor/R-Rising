# Baked subscene spawn placement survey

Pinned client and server identities are checked by their layout readers. The server report was
regenerated locally with `probe_type_registry.py --server --check`; all 1,390 distinct scene
hashes matched. This is offline evidence, not original runtime spawn behavior.

Client inspection (`mega_tile_inventory`) found two MegaStaticPrefabBuffer components with
zero entries, so those buffers do not provide the placed prefab identities in the visual scene.
Server inspection (`subscene_tile_inventory`) reads SubSceneNetworkedPrefab and
SubSceneSpawnChainPrefab plus StaticTransformIndex. It found 1,528 direct requests and
225,983 chain requests. All 227,511 rows have static keys with nonnegative signed transform
indices. The key names the baked section; reused scenes require runtime chunk remapping.

Joining direct request GUIDs to the client tile definition catalog finds 1,514 matches.
The other 14 are not assumed to be tile obstacles. Chain GUIDs identify chains and cannot
be treated as the selected final prefab. Selection, conditions, random state, transitions,
disabled tile flags and live state remain unimplemented in this path.

The reader exposes typed requests, kind and optional placement key. A pinned-server test
checks the full counts and placement-key presence. Nothing in this inventory instantiates
entities or proves that any requested tile is active. Next work must resolve chain definitions
and original selection behavior, and apply verified chunk placement before collision assembly.
No game collision scenario, visual change or comparative performance is claimed.
