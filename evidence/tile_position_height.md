# TilePosition height oracle

Pinned client GameAssembly identity is checked against `reference.lock.json` before capture.
`tools/capture_tile_position_height_windows.py` executes the complete x64 getter leaves at
RVA 0x794a1b0 (GetHeight) and 0x794a130 (get_HeightLevel) in isolated temporary memory on
Windows x64. RIP-relative scalar constants are copied and relocated. The functions make no
calls; level branches stay inside the copied function. No game initialization, runtime state,
helper stubs, original payload or copied instruction bytes are included in the committed fixture.

The executable memory allocation is confined to the capture process and freed in a finally
block. This is needed for an original-side oracle and adds no unsafe code to the Rust runtime.

All 65,536 possible ushort compressed heights were supplied at the serialized TilePosition
offset 10. The fixture records the original float result bits and integer level for every input.
Rust reproduces all float bits and levels exactly in the exhaustive test. The implementation
preserves the arithmetic order, including offsets which could otherwise appear cancellable.

This establishes these two isolated functions for the pinned client, not tile registration,
world placement, terrain sampling, gameplay collision or original scheduling. The new functions
are available in the shared core but are not yet wired into a runtime tile map. No comparative
performance or complete game scenario was measured.
