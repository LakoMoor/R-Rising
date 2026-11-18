# MegaStatic identities and missing references

Both retained CrossOver saves yield the same breakdown of the 471,190
reference occurrences without a target in the projected Rust ECS:

| Field | Current nonpersistent source handle | Stale source version |
| --- | ---: | ---: |
| MegaStatic instance buffer | 235,589 | 0 |
| Spawn chain active child | 235,589 | 6 |
| Spawn chain queued transition | 0 | 6 |

No occurrence has an out-of-range slot or a matching version absent from
the diagnostic listing. The 471,178 nonpersistent occurrences point to
235,589 distinct MegaStatic handles. Each such handle has one instance
entry and one active-child entry. The 12 stale occurrences refer to six
distinct stale handles.

The pinned server invokes `LoadPersistenceSystemV2.ReinstantiateMegaStatics`
(RVA `0x7f06160`) before `RemapEntities` (call sites `0x7f05a7e` and
`0x7f05abe`). The reinstantiation job body (`0x7f195e0`) reads the
instance record as an eight-byte handle followed by a four-byte prefab
GUID; it branches on a zero GUID. The job also compares a
`StaticTransformIndex` against an index derived from the manager and
element ordinal (call `0x7f198fa`). The transform lookup and live entity
creation are not implemented by the Rust plan.

Rust reads a typed plan with the saved manager, original buffer ordinal,
saved instance, GUID, prefab root and placement key. The key is the manager's
16-bit terrain chunk and its 16-bit start index plus the element ordinal,
with the wrap used by the native job. All occupied keys are unique. In both files all 235,589 entries
resolve to known prefab GUIDs (1,182 distinct); 3,101 managers carry the
buffer, with 4,216 vacant slots. Tests cross-check the active-child set
and verify that the manager and prefab targets already exist in the
projected ECS. The projection without terrain still clears those references; the native CLI
now enables the terrain stage described below.

The baked transform lookup is now implemented. In the pinned server,
`StaticTransformIndexLookup.TryFindTransform` at RVA `0x7888d90` uses
the two-byte terrain coordinate to get the chunk's buffer and the signed
16-bit index to select a 28-byte element. Server metadata names the
element fields as position at byte 0 and quaternion at byte 12.

Many terrain locations reuse a baked scene, so the baked row's
`TerrainChunk` value is not a unique runtime key. The Rust catalog joins
the saved coordinate to `TerrainChunkMetadata` (coordinate at byte 0,
scene GUID at 4, rotation degrees at 20), then uses that GUID and buffer
index to read the baked element. As a cross-check, the original CrossOver
log requests scene `672ca7a93fee20b46a9c7d87ff39a4c8` for chunk (2,0),
which is also the metadata lookup result.

The server assets supply 248 coordinates, 189 baked buffers and 227,658
stored elements. Reuse of scene buffers lets the same data resolve all
235,589 occupied placement keys in each save. Their terrain rotations
are 0° for 143,043 entries, 90° for 23,792, 180° for 22,124 and 270°
for 46,630. The native CLI reports these matches under `mega_static_plan`.
The raw catalog returns section-local coordinates. The terrain projection
applies translation by `(160 × x − 3200, 0, 160 × z − 3200)` and the
terrain yaw about the section center `(80, 0, 80)`. Pinned native anchors
are `TerrainChunk.GetWorldTransformMatrix` RVAs `0x78b9080` and
`0x78b9140`; the buffer job `OffsetStaticNetworkedLookupJob.Execute`
(`0x7afe0e0`) left-multiplies the baked rigid transform at `0x7afe417`
and extracts rotation at `0x7afe459`. The Rust implementation follows
that order. Tests independently check the four quarter-turn position
formulas and quaternion equivalence to a yaw composed with the baked
rotation; native float-bit replay has not been performed.

The terrain stage places all 235,589 instances in Rust ECS with prefab
GUID, static index, translation and rotation. It adds their saved handles
before patching components, resolving 471,178 additional occurrences in
each save. Only the 12 stale occurrences still clear. Tests read back all
instance components and the referencing manager buffers. This stage does
not copy full prefab archetypes or execute their behaviors.

Original `MegaStaticInstantiationSystem.InstantiatePrefab` (RVA `0x7f83a80`)
calls `EntityManager.Instantiate` at call site `0x7f83dcc`. The port now
reads baked root rows for the 1,182 GUIDs requested by the placement plan,
builds the projected archetype for each GUID, and copies its supported
scalar values and buffers. Placement-specific transforms and the static
index are written separately.

With the extended server layout report, both retained autosaves copy
4,517,725 additional component values from the prefab roots into the
235,589 placed instances. The test compares every copied byte and keeps
the prior checks of restored instance references. Eligibility excludes
pointer-bearing values, types with entity or saved blob references,
managed/shared/chunk components, tags, cleanup and enable-mask state,
network state, and the prefab registration records themselves. Prefab
children and intra-prefab reference rewriting are still to be ported for
these instantiated copies.

The two captured saves use 1,182 MegaStatic prefab roots, and none has a
linked child. This was checked against the actual scene catalogs for both
captures; other server prefabs can have linked groups. This means the
MegaStatic restore does not need to manufacture child entities for these
saves. Native clone paths are `InstantiateEntitiesOne` at RVA `0x907b8a0`
and `InstantiateEntitiesGroup` at `0x907bc10`.

The root-copy pass now admits layout-validated reference fields when all
targets in the value are null or the source root. Null remains null; a
self-reference is scheduled to become the newly allocated root with its
live version. Fields naming any other entity keep the value out of this
projection. The two original saves exercise the null case and have no
non-null self-reference patches. An isolated test checks self references
in buffer elements and rejection of stale or foreign handles.

The result on each original autosave is 4,753,277 copied component values,
235,552 more than the pointer-free and reference-free pass. All copied
values are compared after ECS installation. The 12 stale references
reported by the save projection are unchanged.
