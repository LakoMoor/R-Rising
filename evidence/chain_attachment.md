# Essential non-static spawn-chain attachment

Pinned client function: `AttachEssentialSpawnChainInstanceComponents`, RVA
`0x7ee19d0`, GameAssembly SHA-256
`a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56`.

`tools/capture_chain_attachment_windows.py` executes the original body in memory
and records its command calls. Metadata is ready; component lookups, type-index
conversion, component-set construction and ECB sinks are supplied. StaticTransformCompatible
is absent on the supplied prefab. Other external calls trap. Original command-buffer
allocation/playback and native structural changes are not executed.

432 synthetic scenarios match Rust command payloads and requested command order:

- With an auto configuration, write the parent's clock as duration plus server
  time, subtracting forwarded time only when positive. Without a configuration,
  emit no clock write; do not reset an existing clock.
- Add the requested component set Disabled + SpawnChainChild to the new child.
  Both generic ComponentType.ReadOnly<T> arguments are resolved through pinned
  metadata. The type-set constructor itself is supplied, not replayed.
- Write SpawnChainChild as the chain handle followed by the element index.
- If the supplied prefab has NetworkedSpawnChainChild, write the chain handle
  and four zero network-state bytes to the new child's corresponding component.
- Write the parent's ActiveChildElement as index followed by new child handle,
  then create an ActiveElementsChangedEvent containing the chain handle.

Cases cover optional config/network flags, indices -1/0/8, three durations, three
server times and four forwarded times, including negative forwarded time. They
do not establish malformed-input, nonfinite-time or all-handle behavior.

`chain_attachment::apply_existing` applies measured field writes to a caller-created
child with prepared component slots and Disabled tag, and creates the change event
in Rust World. It rejects static-transform prefabs explicitly. The caller supplies
the child's component-set shape; structural addition is not implemented here.
Tests cover config/network combinations, preservation of unwritten clock/network
fields, retention of the old child and prefab, and static rejection before writes.
The auto-event/queue/selection test now also attaches its selected candidate and
checks actual parent/child components and clock. These world fixtures are synthetic,
including the supplied live prefab row.

This is immediate Rust ECS application, not original deferred ECB scheduling. It
does not instantiate original prefab content, set TRS, test placement/collision,
remove Disabled, destroy the old child, consume a request or run in either visual
loop. Static/mega-static branches, spawn/destroy barriers and replacement lifecycle
remain necessary. No new visual or performance parity claim is made.

Reproduce (2026-11-28):

```powershell
python tools/capture_chain_attachment_windows.py
cargo test --locked -p rrising-core --no-default-features --test chain_attachment
cargo test --locked -p rrising-core --test auto_chain_ecs --test chain_attachment --test transition_selection --test transition_queue --test auto_chain_job
cargo clippy --locked -p rrising-core --lib --test auto_chain_ecs --test chain_attachment -- -D warnings
cargo check --locked -p rrising-web --target wasm32-unknown-unknown
```

432/432 original command traces match. Attachment and extended pipeline tests,
Clippy and the shared WASM host check pass. Compilation is not browser execution.
Original executable bytes and decompilation stay local; the public oracle contains
synthetic inputs, command names, requested type names and outputs only.
