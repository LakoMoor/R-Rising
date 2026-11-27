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

Original-settings continuation (2026-11-27): `randomized_settings` reads checked relative arrays
with group stride 16 and chain stride 8 from local settings blobs. The pinned server scenes
contain 110 records, representing 55 identical-per-GUID definitions. The core adapter feeds
those values to the same bounded selection function.

The pinned-data test compares four seeds per definition (0, 1, 99712 and u32::MAX) against
220 isolated original-function observations. Every GUID and final RNG state matches. This
uses the same metadata-ready and valid-array accessor substitutions as the synthetic oracle;
there is still no full runtime spawn capture. The settings reader preserves stored total
weights rather than recomputing them. Bounds and malformed nested arrays have a synthetic test.

To regenerate locally, set VRISING_SERVER_DATA and RRISING_SERVER_LAYOUT_REPORT, set
RRISING_SETTINGS_PROBE to an absolute path under ignored target/, and run the ignored
`original_weighted_settings` test. It writes local-only settings probe inputs. Point
RRISING_WEIGHTED_INPUT at that file and run `capture_weighted_spawn_windows.py`; only
settings GUID, seed, selected GUID and final state enter the committed output fixture.
Do not publish the local probe input containing original settings arrays. Runtime RNG seeding,
replacement lookups, instantiated entities and chain transitions remain pending.
