# R-Rising: working notes for the next agent

Goal: port V Rising v1.1.13.0-r99712-b17 to Rust (wgpu, native + browser) with 1:1 behaviour and
original content, measured honestly. **Never commit game files or decompiler output** (`*.dll`, `*.exe`,
`*.entities`, `evidence/decompiled/`, saves and dumps are gitignored; keep it that way). Never claim
parity that was not measured; `README.md` and `docs/COMPATIBILITY.md` carry the honest numbers
(original-game scenarios: **0**).

## What exists
- Replay oracles: original x86-64 bytes extracted from the pinned image, assembled with llvm-mc and
  run under Rosetta (`tools/oracle_replay.py`, `tools/capture_*.py`), compared bit for bit with Rust
  (`crates/rrising-core/tests/*`, `evidence/oracles/*.tsv`). 119 addresses so far.
  `python3 tools/oracle_inventory.py` recounts. Replay catches op-order bugs (see
  `evidence/original_leaf_oracles.md`).
- Save format: `crates/rrising-content/src/persistence*` decodes PersistenceV2 saves; structural
  parity 136/136 archetypes (`evidence/save_bindings.md`). `restore_world.rs` projects a save into the
  ECS store.
- Original runtime: `tools/probe_crossover_server.py` runs the pinned Windows server in a disposable
  CrossOver bottle (macOS only). `--seed-save AutoSave_N.save.gz` makes the original load a save.
  Result and numbers: `evidence/crossover_runtime.md`.

## What you need locally (not in Git)
Legally owned V Rising client and dedicated server installs matching `reference.lock.json` and
`reference.server.lock.json` (SHA-256 pinned), CrossOver for the server probe, Il2CppDumper/Ghidra only
if you want to re-derive layouts (output stays outside Git).

## Next steps (in order)
1. First scenario that runs on the original (CrossOver server) and on Rust from the same save, then
   compare state and time. Needs typed world restore beyond `restore_world.rs` and a per-tick state
   dump on the original side (`StorePersistenceDebugData` gives debug dumps per autosave).
2. Remaining exclusion-rule gap (about 50 of 125 excluded names unexplained, `evidence/save_bindings.md`).
3. More replay oracles for pure helpers; sine/expo/circ/elastic easings need the IL2CPP math wrappers
   and `cos` bit-exactness check first.
4. Original performance numbers under the same host conditions (CrossOver adds Wine/Rosetta overhead;
   say so when comparing).

Rules: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test` before commit; commit
messages end with the Co-Authored-By trailer used in the log.
