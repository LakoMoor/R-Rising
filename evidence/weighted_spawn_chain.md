# Weighted randomized spawn-chain selection

Pinned client `RandomizedSpawnChainSettingsBlobAsset.GetRandomWeightedSpawnChain` at RVA
0x79381a0 is replayed by `tools/capture_weighted_spawn_windows.py` on Windows x64. The capture
checks the GameAssembly hash from reference.lock.json, copies original instructions only into
temporary process-local memory, and frees that memory after execution. Original bytes are not
committed. The fixture contains generated synthetic inputs and observed outputs.

Explicit substitutions: metadata initialization is marked complete; two generic blob-array
accessors are replaced with bounded-input relative-offset accessors of strides 16 and 8. Their
runtime generic metadata, bounds exceptions and failure paths are not emulated. The 1,024
cases use valid generated arrays, finite nonnegative weights and matching total weights.
They cover empty roots, empty selected groups, zero weights, several group/chain counts and
multiple RNG seeds including zero. They are not exhaustive and do not use original settings.

Rust matches the selected GUID and final 32-bit RNG state for every case. Selection uses the
old state to generate a float, advances xorshift32, accumulates weights in order and accepts
the first cumulative value >= the target. An empty root advances no RNG; an empty selected
group consumes only the group draw. The shared core implementation rejects nonfinite weights
and reports an error for out-of-array selection. Those errors are not original exception parity.

The helper is not yet wired to original settings blobs, runtime replacement lookups, seed
initialization, spawning or chain transitions. No game scenario or comparative performance
is measured. It is a verified bounded selection operation for the pending spawn pipeline.
