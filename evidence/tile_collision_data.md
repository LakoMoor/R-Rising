# Tile collision data survey

Pinned client build and hashes: see `reference.lock.json`. This is an offline format survey,
not a collision behavior measurement. Original payloads remain in the owner's installation.

The pinned metadata identifies `ProjectM.TileData.Data` as a blob reference at offset zero.
`TileBlob.CollisionTiles2D` is at 0x170, followed by `PlacementTiles` at 0x190. Its dense
collision flag array, existence array and integer Min/Max bounds occupy that 32-byte region.
The reader resolves signed relative array offsets and checks every nonempty byte range against
the blob. Shape placement, layer selection, height rules and collision queries are still absent.

Reproduction: with `VRISING_DATA` and `RRISING_LAYOUT_REPORT` set, run
`cargo run --locked -p rrising-content --example tile_collision_inventory`.
On the local pinned client, all 9,268 TileData references resolved, none were null. There were
4,553 distinct blob hashes. Counting each blob once yielded 99,199 dense collision flag entries,
37,507 nonzero. Every visited blob had equal flag and existence array lengths. These counts
include prefab data and do not establish that the tiles are instantiated in the diagnostic world.

The runtime map path must still be traced from the original `CheckMovingCircle` caller through
tile registration, placement, height level and disabled flags. Movement cannot yet use this data
faithfully. No gameplay collision scenario or original-side performance comparison was executed.
