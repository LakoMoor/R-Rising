#!/usr/bin/env bash
# Local diagnostic only. Reads the owner's pinned game files; never copies them into the repo.
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"

usage() {
  cat <<'EOF'
Usage: tools/run_player_prototype.sh [--check]

VRISING_DATA             path to VRising_Data (default: macOS Steam installation)
RRISING_LAYOUT_REPORT    optional existing matching client layout JSON
RRISING_PLAYER_POSITION  optional diagnostic x,y,z position (not an original spawn)
RRISING_SCENE            optional scene .0.entities path

Without a layout report, the script derives one in a temporary directory.
--check validates paths and pinned hashes without building or opening a window.
EOF
}
if [[ ${1:-} == --help || ${1:-} == -h ]]; then usage; exit 0; fi
if [[ $# -gt 1 || ( $# -eq 1 && $1 != --check ) ]]; then usage >&2; exit 2; fi
check_only=${1:-}

data=${VRISING_DATA:-"$HOME/Library/Application Support/Steam/steamapps/common/VRising/VRising_Data"}
install=$(dirname "$data")
scene=${RRISING_SCENE:-"$data/StreamingAssets/EntityScenes/c6d34432693f24046ae5916650cc70ef.0.entities"}
report=${RRISING_LAYOUT_REPORT:-}
for file in "$install/GameAssembly.dll" "$data/il2cpp_data/Metadata/global-metadata.dat" "$scene" assets/typetrees.tpk; do
  [[ -f "$file" ]] || { printf 'Missing required file: %s\n' "$file" >&2; exit 1; }
done
command -v python3 >/dev/null || { echo 'python3 is required' >&2; exit 1; }
command -v cargo >/dev/null || { echo 'cargo is required' >&2; exit 1; }

python3 - "$install/GameAssembly.dll" "$data/il2cpp_data/Metadata/global-metadata.dat" "$report" <<'PY'
import hashlib, json, sys
expected = ('a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56',
            'b8f9f76ffb72a0c77079082aac8eb78da839094e9d090e8461b49230e8831271')
for path, digest in zip(sys.argv[1:3], expected):
    with open(path, 'rb') as f:
        h = hashlib.sha256()
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
        actual = h.hexdigest()
    if actual != digest:
        sys.exit(f'Pinned build hash mismatch: {path}')
if sys.argv[3]:
    with open(sys.argv[3], encoding='utf-8') as f:
        layout = json.load(f)
    if (layout.get('binary_sha256'), layout.get('metadata_sha256')) != expected:
        sys.exit(f'Layout report does not match pinned client: {sys.argv[3]}')
print('Pinned client hashes verified.')
PY

if [[ $check_only == --check ]]; then
  printf 'Scene: %s\n' "$scene"
  if [[ -n $report ]]; then printf 'Layout: %s\n' "$report"; else echo 'Layout will be generated at launch.'; fi
  exit 0
fi

temporary_dir=''
cleanup() { if [[ -n $temporary_dir ]]; then rm -rf -- "$temporary_dir"; fi; }
trap cleanup EXIT
if [[ -z $report ]]; then
  temporary_dir=$(mktemp -d "${TMPDIR:-/tmp}/rrising-layout.XXXXXXXX")
  report="$temporary_dir/layout.json"
  echo 'Deriving a temporary client layout report; this may take a while.'
  python3 tools/probe_type_registry.py --install "$install" --check --output "$report"
fi
args=(--data "$data" --scene "$scene" --type-layouts "$report" --archives 100000
      --scene-live --spawn-player --experimental-motion --follow-player --animate-player)
position=${RRISING_PLAYER_POSITION:-}
if [[ -z $position && -z ${RRISING_SCENE:-} ]]; then position='142.5,20,22.5'; fi
if [[ -n $position ]]; then args+=(--player-position "$position"); fi
cat <<'EOF'
Experimental diagnostic: movement, camera and face attachment are approximations; no collision,
complete materials, gameplay animation state or original spawn. WASD moves the player,
arrow keys turn the camera, Q/E move it vertically. Rest/walk selection is approximate.
EOF
if [[ -n $position ]]; then printf 'Diagnostic position: %s (no collision check).\n' "$position"; fi
cargo run --locked -p rrising-app -- "${args[@]}"
