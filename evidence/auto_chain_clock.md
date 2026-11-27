# Automatic spawn-chain clock helpers

Pinned client complete functions: SpawnChainUtility.GetRemainingDuration RVA 0x793d720,
GetTimePassed 0x793d730, CreateAutoChainData 0x793d750. The Windows x64 capture reads the
pinned GameAssembly, copies these leaves into temporary process-local executable memory,
and relocates the disabled-clock constant. No calls, runtime globals or helper stubs are
used. Temporary memory is freed; original instruction bytes are not committed.

`tools/capture_auto_chain_clock_windows.py` records 240 combinations of enable flag, finite
f32 duration, f64 server time and forwarded time, including negative values, signed zero and
large timestamps. Rust matches next-attempt, remaining and passed double bits for every row.
Disabled creation stores -1. Enabled creation adds duration to server time and subtracts only
positive forwarded time. Remaining/passed calculations preserve the original subtraction order.
This is sampled coverage, excluding a claim about all NaN payloads or infinite operands.

The chain reader now exposes auto-transition identifier, transition index, attempt cooldown
and duration. Of 4,297 unique installed elements, 2,996 enable auto-transition. All enabled
entries have finite duration/cooldown and in-range transition indices in the installed data.
The existing pinned-data chain test checks those facts. This is a data invariant, not evidence
that every transition is taken at runtime.

Event matching, importance-based selection, scheduling, cooldown enforcement, active-child
replacement and collision updates remain to be implemented/verified. No original game scenario
or comparative performance is claimed. Reproduce arithmetic with the auto_chain core test;
rerun the ignored original_spawn_chains test for local definition validation.
