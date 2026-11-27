# Automatic job to ECS differential replay

`capture_auto_chain_job_windows.py` replays the pinned client's UpdateJob.Execute at RVA
0x7eb56b0 together with its original recursive progression and numeric remainder helpers.
Its copied range includes the disabled-clock and exception-tail branches; the exercised
finite valid-array path does not take exceptions. Temporary instruction memory is freed;
original code bytes are never included in the repository.

Substitutions: initialized metadata and no-op blob validation; valid-array element/transition
accessors; non-FMA numeric dispatch; command-buffer allocation returns a synthetic handle and
AddComponent records the 96-byte event payload in a local sink. Debug is disabled. Original
ECS allocation/playback, workers, query exclusions, debug logging and error/domain paths are
not emulated. This is an instrumented isolated job oracle, not the original running world.

800 generated cases vary automatic source/target flags, terminal transitions, inactive indices,
deadlines, server time, and initial worker event counts 0/10. The Rust ECS test agrees on final
clock bits, emitted/not-emitted status, event payload and final count in every case. The chain
entity field is compared after mapping the oracle's synthetic input handle to the test world's
input handle; all other payload bytes are compared directly, including the child handle.

`apply_entity_with_count` now accepts the per-job counter and preserves the original ten-event
gate, including initialization/disable handling before that gate. `apply_entity` remains a
single-entity convenience with a fresh counter; batch callers must share the explicit counter.
No live world event consumer, active-child replacement, placement or map registration is yet
connected. Gameplay scenarios, browser execution of this oracle and performance parity remain
unmeasured. Run the auto_chain_job core test to replay the committed observed outputs.
