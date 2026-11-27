# Essential spawn-chain attachment

Pinned client function: `AttachEssentialSpawnChainInstanceComponents`, RVA
`0x7ee19d0`, GameAssembly SHA-256
`a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56`.

`tools/capture_chain_attachment_windows.py` executes the original body in memory
and records its command calls. Metadata is ready; component lookups, type-index
conversion, component-set construction and ECB sinks are supplied. The original
432-case oracle has no StaticTransformCompatible on its supplied prefab. The new
512-case oracle exercises static and mega-static attachment. Other external calls trap. Original command-buffer
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
in Rust World. Static prefabs require explicit StaticAttachmentTypes bindings.
The caller supplies
the child's component-set shape; structural addition is not implemented here.
Tests cover config/network combinations, preservation of unwritten clock/network
fields, retention of the old child and prefab, and missing static bindings before writes.
The auto-event/queue/selection test now also attaches its selected candidate and
checks actual parent/child components and clock. These world fixtures are synthetic,
including the supplied live prefab row.

This is immediate Rust ECS application, not original deferred ECB scheduling. It
does not instantiate original prefab content, set TRS, test placement/collision,
remove Disabled, destroy the old child, consume a request or run in either visual
loop. Spawn/destroy barriers and replacement lifecycle remain necessary. No new
visual or performance parity claim is made.

## Static and mega-static continuation

512 additional original-body traces match both the Rust plan and the resulting
immediate ECS writes (with supplied original handles remapped into Rust World).
The prefab's complete 24-byte StaticTransformCompatible payload is copied,
including padding and non-static transform bits. Only byte 0 (UseStaticTransform)
and bytes 2..6 (packed StaticTransformIndex) change: a parent index sets the flag
and is copied; without a parent index the flag and index are zeroed. The prefab's
source flag/index are not authoritative here.

With a parent index, the requested set includes GuaranteedStaticTransform and
Unity.Transforms.Static. If the prefab also has MegaStaticCompatibleTag, it includes
MegaStaticTag before Static in the original constructor's argument order. Without
an index, neither the guaranteed/static nor mega-static tags are requested. The
three added generic ReadOnly<T> identities are resolved from pinned metadata;
type-index conversion, all type-set constructors and structural addition remain
supplied boundaries. Non-static 432-case output still reproduces byte-for-byte.

An installed-data acceptance test uses the registration catalog's root handles
from all nine server prefab scenes. It projects only the relevant source values
and presence flags into a Rust World, prepares a child, supplies a parent index,
and executes attachment. 3,589 registered prefab roots carry StaticTransformCompatible;
93 also have NetworkedSpawnChainChild. Zero of these roots carry MegaStaticCompatibleTag;
that branch currently has synthetic original/ECS coverage only. Source prefab bytes
remain unchanged, while the child's padding/transform bytes and index are checked.
This is not cloning the full original prefab or restoring a real world transition.
The behavioral oracle uses the client body; this data acceptance uses server files.

One Windows opt-level-1 test pass measured 447 ms for catalog reading/projection
and application, including 8,859 microseconds accumulated over prepared-row creation,
attachment and assertions. This is a single Rust-only measurement, not a release
benchmark, game frame time, or original-runtime comparison.

Reproduce (2026-11-28):

```powershell
python tools/capture_chain_attachment_windows.py
python tools/capture_chain_attachment_windows.py --static
cargo test --locked -p rrising-core --no-default-features --test chain_attachment
cargo test --locked -p rrising-core --no-default-features --test static_attachment_ecs
cargo test --locked -p rrising-core --test auto_chain_ecs --test chain_attachment --test transition_selection --test transition_queue --test auto_chain_job
cargo clippy --locked -p rrising-core --lib --test auto_chain_ecs --test chain_attachment -- -D warnings
cargo check --locked -p rrising-web --target wasm32-unknown-unknown
# With VRISING_SERVER_DATA and RRISING_SERVER_LAYOUT_REPORT configured:
cargo test --locked -p rrising-core --test original_static_attachment -- --ignored --nocapture
```

432/432 non-static and 512/512 static original command traces match. Attachment and extended pipeline tests,
Clippy and the shared WASM host check pass. Compilation is not browser execution.
Original executable bytes and decompilation stay local; the public oracle contains
synthetic inputs, command names, requested type names and outputs only.
