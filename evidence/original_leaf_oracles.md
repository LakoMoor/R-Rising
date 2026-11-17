# Original native leaf-function oracle

Reference: `reference.lock.json`, GameAssembly.dll SHA-256
`a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56`.

## Executed evidence

`tools/capture_type_flags.py` extracts exactly ten bytes each at RVA `0x8fcda80`
(`TypeIndex.HasEntityReferences`) and `0x8fcda90` (`IsChunkSerializable`) from the
pinned binary. It links those bytes as two x86-64 leaf functions into a temporary
Mach-O executable, calls them using the Windows x64 `ms_abi`, and executes via
Rosetta. The temporary executable and assembly are removed. Only 1,024 scalar
input/output records are retained in `tests/golden/type_flags.tsv`.

REA controlled-process evidence:
`ev_00f5c5e4af9046b88f8edd949a18887dd1fa436e3781d20e915002e15ce540be`,
exit 0, output confirms 1,024 captured inputs. Reproduce:

```sh
python3 tools/capture_type_flags.py
cargo test --locked -p rrising-ecs --test type_flags
```

The Rust test matches all 1,024 original outputs for both properties. Both
getters return true when their respective flag is **clear**. The earlier port
used positive polarity. `BuildComponentType` at RVA `0x905a560` corroborates
that the bits represent absence of references and invalid serialization.

## Static corroboration

Targeted LLVM 23.1.4 disassembly of the original bytes shows shift-right by 17
or 18, inversion of AL, then `AND AL,1`. `ArchetypeChunk.get_SequenceNumber`
at RVA `0x8ed6720` loads a qword at chunk+`0x28`. Thus the old header read from
bytes 36..44 was wrong; the field occupies 40..48. Header-length tests also
cover every truncated length 0..47 and require short writes to leave memory intact.

## Limits and unavailable checks

These are two pure functions, not a running Unity game, game-loop trace, or full
ECS parity. Rosetta and the compiler are part of this oracle environment.
The full original `VRising.exe` was separately attempted with bounded headless
arguments and a scratch save/log path: REA evidence
`ev_1bb3fa38dc123a08dd54c4b4d4e7c9d3df576c062a71cca1540d71c1bdd47f0a`,
exit 126, `cannot execute binary file` on this macOS arm64 host. The original
Windows engine is not running here. Fresh whole-binary Ghidra auto-analysis via
REA timed out after 300 seconds; saved decompilation and focused LLVM reads were
used instead of repeatedly restarting full analysis. Static decompilation is
not recorded as execution evidence.

## MaterialMeshInfo leaf oracle

`tools/capture_material_mesh.py` executes five additional original functions:
ArrayIndexToStaticIndex (RVA 0x8f46ef0), StaticIndexToArrayIndex (0x8f46f00),
get_SubMesh (0x8f46e70), get_HasMaterialMeshIndexRange (0x8f46ed0), and
set_SubMesh (0x8f46e80). The same temporary Mach-O / Windows-ms_abi / Rosetta
harness captures 1,024 inputs including signed minima, maxima, negative indices
and random packed words. REA controlled-process evidence:
`ev_e55d039db7d52de5a325d4a8ce71a07ecf04114fe78e319ebf7c8abc4548b5a5`, exit 0.

The Rust comparison passes all cases. Important retained behavior: the getter
reads only eight submesh bits, while the ushort setter replaces the full packed
word; negative static indices pass through encoding; INT_MIN decoding wraps to
INT_MAX. Range construction comes from static inspection of SubMeshIndexInfo32
at RVA 0x8f577b0 (16-bit start, 7-bit length, high-bit range marker). It has not
been replayed as an original function. Invalid lengths produce Rust errors,
rather than reproducing Unity assertion infrastructure.

The component is decoded by the read-only scene inspector when available.
Current admitted archetypes contain **zero** MaterialMeshInfo records: their
rendering archetypes still include unresolved types/shared objects. No real
mesh/material binding or original world rendering is claimed from these leaf tests.

## Pure Unity.Mathematics / Unity.Entities leaf oracles (2,048 cases each)

`tools/oracle_replay.py` extracts each function's bytes from the pinned GameAssembly.dll, rejects anything
containing calls or jumps leaving the function, patches RIP-relative constant reads into a temporary
Mach-O, and runs it under Rosetta with the Windows x64 ABI. Only input/output rows are kept in
`evidence/oracles/*.tsv`; no original bytes or decompiled text.

