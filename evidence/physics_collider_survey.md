# Collider references and map collision survey

Scope: client v1.1.13.0-r99712-b17, GameAssembly SHA-256
`a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56`.
This is an offline data and metadata survey, not a physics or gameplay oracle.
No original blob payloads are included in this repository.

`VRISING_DATA=... RRISING_LAYOUT_REPORT=... cargo run --locked -p rrising-content
--example collider_inventory` reads each local entity scene, resolves its `Unity.Physics.PhysicsCollider`
component references to the scene's own blob batch and reads the common 20-byte header. The
serialized component is eight bytes and the known blob-reference offset is zero. A signed -1
reference is null; non-null offsets must match a blob payload boundary. No serialized offset is
interpreted as a native pointer. Header fields and shape values are named from the pinned client
metadata; a bounded reader checks length, but does not parse shapes or perform collision queries.

On this installation, 199 of 1,183 scenes contain 36,175 such component values; 100 are null.
All 36,075 non-null references resolved; 15,038 distinct blob hashes were observed across scenes.
The referenced root shape bytes, counted per component, were: convex 8,219; sphere 1,188;
capsule 5,296; quad 31; box 8,184; cylinder 156; mesh 10; compound 12,991. Repeated references
are counted repeatedly, not as unique colliders. The diagnostic scene `c6d34432693f24046ae5916650cc70ef`
contains 147 references, all to compound shapes; the terrain source scene
`672ca7a93fee20b46a9c7d87ff39a4c8` contains 17, also all compound. These counts do not establish
that the entities are active, their world transforms, or collision response.

Separately, pinned client metadata identifies `ProjectM.MapCollision` (radius, flags, result)
and `ProjectM.Tiles.TileMapCollisionMath.CheckMovingCircle`, along with tile polygon and height
inputs. This indicates a distinct candidate path for map movement collision; metadata alone does
not establish its runtime scheduling, exact integration into player motion, or how the tile data
is generated. Next work should trace those callers and recover actual tile data and original-side
collision outputs. Unity Physics collider presence is not evidence that a replacement query over
these shapes would reproduce player movement.

Validation: synthetic header boundary/filter test and a pinned-data test resolving all 147
references in the diagnostic scene. The full local inventory also resolved all non-null collider
references. No original gameplay collision scenario or comparative performance has been measured.
