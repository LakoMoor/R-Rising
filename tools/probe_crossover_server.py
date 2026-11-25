#!/usr/bin/env python3
"""Observe the pinned original server in a fresh, private CrossOver bottle.

macOS/APFS only. Game files, logs, saves and persistence debug data stay in the
required external work directory. This observes startup/save behavior, not
deterministic gameplay or Rust parity. No existing bottle or save is reused.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import signal
import socket
import subprocess
import sys
import time

REPO = Path(__file__).resolve().parents[1]
STARTUP = "[Server] Startup Completed - Disabling Scene Loading Systems"


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify(root: Path, files: list[dict]) -> bool:
    return all((root / f["path"]).stat().st_size == f["size"]
               and digest(root / f["path"]) == f["sha256"] for f in files)


def windows(path: Path) -> str:
    return "Z:" + str(path).replace("/", "\\")


def inventory(root: Path) -> list[dict]:
    return [{"path": str(p.relative_to(root)), "size": p.stat().st_size,
             "sha256": digest(p)} for p in sorted(root.rglob("*")) if p.is_file()]


def persistence_events(text: str) -> list[dict]:
    pattern = (r"PersistenceV2 - Finished Saving to '[^\n]*AutoSave_(\d+)\.save\.gz'\. "
               r"Total Persistent ChunkCount (\d+)\. Total Save Size: (\d+) "
               r"Total Uncompressed Save Size: (\d+)")
    return [dict(zip(("autosave_index", "persistent_chunks", "compressed_bytes",
                      "uncompressed_bytes"), map(int, row)))
            for row in re.findall(pattern, text)]


def observe(args: argparse.Namespace) -> dict:
    source = args.server.resolve(strict=True)
    work = args.work_dir.resolve()
    app = args.crossover.resolve(strict=True)
    base = app / "Contents/SharedSupport/CrossOver"
    # Fail before creating anything if the caller points at a source tree or
    # public repository. The output includes proprietary game/save payloads.
    if any(work == p or work.is_relative_to(p) or p.is_relative_to(work)
           for p in (REPO, source, app)):
        raise ValueError("work-dir must be outside the repository, game and CrossOver")
    if work.exists():
        raise ValueError("work-dir must be new; existing bottles/saves are never reused")
    if not 10 <= args.duration <= 600:
        raise ValueError("duration must be in 10..600 seconds")
    if not 1024 <= args.port <= 65534:
        raise ValueError("port must be in 1024..65534 (query port is port + 1)")
    for port in (args.port, args.port + 1):
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.bind(("127.0.0.1", port))
    reference = json.loads((REPO / "reference.server.lock.json").read_text())
    if not verify(source, reference["files"]):
        raise ValueError("server does not match reference.server.lock.json")
    with (app / "Contents/Info.plist").open("rb") as stream:
        crossover_version = plistlib.load(stream).get("CFBundleShortVersionString")

    work.mkdir(parents=True)
    clone = work / "server"
    prefix = work / "bottles/reference"
    prefix.parent.mkdir()
    profile = work / "profile"
    (profile / "Settings").mkdir(parents=True)
    settings = {
        "Name": "R-Rising Reference", "Address": "127.0.0.1",
        "Port": args.port, "QueryPort": args.port + 1,
        "MaxConnectedUsers": 1, "MaxConnectedAdmins": 1,
        "SaveName": "rrising-reference", "LanMode": True, "Secure": False,
        "ListOnSteam": False, "ListOnEOS": False, "AnalyticsEnabled": False,
        "AutoSaveCount": 1, "AutoSaveInterval": 10, "AutoSaveIntervalVariance": 0,
        "DumpPersistenceSummaryOnSave": True, "StorePersistenceDebugData": True,
        "ServerFps": 30, "LowerFPSWhenEmpty": False,
        "Rcon": {"Enabled": False, "BindAddress": "127.0.0.1"},
        "API": {"Enabled": False, "BindAddress": "127.0.0.1"},
    }
    (profile / "Settings/ServerHostSettings.json").write_text(
        json.dumps(settings, indent=2) + "\n")
    env = os.environ.copy()
    env.update(WINEPREFIX=str(prefix), CX_BOTTLE_PATH=str(prefix.parent),
               WINEDEBUG="-all", SteamAppId="1604030")
    print("Cloning the pinned server and creating an isolated win10_64 bottle", flush=True)
    subprocess.run(["/bin/cp", "-cR", str(source), str(clone)], check=True)
    with (work / "bottle-setup.log").open("w") as out:
        subprocess.run([str(base / "bin/cxbottle"), "--bottle", str(prefix),
                        "--create", "--template", "win10_64", "--description",
                        "R-Rising disposable original-server observation"],
                       env=env, stdout=out, stderr=subprocess.STDOUT, check=True, timeout=900)
    log = work / "server-unity.log"
    command = [str(base / "bin/wine"), "--bottle", str(prefix), "--no-gui",
               "--wait-children", "--workdir", str(clone), "--cx-app",
               windows(clone / "VRisingServer.exe"), "-batchmode", "-nographics",
               "-persistentDataPath", windows(profile), "-logFile", windows(log),
               "-bindAddress", "127.0.0.1", "-lanMode", "true",
               "-listOnSteam", "false", "-listOnEOS", "false",
               "-saveName", "rrising-reference", "-saveInterval", "10",
               "-saveVariance", "0", "-dumpPersistenceSummaryOnSave", "true",
               "-storePersistenceDebugData", "true"]
    started = time.monotonic()
    startup_seconds = None
    cleanup = "not_needed"
    reason = "exited"
    with (work / "server-wine.log").open("w") as out:
        process = subprocess.Popen(command, cwd=clone, env=env, stdout=out,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        try:
            while process.poll() is None:
                elapsed = time.monotonic() - started
                text = log.read_text(errors="replace") if log.exists() else ""
                if startup_seconds is None and STARTUP in text:
                    startup_seconds = round(elapsed, 3)
                    print(f"Original server completed world startup at {startup_seconds}s", flush=True)
                if elapsed >= args.duration:
                    reason = "bounded_observation"
                    break
                time.sleep(min(1, args.duration - elapsed))
        except KeyboardInterrupt:
            reason = "interrupted"
        finally:
            # wineserver has no --bottle option. WINEPREFIX scopes these commands
            # to the brand-new prefix owned by this invocation, never a user bottle.
            if process.poll() is None:
                cleanup = "owned_prefix_wineserver_kill"
                subprocess.run([str(base / "bin/wineserver"), "-k"], env=env,
                               stdout=out, stderr=subprocess.STDOUT, timeout=15, check=True)
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
                    cleanup += ";owned_wrapper_kill"
            services_exit = subprocess.run(
                [str(base / "bin/wineserver"), "-w"], env=env, stdout=out,
                stderr=subprocess.STDOUT, timeout=15, check=False).returncode

    text = log.read_text(errors="replace") if log.exists() else ""
    report = {
        "observed_at_utc": datetime.now(timezone.utc).isoformat(),
        "kind": "original-server-startup-and-persistence-observation",
        "reference": "reference.server.lock.json", "original_build": reference["build"],
        "crossover_version": crossover_version, "bottle_template": "win10_64",
        "profile_settings": settings, "requested_observation_seconds": args.duration,
        "elapsed_seconds": round(time.monotonic() - started, 3),
        "startup_completed": STARTUP in text, "startup_first_observed_seconds": startup_seconds,
        "streamed_scene_log_events": text.count("Streamed scene with"),
        "loaded_chunk_log_events": text.count("[Server] Loaded Chunk "),
        "steam_gameserver_initialization_error": "Unable to Initialize Steam GameServer" in text,
        "completed_persistence_log_events": persistence_events(text),
        "exit_code": process.returncode, "reason": reason, "cleanup": cleanup,
        "owned_wineserver_wait_exit_code": services_exit,
        "source_pinned_files_unchanged": verify(source, reference["files"]),
        "clone_pinned_files_unchanged": verify(clone, reference["files"]),
        "unity_log": {"size": log.stat().st_size, "sha256": digest(log)} if log.exists() else None,
        "profile_files": inventory(profile),
        "coverage": {"scripted_player_inputs": 0, "rust_state_comparisons": 0,
                     "deterministic_gameplay_scenarios": 0},
    }
    (work / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--crossover", type=Path,
                        default=Path.home() / "Applications/CrossOver.app")
    parser.add_argument("--duration", type=int, default=90)
    parser.add_argument("--port", type=int, default=19876)
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("requires macOS with CrossOver and APFS clone support")
    print(json.dumps(observe(args), indent=2))


if __name__ == "__main__":
    main()
