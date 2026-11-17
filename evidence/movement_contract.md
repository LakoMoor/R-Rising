# Movement steering boundary — pinned build

This is a reconstruction of the shared steering job and waypoint selector, not a
player controller, path search, collision solver or a running gameplay scenario.
All identities refer to `reference.lock.json` (GameAssembly SHA-256 `a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56`).
Raw code and original content are not included.

## Executed original function

`ProjectM.Pathfinding.PathfindingUtility.TryGetTargetPositionFromWaypoints`, RVA
`0x75c6390`, MethodDefinition 223249. Return is Boolean; metadata parameters are
`PathWaypoints (in)`, `float2 position`, `float2 velocityDir`, `float worldSpaceRadius`,
`float2 (out) targetPosition`. `tools/capture_pathfinding.py` runs the original
instructions under Rosetta, with no helper substitutions, and checks output/input
write guards. Only 4,096 input/output rows are retained in `oracles/pathfinding.tsv`.

REA controlled-replay evidence:
`ev_e1fd524db277b9f4bab7dde59a0f7c99c28eacc1f6a5784f5f87b2fcd828197f`.
Process capture identifies Python, not the original executable; the script independently
checks the original image digest before extracting temporary instructions. REA's process-tree,
filesystem-sampling and inherited-environment limitations still apply. This does not launch V Rising.

Observed constants at original data RVAs `0xcf29504`, `0xcf28a88`, `0xcf28704` are
6400.0, 0.5, and the f32 representation of 0.05. Conversion keeps the original
subtraction, multiplication and addition order. With no points the function writes positive
zeros and returns false. It can use only the next point, or advance to it when the current
point's velocity-plane dot is not strictly positive. In particular, unordered (NaN)
comparisons advance too: the original uses `comiss` then `ja`. A literal translation of the
decompiler's `<= 0` failed at row 87 and was corrected from native instruction evidence.
The Rust implementation matches every return and output float bit in the captured set.

## Metadata and caller contract

| Type | Scene stable hash | Size | Relevant managed offsets |
| --- | --- | ---: | --- |
| `PathWaypoints` (unnamespaced, type 15658) | embedded | 20 | Waypoint int2 @0; NextWaypoint int2 @8; HasWaypoint bool @16; HasNextWaypoint bool @17; padding @18 |
| `ProjectM.MoveTowardsPositionBuff` | `809388619ae080d7` | 40 | DistanceSq f32 @0; RecalculationFrequency f32 @4; NextRecalculationTime f64 @8; Waypoints @16 |
| `ProjectM.MapCollision` | `179eefa3019ca077` | 20 | Radius f32 @0 |
| `ProjectM.Velocity` | `8da56f730a0264b0` | 12 | Value float3 @0 |
| `ProjectM.EntityInput` | `cf49a498d7474572` | 96 | Movement float2 @0; AimPosition float3 @8; AimPositionPlane float3 @20; remaining input from @32 |
| `ProjectM.Buff` | `5c0689788602b4e5` | 32 | StartTimeUnmodified f64 @0; Target Entity @8 |
| `Unity.Transforms.Translation` | `a42de7cc0763e5be` | 12 | Value float3 @0 |

The original caller is the generated OriginalLambdaBody at `0x669a880`
(MethodDefinition 33421), with read-only Translation, Buff, MoveTowardsPositionBuff.
Query builder `0x669a3f0` includes those three types. TypeHandle 9958 identifies read-only
lookups for target Translation/MapCollision/Velocity and writable EntityInput. It uses
**Velocity, not MoveVelocity**, and projects target Translation/Velocity onto XZ.
Buff.Target selects the lookups; the buff entity's own Translation is the no-waypoint fallback.
The normalization helper is `0x63354f0`. Movement is gated by DistanceSq versus the squared
XZ distance, while both aim fields point one normalized direction step from the target position.
These identities were established from metadata and instructions. The job body is now also
executed with controlled component access, as described below.

System attributes put the shared update in `ProjectM.SetInputGroup`, worlds `[2,4]`, with
RequireMatchingQueriesForUpdate. Server_Update runs in the same group before Shared_Update;
Server_Create runs in ReactToSpawnGroup, server world `[2]`. Porting the waypoint function does
not establish that the original system ordering or scheduler is implemented.

## Executed shared job, with isolated ECS access

`tools/capture_movement_shared.py` executes the original shared job (`0x669a880`),
waypoint selector and float2 normalization helper (`0x63354f0`) under Rosetta.
The 4,096 rows in `oracles/movement_shared.tsv` retain supplied inputs, normalization
outputs and all 96 bytes of the resulting EntityInput. REA controlled-replay evidence:
`ev_4387628803de500de7b8fbcda60f72e16b1ba3572aa0ce161917b9f9596ad625`.

Five ECS accessors are **supplied-value callbacks**, not original store execution:
Translation (`0x208f70`), MapCollision (`0x2027b0`), Velocity (`0x20a300`),
EntityInput read (`0x1fdbb0`) and write (`0x21e380`). Each callback verifies the
Buff.Target handle (index 77, version 3) and must run exactly once. Metadata is
preinitialized, float2.zero is local static data, and the sqrt wrapper is replaced
by `sqrtss`. Unexpected metadata initialization traps. The harness checks output
write guards, read-only buff bytes and preservation of input bytes 32..96.
This bounds the claim to original job control flow and math; scheduler, metadata
initialization, ECS lookup implementation and missing-target behavior are not native-tested.

`rrising-core::movement` matches every captured float bit, including NaNs, and
the complete 96-byte result. Its World integration repeats all 4,096 cases through
the Rust schedule with the same target handle. Additional tests check Prefab/Disabled
exclusion and the port's explicit missing-target error. The normalizer keeps the
original `y*y + x*x` order, smallest-normal threshold and SSE invalid-result NaN sign.
The stop comparison is strictly greater; an unordered distance does not stop movement.

`scene_view::simulate` runs the system in SetInput before transforms when the initial
World has live query matches. The scene loader now resolves the store's special types
from the registry; previously its unset Prefab slot admitted all 32 templates and hit
null-target errors. The original-scene regression was observed failing, then passing
with the default query correctly excluding them. The dependency-light core also builds
for wasm32 without the default `content` feature. Gameplay is not connected to the
browser viewer, and locomotion/path search/collision remain absent.

## Real content boundary

All 1,183 archetype streams were inspected. Only two scenes contain query candidates:
`0eb9943ba313ad04782f5bf7ae01c25b.0.entities` and
`295c92b7a799a3443b40e9ef192b5727.0.entities`, 16 each. All 32 are Prefab entities;
the default runtime query therefore matches **zero**. Reading the raw resolved component bytes
shows both waypoint flags zero, Buff.Target = Entity.Null and NextRecalculationTime = 0 for all 32.
These validate template storage, not live steering. Runtime buff instantiation, target references,
path results, navigation, collision and command processing remain to be connected and verified.
