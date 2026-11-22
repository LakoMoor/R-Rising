# Original dedicated server through CrossOver

The original Windows x64 **dedicated server** now runs on this Apple M4 Mac.
It reaches `Startup Completed`, runs the world, and writes autosaves with its
built-in persistence debug data enabled. This removes the server-launch blocker;
it is not an original-client render capture or a deterministic gameplay test.

## Identity and isolation

The additional target is pinned separately in [reference.server.lock.json](../reference.server.lock.json):
`VRisingServer: v1.1.13.0-r99712-b17 (202605251709)`, Unity 2022.3.58f1,
GameAssembly SHA-256 `41ba4eb27c53dd9dc1a3d3337a2c8550b8386421361792ee6ac159ef00ff0ac7`.
The client image and its function RVAs in `reference.lock.json` remain different.
CrossOver reports version 26.3 (installed product 26.3.0.39832), win10_64 template.

The launcher creates an APFS clone of the server, a new bottle and a new persistent
profile in an external work directory. The existing user bottle and saves are not
used. Game/query bind is 127.0.0.1, public listings and analytics are disabled,
RCON/API are disabled. The observation is bounded; cleanup uses `WINEPREFIX` plus
`wineserver -k/-w` for only the owned bottle. It does not identify/kill arbitrary
Wine processes. The six pinned source and clone files still match after the run.

```sh
python3 tools/probe_crossover_server.py \
  --server ../VRising_Server \
  --work-dir /private/tmp/rrising-reference-new-run \
  --duration 90
```

The work directory must not already exist or overlap the repository, original
server or CrossOver. It contains **private original files and saves** and must not
be committed. Only derived observations and hashes are published here.

## Observed result, 2026-11-17

[Machine-readable report](../docs/verification/crossover-server.json): world startup
first observed at 17.055 s; 261 streamed-scene events and 248 loaded-chunk events.
During the 90 s observation the original completed **six autosaves**. The saved
worlds have roughly 274,000 entities and 2,671–2,673 persistent chunks; their debug
dumps expose the original included/excluded archetype component sets. Two gzip
save payloads and all six text dumps remained after original autosave retention.
The snapshot sizes are about 7.6 MB compressed / 36 MB uncompressed.

`AutoSaveInterval=10`, variance zero, `DumpPersistenceSummaryOnSave=true` and
`StorePersistenceDebugData=true` are acknowledged in the original log. These are
real original persistence outputs, not reconstructed Rust state. Their timestamps,
AI and other world updates are not deterministic across runs. The built-in debug
mode logs a separate-world load test, but no independently checked state equivalence
has been established from that message.

Known failure: `Unable to Initialize Steam GameServer` is logged during bootstrap.
World startup and autosaves nevertheless complete. Network authentication, client
connection and multiplayer are untested. The deadline uses a forced owned-prefix
stop (wrapper exit 1), not a graceful game shutdown; `wineserver -w` then exits 0.

An earlier REA `cmd.exe /c ver` capture observed exit 0, but its ownership checks
could not classify Wine helper processes during cleanup. That provider limitation
is not reported as a clean runtime capture. The reproducible launcher and its own
private-prefix cleanup were used for the server observation above.

## Independent capture, 2026-11-21

A fresh isolated CrossOver profile reached world startup after 52.325 seconds
and produced three autosaves during the 90-second observation. The retained
`AutoSave_2` was parsed and projected by the Rust CLI using a freshly generated
server layout report. The projection allocated 552,220 live entities, including
274,802 saved job rows and 236,213 placed MegaStatic instances. It restored
260 component-column types and 49 tag types, with 18 column types and six tag
types skipped. It copied 1,830,217 scalar values and 277,113 buffer rows.
A regenerated report also includes 1,381 portable server value-type layouts
that do not occur in the scene hash set. Their metadata-derived identities are
matched to the saved stable hashes and their stride and flags checked by the
loader. With this report the same capture admits 271 column types and skips
seven. With owned prefab blob storage, the projection also restores
`BloodConsumeSource`, including its blob address, bringing the current result
to 273 admitted types and five skipped. The projection also restores
`IdleInteractionPointBuffer` and `IdleInteractor`, bringing the total to
275 admitted types and three skipped. On both retained autosaves a test checks
1,466 buffer elements and one scalar against saved bytes and the source prefab
blob and excluded fields. Null blob sources are covered by a modified-input regression test. Non-null
prefab entity references in these two components are not yet handled. Both retained autosaves passed the
component byte and blob address check. Blob addresses remain valid while the
projection owns their storage. This is a storage-level check, not an
original-system replay.
Twelve stale-version references were cleared. This capture was generated
independently from the previous two saves and is not a gameplay comparison.

## Next comparison boundary

The [outer save container is now readable in Rust](save_container.md); both
remaining original files match captured member ranges and payload fingerprints.
The [header decoder](save_header.md) also recovers the complete saved type registry
and prefab/blob metadata. The [job reader](save_jobs.md) indexes saved archetypes,
entities and component columns. No original save is loaded into the Rust World
yet. No fixed seed/time control, scripted
player input, original-client screenshot or original-vs-Rust state comparison
was captured. Original-game parity scenarios therefore remain **zero**.
