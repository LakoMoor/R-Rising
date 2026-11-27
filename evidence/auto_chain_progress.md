# Automatic chain progression

Pinned client AutoChainTransitionSystem.TryProgressAutoChainTransitionTarget, RVA 0x7eb4cc0.
`capture_auto_chain_progress_windows.py` copies its body, preserving recursive calls, into
process-local executable memory and supplies the same initialized metadata/blob helpers as
the auto-path oracle. Blob validation is a no-op; accessors use valid generated arrays.
Initialization and bounds exceptions are excluded. The copied code is freed, not distributed.

1,040 synthetic valid-graph cases exercise lifetime consumption, skipped first lifetime,
stop-at-main, loop-on-end, invalid initial indices, disabled automatic elements, end markers
and multiple recursive advances. All original success values, source/target indices, 32-byte
event identifiers and forwarded f64 bits match the Rust implementation. Rust uses an iterative
loop, avoiding recursive stack growth. A caller-supplied step budget produces an explicit error
on exhaustion; it is a host guard and is not claimed as original behavior. A synthetic zero-time
cycle test verifies the guard. Other arithmetic fixtures still pass.

This implements progression calculation. It does not choose event transitions by importance,
queue an event, replace active entities, register tile changes or draw changed visuals. Source
settings/chain data are already readable, but live scheduling and ECS integration remain pending.
No whole-game or original-side performance comparison is available.
