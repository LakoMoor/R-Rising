# Forwarded auto-chain time remainder

The original auto-chain update invokes helper RVA 0x170fce0 when the computed target is an
automatic element. `capture_auto_chain_remainder_windows.py` replays the complete helper
body in temporary Windows x64 memory. Read-only constants are relocated and aligned; its
CPU-dispatch flag is explicitly supplied as zero (the non-FMA path). The domain-error helper
is not substituted or executed: all inputs are finite with a finite nonzero divisor. Executable
memory is freed and no original instruction bytes are committed.

154 combinations include positive/negative dividend and divisor, signed zero, subnormals,
small and large finite values. Rust's remainder operator matches all original result bits on
this Windows host. This is sampled coverage; the FMA branch, domain errors and NaN payloads
are not covered. WASM compilation passes; equivalent browser execution of this fixture has
not yet been measured.

The ECS bridge now normalizes forwarded time by the automatic target's duration for the
verified finite/nonzero-divisor domain. Its synthetic test checks an emitted event containing
205 % 100 = 5 seconds, as well as the ordinary target case and an unsupported zero-duration
case which leaves the source clock unchanged. The event producer still has host-owned order
and bindings, no full original job replay, no consumer or live child replacement. Full gameplay
collision and comparative performance remain unmeasured.
