# Transition event queue bridge

Pinned client event equality at RVA 0x793c970 and TransitionId equality at 0x793d030 are
replayed by capture_chain_event_key_windows.py. Metadata is supplied ready; the valid-string
byte comparator is a supplied callback. 123 synthetic cases match Rust equality, including
ignored FixedString padding and forwarded time. Malformed string lengths are rejected in Rust;
original exception behavior and non-ASCII comparison routines are not measured.

Static enqueue anchor: EnqueueTransitionEvents_Job.OriginalLambdaBody RVA 0x7ee4fb0. It checks
that the chain's current active index and complete child handle match the event. In the buffer
loop it calls event equality, replaces the first equal entry, otherwise appends. A Disabled
chain retains its queued request but is not added to the ready-to-process list. These queue
observations have not yet been validated by a complete native enqueue-job replay.

The Rust bridge reads an explicit event entity, checks the live chain handle and active child,
updates QueuedTransitionRequests and reports whether the chain is ready. Caller owns source
event destruction, ordering, scheduling and ready-list handling. Synthetic ECS tests check
append, replacement by a newer forwarded time, disabled chains, stale chain versions and stale
child versions. An integration test passes an actually produced auto event through this queue
into the chain's buffer and compares the payload. No active child is replaced by this stage.

Clippy and native tests pass; WASM compilation was checked in the preceding work session.
Browser execution, full original queue parity, transition priority selection, prefab replacement
and tile-map updates remain pending.

Follow-up: bounded priority/candidate selection now has 400 original-side oracle
cases and an ECS queue-to-selection integration test. See transition_selection.md.
Full queue parity, actual replacement, placement and scheduling are still pending.
