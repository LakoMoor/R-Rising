# Spawn wrapper connected to admitted prefab cloning

Pinned client GameAssembly SHA-256:
`a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56`.
SpawnChainPrefabInstance RVA `0x7ee1740` instantiates a prefab, writes Translation,
Rotation, LocalTransform and LocalToWorld on its root, then delegates to attachment.
Generic metadata resolves the four writes as SetComponent<T>, not AddComponent<T>.
Translation/rotation are explicit inputs; scale is 1 and rotation is not normalized.

`capture_chain_spawn_windows.py` executes the original wrapper, TRS helper
`0x5a7af10` and quaternion-to-matrix helper `0x9338770`. Metadata is ready;
instantiate, ECB writes and attachment are supplied sinks. 256 synthetic finite
cases include identity/unnormalized rotations, varied positions, signed zero,
optional configuration, indices and forwarded time. Rust SpawnPose matches all
transform bytes. Tests also check observed operation order and captured attachment
arguments. Native ECB instantiation/playback is not measured; attachment has its
separate 432/512-case evidence.

## World integration and real content

LinkedPrefabTemplate::instantiate_prepared extends the admitted-value cloner with
extra root slots. Copied values, enable bits, linked buffers and reference remapping
stay on the existing path. Duplicates are canonicalized. Descendants do not receive
extras. Nonportable/managed/network or unresolved external values can still be
omitted under existing reader rules; this is not full Unity Instantiate parity.

chain_spawn::content::spawn_linked requires the four source transform slots,
prepares attachment slots during root allocation, writes the measured pose and
attaches the new instance. It returns group members and the change event. A missing
transform is rejected without changing the parent's active state. A later attachment
failure cleans up returned clone members; earlier allocation failures in the existing
cloner are not made transactional by this wrapper.

Installed-data acceptance selects a real chain/main-prefab definition. Current case:
chain GUID -2144361548, prefab GUID 576352792, one admitted member and 32 root values.
The source and new instance have independent handles. The test verifies four root
transforms at supplied pose (1,2,3; identity rotation), Disabled, chain/child links,
change event and any linked-group buffer. This is one partial initial-spawn case,
not an original world recording or proof of the original starting pose/selection.

The prepared-root regression clones all 23,583 registered prefabs twice (41,205
source group members), adding a synthetic root tag/value to one copy. It checks
1,302,980 admitted values, 50,296 remapped fields (13,218 buffer fields), group
handles and enable bits. This run has zero resolved external fields; a separate
supplied external-binding fixture passes too.

One Windows opt-level-1 spawn observation took 51 microseconds, excluding catalogs
and source projection. The catalog regression took 2.09 seconds. These are single
Rust-only test observations, not release/game benchmarks or an original comparison.

## Remaining work

Full prefab component/blob fidelity, matrix-to-rotation selection, deferred spawn/
destroy order, activation/removal of Disabled, placement, old-child destruction,
request consumption and visual-loop scheduling remain absent. No visible replacement
is claimed. Pose calculation compiles for the browser core; content cloning remains
native because the browser transfer path does not load these templates.

Reproduce (2026-11-28):

```powershell
python tools/capture_chain_spawn_windows.py
cargo test --locked -p rrising-core --no-default-features --test chain_spawn
# Set VRISING_SERVER_DATA and RRISING_SERVER_LAYOUT_REPORT to local pinned data:
cargo test --locked -p rrising-core --test original_chain_spawn -- --ignored --nocapture
cargo test --locked -p rrising-content --test prefab_catalog original_prefab_groups_clone_independent_handles_and_reference_fields -- --ignored --nocapture
cargo check --locked -p rrising-web --target wasm32-unknown-unknown
```

Checks and targeted Clippy pass. Published oracle data is synthetic; original bytes,
prefab payloads and decompilation remain local.
