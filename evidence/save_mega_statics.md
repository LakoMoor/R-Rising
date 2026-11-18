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
projected ECS. The projection still clears missing references until the
instances can be recreated and bound before patching.

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
Positions and quaternions are still section-local. World placement and
instantiation are separate and have not been verified.
