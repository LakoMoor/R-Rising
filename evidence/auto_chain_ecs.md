# Bounded automatic event ECS bridge

`auto_chain_ecs::apply_entity` connects the verified progression helper to selected ECS
component columns and creates a 96-byte transition event entity for supported paths.
This is an experimental bridge; bindings/order and total event limits are caller-owned.
It does not implement the original job scheduler or command-buffer playback.

Static source anchors: pinned client AutoChainTransitionSystem.UpdateJob.Execute RVA 0x7eb56b0.
Observed gates include inactive index -1, disabled deadline -1, disabled auto config, initial
zero deadline, and strict server-time > deadline. The original caps events per worker/job at 10;
the single-entity bridge leaves that cap to the caller. After an attempt it uses current server
time plus the current element's attempt cooldown. Event fields are identified from metadata
and observed writes at 0x7eb5d02..0x7eb5d5b.

The Rust test actually creates the event in ECS, verifies all payload fields, checks retry
clock update and the not-due gate. It also verifies that a target needing the unimplemented
numeric time adjustment returns UnsupportedNormalization without changing the clock. This is
synthetic ECS validation, not an original job replay. Original event order, debug behavior,
full query exclusions, target-time adjustment and failure/exception paths remain unverified.

Event consumers, active child replacement, original world scheduling and tile-map updates are
not connected. No visual change, complete gameplay scenario or performance parity is claimed.