| Script | Golden | Functions | Rust test |
|---|---|---|---|
| `capture_math_int.py` | `math_int.tsv` | lzcnt, reversebits, ceilpow2 (int, uint), ceillog2, floorlog2, ispow2, abs, min, max, clamp (int) | `rrising-math/tests/oracle_math.rs` |
| `capture_math_float.py` | `math_float.tsv` | f32tof16, f16tof32, radians, degrees, unlerp, remap, step, sign, clamp, min, max, smoothstep (incl. NaN/Inf/denormal inputs) | same |
| `capture_entities.py` | `typeindex_flags.tsv` | 19 TypeIndex predicates plus Flags and Index | `rrising-ecs/tests/oracle_entities.rs` |
| | `hash128.tsv` | Hash128 op_LessThan, op_GreaterThan, CompareTo, GetHashCode, op_Inequality | same |
| | `archetype_getters.tsv` | 13 Archetype flag predicates and 11 section-count getters | same |
| | `entity_compare.tsv` | Entity.CompareTo, ChunkIndex lt/gt, ComponentTypeInArchetype >= | same |
| `capture_math_matrix.py` | `math_matrix.tsv` | quaternion-to-matrix (3x3, 4x4), TRS, Translate, Scale, matrix mul, quaternion mul, quaternion normalize | `rrising-math/tests/oracle_matrix.rs` |
| `capture_change_versions.py` | `change_versions.tsv` | DidChange, IncrementGlobalSystemVersion | `rrising-ecs/tests/change_versions.rs` |

Initial scalar/ECS group: 77 functions x 2,048 cases (157,696 function evaluations; two TypeIndex getters overlap the earlier
type_flags oracle). Divergences found and fixed in Rust: `math.clamp(int)` is `max(a, min(b, x))` (differs for
a > b), `math.abs(int.MinValue)` stays MinValue (Rust overflowed), and `Archetype.HasChunkComponents` is
`FirstChunkComponent != TypesCount`. Not covered: functions that call out (Hash128.get_IsValid reads statics,
sinh/frac call helpers) and the archetype layout builder, which is not a leaf.

The eight matrix/quaternion bodies add 16,384 evaluations. Captured calls between the bodies are
linked in the temporary executable; quaternion normalization's lazy IL2CPP sqrt wrapper is replaced
by `sqrtss`, so that wrapper's initialization and exceptional-value semantics are not covered.
See `capture_math_matrix.py` and `docs/COMPATIBILITY.md` for the observed operation-order fixes.

## Change-version filtering — 2026-11-17

Original `ChangeVersionUtility.DidChange` at RVA `0x9072720` and
`IncrementGlobalSystemVersion` at `0x9072730` each replay 2,048 inputs under Rosetta, with no helper
stubs. The boundary grid includes zero, adjacent values, the signed half-range, and u32 wrap.
Only inputs/outputs are retained. `DidChange` forces true when the last system version is zero;
otherwise it uses a positive signed wrapping difference. Increment skips zero after wrapping.

The prior `FrameContext::did_change` used unsigned `>` and disagreed with 315 / 2,048 original
outputs, including `(0, 0)` and `(1, 0xffffffff)`. The regression was observed failing before
the fix. `change_version.rs` now implements the two functions; `FrameContext` and the store call
them. Both oracle tests pass, plus a transform-hierarchy integration test checks a post-wrap input
version of 1 against a pre-wrap last-system version of `0xffffffff`.

Across all recorded groups: 93 distinct native function addresses (7 earlier captures, 87 captures
of 2,048 cases and one waypoint capture of 4,096 cases, with 2 overlapping TypeIndex getters). This is function evidence, not gameplay parity.
`python3 tools/oracle_inventory.py` regenerates
[native-oracles.json](../docs/verification/native-oracles.json) directly from the pinned golden headers:
189,440 function evaluations including repeated captures of the same function. CI checks the report.
This corrects the earlier scalar/ECS subtotal of 75: the actual captured list has 77 addresses.


## First movement-path function

`capture_pathfinding.py` adds the original waypoint selector (RVA 0x75c6390),
4,096 cases with no helper stubs. Returns and float bits match `rrising-core/tests/pathfinding.rs`.
See [movement_contract.md](movement_contract.md) for controlled-replay evidence, exact metadata,
NaN-branch correction and the limit of the baked-prefab corpus. This is not live movement.
