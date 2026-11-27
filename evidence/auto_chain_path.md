# Automatic path duration to main element

Pinned client TryCalculateDurationUntilMainElement, RVA 0x793d790. Capture script
`capture_auto_chain_path_windows.py` replays the original instruction body in temporary
Windows x64 process memory. Substitutions: metadata-ready state and minimal initialized
metadata, a no-op blob-reference validation helper, a pointer dereference helper, and
valid-array accessors for 96-byte elements / 44-byte transitions. Initialization calls are
skipped. Original validation exceptions, invalid array indices and full runtime are excluded.
No original instruction bytes are distributed; allocated code is freed after capture.

404 synthetic paths cover direct arrival, auto-transition disabled, negative targets,
cycles and chains near the original 20-iteration limit. Rust agrees on success and f64
duration bits for every case. The helper succeeds on main-element arrival before adding
that element's duration; following the twentieth edge does not get another arrival check.
A failed original path writes zero duration. Rust exposes failure as None; callers can
project that to false/zero. This helper follows the configured transition target and does
not inspect end-of-chain flags or choose transitions by event/importance.

The content adapter reads actual auto-transition indices and durations from parsed chains.
Across all 4,297 installed element starting points, the Rust adapter returns 4,251 reachable
paths and 46 failures. These whole-install counts are Rust-side observations; they have not
been separately replayed with original data. Failures are retained, not coerced into main
states. This does not spawn objects or run a game scheduler. Event selection, child replacement,
map registration and collision remain pending; comparative performance is unmeasured.
