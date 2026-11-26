# Player input to movement: static observations

Build examined: V Rising dedicated server `GameAssembly.dll`, SHA-256
`41ba4eb27c53dd9dc1a3d3337a2c8550b8386421361792ee6ac159ef00ff0ac7`.
All RVAs below belong to that server image, including the client-designated
systems compiled into it. They must not be used as offsets into the client.
The observations are from the IL2CPP metadata and disassembly, not a live game.

`MoveEntitySystem.OnUpdate` (RVA `0x7d74270`) queries `Translation`,
`Velocity`, `MoveVelocity`, `Movement`, `Rotation`, `EntityInput` and
`NetworkInterpolated_Shared`. At `0x7d74ed7` it copies eight bytes from
`Movement+0x4c`; at `0x7d750ed` it passes their address to
`CommandUtility.DoMovement` (call `0x7d750fd`). Its speed argument comes from
`Movement+0x5c`, and the ability multiplier from `Movement+0x60`. The metadata
names those fields `MoveInput`, `Speed`, and `AbilityCastSpeedMultiplier`.
`Movement` is read-only in this particular query: writing `EntityInput` does
not establish that `Movement.MoveInput` changes.

Another direct call to `DoMovement` is in `CommandUtility.DoCommand` (RVA
`0x83a8e00`, call `0x83a97a1`). `CommandInput` contains an
`InputCommandBufferElement` at `+0x10`, and that buffer element begins with
`CommandMovementInput.MoveDirection`; its separately stored `RawInput` starts
at `+0x14`. `DoCommand` loads the first 16 command bytes at `0x83a918d` and
passes the address of the copied movement input at `0x83a9725`.
The separate raw input and command direction fields cannot be conflated.

One candidate writer was excluded by inspecting the pinned server image:
`LegDirectionSystem_Update.OnUpdate` (RVA `0x6392f70`) queries writable Movement and
LegDirection and read-only EntityInput and Rotation. Its stores near `0x63935b7`
target the LegDirection tuple item, while the Movement pointer is used near
`0x6393768` to access Speed at offset `0x5c`. These observations do not identify a
write to Movement.MoveInput and therefore do not establish the missing input bridge.
This is a bounded static inspection, not proof that the system never modifies it
through another call or execution path.

Direct relative calls to `DoCommand` in the server image occur in
`PlayCommandsSystem_Server.OnUpdate` at RVA `0x839f598` and
`PlayCommandsSystem_Client.OnUpdate` at `0x83a622e`. The latter has metadata
`UpdateAfter(CreateInputCommandsSystem)` in `PlayCommandSystemGroup` and
queries `InputCommandDataProxy`, `InputCommandBufferElement` and `Controller`.
`Controller.Controlled` and `InputCommandDataProxy.CommandDataEntity` are
networked references; the buffer contains direction, raw input, frame numbers,
end time and delta time. This static path identifies a subsequent command
consumer, not a running client trace. Its lookup, buffering, replay, collision,
and full call prerequisites remain to be reconstructed. Server-image RVAs are
not client-image addresses.

The same image exposes `CommandUtility.CalculateDirectionAndRotationSpeed` at
RVA `0x83aa8a0`, a candidate for the missing player-facing rotation path. Metadata
declares its inputs as `Movement`, `TargetDirection`, `EntityInput`, `Translation`,
`Rotation`, player flag and optional aim parameters, with direction and rotation
speed outputs. The instruction branch `0x83aa930..0x83aa9ba` checks the squared
length of the first two `EntityInput` floats, and if positive and
`TargetDirection.RotateTowardsMouse` (byte `+0x2c`) is zero, normalizes those
input components as an XZ direction. Subsequent instructions also calculate
rotation speed, and other branches use aim or forced direction. This is static
analysis on the server image, not a complete implementation or runtime trace;
normalization edge cases and all remaining branches still need original replay.

`tools/capture_move_facing.py` replays the ordinary nonzero movement-input slice
(server RVAs `0x83aa956..0x83aa959`, `0x83aa95d..0x83aa989`, and
`0x83aa997..0x83aa9ba`) in isolation under Rosetta. It supplies the 1.0 constant,
substitutes `sqrtss` for the original math wrapper and assumes that the earlier
positive-length and mouse-facing predicates passed. The Rust `movement_facing`
matches 80 finite input pairs bit-for-bit. The function explicitly refuses zero,
non-finite and overflowed input, and mouse-facing mode; it does not implement the
other branches, the actual rotation, or rotation speed. This is not gameplay parity.

The compiled `CreateInputCommandsSystem.OnUpdate` (RVA `0x7e4e050`) queries a
command buffer, `EntityInput`, and `Controller`. It copies all 96 bytes of the
input into the `RawInput` portion of a temporary command at `0x7e4e71b` through
`0x7e4e75a`. At `0x7e4e804`, one conditional path copies the first eight bytes
of `EntityInput` to the command movement input. Generic method metadata at
`0xdb30190` identifies that first lookup as `TryGetComponentData<Movement>`;
the output is at `rsp+0x230`, so `rsp+0x27c` at `0x7e4e862` is
`Movement.MoveInput` (`+0x4c`). When Movement exists, another lookup identified
by `0xdaf5488` reads `IsSpellControlled`. If either of its first two bytes
(`MovementControlled`, `RotationControlled`) is nonzero, that path writes zero
at `0x7e4e853`. The chosen direction is stored at `0x7e4e90f`. An earlier
branch at `0x7e4e934` also clears it when the controlled entity fails
preliminary checks; those checks are not fully identified.

`tools/capture_command_move_selection.py` replays the original instruction
slices at `0x7e4e804` (4 bytes) and `0x7e4e83b..0x7e4e86b` (48 bytes), with
synthetic stack values and lookup results. The Rust selection helper matches
the recorded 36 cases bit-for-bit, including signed zero, NaN payload bits,
and infinity. The test covers copying/zero selection only: it does not execute
the actual ECS lookups, preliminary entity checks, command construction, or
the game loop. The decision that Movement is absent or present is supplied by
the harness rather than replayed.

The existing isolated `DoMovement` replay covers a bounded arithmetic branch,
not the surrounding command replay, scheduling, collision, flags, ability
handling, or network reconciliation. A faithful player driver needs the actual
command-construction conditions and the `DoCommand` prerequisites, plus a
comparison against the original end-to-end. The current partial player prefab
and scene driver do not supply that evidence.
