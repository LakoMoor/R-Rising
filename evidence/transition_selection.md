# Spawn-chain prefab candidate selection

Pinned Windows client `GameAssembly.dll` SHA-256:
`a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56`.
Native anchor: `SpawnChainTransitionSystem.TransitionJob.TryGetPrefabToSpawn`,
RVA `0x7ee4940`. Static instruction inspection supplies the control-flow hypothesis;
`tools/capture_transition_selection_windows.py` then executes the isolated original
selection body, TransitionId equality and finite remainder helper in memory.

Metadata is preinitialized. Blob element access, profiler marker, byte-string
comparison and a successful prefab lookup are supplied. Other external calls
trap; missing-prefab logging and invalid original accesses are excluded.
No original executable bytes, blob data or decompilation are published.

## Measured behavior

400 seeded synthetic cases match Rust: success, signed importance and all 136
output bytes. 232 cases select a candidate; 168 return no candidate. Cases cover
empty/mismatched IDs, signed priority extremes, exclusive last-failed priority,
equal-priority replacement in iteration order, end/loop flags, matrix copying,
optional target auto configuration and forwarded-time reduction when the target
is the main auto element with positive duration. These are synthetic scenarios,
not recordings from the original world. Timing inputs are finite and nonnegative;
malformed strings, invalid indices and missing prefab behavior are not measured.

`transition_selection` is available in the shared core without content readers.
Its `ChainSource` boundary lets the native content adapter expose the original
decoded definitions without copying transition vectors. `select_queued_primary`
reads the actual ECS request buffer, instance and transform components. A test
passes an auto-produced event through enqueue and candidate selection, checking
that the active child and queued request are still present. Placement, secondary
ID retries, active-child replacement, request destruction and scheduling remain
unimplemented by this stage. Neither the native visual loop nor BrowserViewer
currently schedules this chain pipeline.

## Real-content acceptance and timing

The installed server scene definitions contain 1,495 unique chains with 4,297
elements and 15,727 transitions. Running selection for their unique per-element
IDs selected 11,893 candidates, with zero invalid-target IDs skipped. The prefab
lookup is supplied here too; this checks definition execution, not availability,
world placement or original server behavior. The behavioral oracle above uses
the client binary, not the server binary.

One warm-data Windows test-profile pass took **1,821 microseconds**, including ID
deduplication and assertions but excluding scene loading. This is a single
Rust-only observation at opt-level 1, not a release benchmark or original-game
comparison. No performance improvement is claimed.

## Reproduce and checks (2026-11-28)

```powershell
python tools/capture_transition_selection_windows.py
cargo test --locked -p rrising-core --test transition_selection --test auto_chain_ecs --test transition_queue --test auto_chain_job
cargo clippy --locked -p rrising-core --lib --tests -- -D warnings
cargo check --locked -p rrising-core --no-default-features --target wasm32-unknown-unknown
$env:VRISING_SERVER_DATA = '../VRising_Server/VRisingServer_Data'
$env:RRISING_SERVER_LAYOUT_REPORT = 'target/server-layout.json'
cargo test --locked -p rrising-core --test original_spawn_chains -- --ignored --nocapture
```

These checks passed. A full default-feature core WASM check was also attempted;
it fails inside `primase-foundations` 0.3.6 because its Oodle loader unconditionally
references `libloading::Library`. The supported browser core path excludes those
archive readers and passes. WASM compilation is not browser execution evidence.

Follow-up: a non-static essential attachment bridge can now apply selected
candidate fields to a prepared child and parent in the ECS test. This still does
not implement prefab creation, placement, old-child destruction or visual-loop
scheduling. See chain_attachment.md for its 432 original-side command traces.
