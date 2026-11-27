# Baked TilePosition to world placement oracle

Pinned client hash: checked against `reference.lock.json`. Capture script:
`tools/capture_tile_position_world_windows.py`, Windows x64 and LLVM objdump required.

The original `TilePosition.FromBakedSpaceToWorldSpace` function at RVA 0x794a3b0 is
replayed in temporary process-local memory. Its metadata-initialized byte is supplied as 1,
skipping the original metadata initialization call; this is an explicit runtime substitution.
No original game is started and no instruction bytes or game assets are distributed.
Temporary executable memory is freed after capture, as with the height getter oracle.

The 4,096 cases cover every byte value of chunk rotation, deterministic sampled source
rotation/height, and coordinate/offset values including signed 32-bit extrema. They establish:
quarter turns around the integer origin for rotations 1..3; unchanged coordinates for other
rotation bytes; wrapping 32-bit translation; rotation sum masked to two bits; unchanged
compressed height. Rust matches every recorded output exactly. These are sampled coordinate
cases, not exhaustive coverage of all coordinate pairs.

This implements one original placement operation in the shared core. Callers must still
establish the original chunk offset, pivot handling, instantiated tile models and height layers.
The operation alone does not build a world collision map or resolve player movement. Runtime
collision scenarios and comparative performance remain unmeasured.
